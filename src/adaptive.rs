//! Behavioral prefix built from SQLite session index. Written to `adaptive_prefix.md` on ingest.

use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// Truncate UTF-8 string to at most `max` chars (character count, not bytes).
pub fn truncate_to_char_budget(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for ch in s.chars() {
        if out.chars().count() >= max.saturating_sub(1) {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

/// Max chars for regeneration: config override, else model hint from latest enriched hook trace, capped at 2000.
pub fn rebuild_max_chars_for_db(conn: &Connection) -> usize {
    let cfg = crate::config::Config::load();
    if let Some(m) = cfg.adaptive_prefix_max_chars {
        return m.clamp(100, 8000);
    }
    let hint: Option<String> = conn
        .query_row(
            "SELECT LOWER(COALESCE(model, '')) FROM hook_traces \
             WHERE model IS NOT NULL AND LENGTH(TRIM(model)) > 0 \
             ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    let h = hint.as_deref().unwrap_or("");
    if h.contains("haiku") {
        1280_usize
    } else {
        2000
    }
}

/// Per-hook call: config override or model string from hook JSON (Sonnet default 2000).
pub fn max_chars_for_hook_input(model_hint: Option<&str>) -> usize {
    let cfg = crate::config::Config::load();
    if let Some(m) = cfg.adaptive_prefix_max_chars {
        return m.clamp(100, 8000);
    }
    let h = model_hint.unwrap_or("").to_lowercase();
    if h.contains("haiku") {
        1280_usize
    } else {
        2000
    }
}

fn section_budgets(max_chars: usize) -> [usize; 5] {
    // Priority order: anti-patterns, tools, coding style, task types, session norms — larger share to higher priority.
    let w = [3usize, 3, 2, 2, 2];
    let sum: usize = w.iter().sum();
    let base = max_chars / sum;
    let rem = max_chars.saturating_sub(base * sum);
    let mut out = [
        base * w[0],
        base * w[1],
        base * w[2],
        base * w[3],
        base * w[4],
    ];
    for item in out.iter_mut().take(rem.min(5)) {
        *item += 1;
    }
    out
}

fn query_correction_snippets(conn: &Connection, budget: usize) -> String {
    let mut stmt = match conn.prepare(
        "SELECT DISTINCT TRIM(human_text_prefix) AS p FROM turns \
         WHERE human_text_prefix IS NOT NULL \
           AND LENGTH(TRIM(human_text_prefix)) > 8 \
           AND (flags LIKE '%correction%' OR flags LIKE '%\"correction\"%') \
         ORDER BY id DESC LIMIT 24",
    ) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };
    let rows: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .into_iter()
        .flatten()
        .filter_map(|x| x.ok())
        .collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut bullets: Vec<String> = Vec::new();
    for p in rows {
        let key: String = p.chars().take(24).collect::<String>().to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        let line = truncate_to_char_budget(p.trim(), 100);
        if !line.is_empty() {
            bullets.push(format!("- {line}"));
        }
        if bullets.join("\n").chars().count() >= budget.saturating_sub(40) {
            break;
        }
    }
    if bullets.is_empty() {
        return String::new();
    }
    let head = "## Correction and coaching signals\n";
    let body = bullets.join("\n");
    truncate_to_char_budget(&(head.to_string() + &body), budget)
}

fn query_tool_patterns(conn: &Connection, budget: usize) -> String {
    let mut stmt = match conn.prepare(
        "SELECT server_prefix, CAST(COUNT(*) AS INTEGER) AS n FROM tool_invocations \
         WHERE ts >= datetime('now', '-30 days') \
         GROUP BY server_prefix ORDER BY n DESC LIMIT 12",
    ) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };
    let pairs: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .into_iter()
        .flatten()
        .filter_map(|x| x.ok())
        .collect();
    if pairs.is_empty() {
        return String::new();
    }
    let top: Vec<_> = pairs.iter().take(5).cloned().collect();
    let heavy = top
        .iter()
        .map(|(s, n)| format!("{s} ({n}×)"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut tail = String::new();
    if pairs.len() > 5 {
        let rare: Vec<_> = pairs
            .iter()
            .skip(8)
            .take(4)
            .map(|(s, _)| s.as_str())
            .collect();
        if !rare.is_empty() {
            tail = format!("\nLess active recently: {}.", rare.join(", "));
        }
    }
    let text = format!("## Tool usage\nHeavier use on: {heavy}.{tail}");
    truncate_to_char_budget(&text, budget)
}

static LANG_KEYS: &[&str] = &[
    "rust",
    "python",
    "typescript",
    "javascript",
    "go",
    "java",
    "kotlin",
    "swift",
    "ruby",
    "php",
    "c++",
    "cpp",
    "react",
    "vue",
    "svelte",
    "next",
    "node",
    "django",
    "rails",
];

fn query_coding_style(conn: &Connection, budget: usize) -> String {
    let mut stmt = match conn.prepare(
        "SELECT LOWER(human_text_prefix) FROM turns \
         WHERE human_text_prefix IS NOT NULL AND LENGTH(TRIM(human_text_prefix)) > 0 \
         ORDER BY id DESC LIMIT 400",
    ) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    for row in stmt
        .query_map([], |r| r.get::<_, String>(0))
        .into_iter()
        .flatten()
        .filter_map(|x| x.ok())
    {
        for k in LANG_KEYS {
            if row.contains(k) {
                *counts.entry(*k).or_insert(0) += 1;
            }
        }
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by_key(|row| std::cmp::Reverse(row.1));
    let top: Vec<_> = v.into_iter().take(6).map(|(k, _)| k).collect();
    if top.is_empty() {
        return String::new();
    }
    let text = format!(
        "## Coding style hints\nRecent user messages often mention: {}.",
        top.join(", ")
    );
    truncate_to_char_budget(&text, budget)
}

fn classify_task(text: &str) -> Option<&'static str> {
    let t = text.to_lowercase();
    if t.contains("refactor") || t.contains("rename") || t.contains("migrate") {
        Some("refactoring")
    } else if t.contains("bug") || t.contains("fix") || t.contains("error") || t.contains("debug") {
        Some("debugging")
    } else if t.contains("review") || t.contains("pr ") || t.contains("pull request") {
        Some("reviewing")
    } else if t.contains("plan") || t.contains("design") || t.contains("roadmap") {
        Some("planning")
    } else if t.contains("implement")
        || t.contains("add ")
        || t.contains("write ")
        || t.contains("create ")
    {
        Some("implementation")
    } else {
        None
    }
}

fn query_task_distribution(conn: &Connection, budget: usize) -> String {
    let mut stmt = match conn.prepare(
        "SELECT COALESCE(first_user_message, ''), COALESCE(embed_text, '') FROM sessions \
         WHERE (first_user_message IS NOT NULL AND LENGTH(TRIM(first_user_message)) > 0) \
            OR (embed_text IS NOT NULL AND LENGTH(TRIM(embed_text)) > 0) \
         ORDER BY id DESC LIMIT 300",
    ) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };
    let mut buckets: HashMap<&'static str, usize> = HashMap::new();
    for row in stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .into_iter()
        .flatten()
        .filter_map(|x| x.ok())
    {
        let (a, b) = row;
        let merged = format!("{a} {b}");
        if let Some(cat) = classify_task(&merged) {
            *buckets.entry(cat).or_insert(0) += 1;
        }
    }
    if buckets.is_empty() {
        return String::new();
    }
    let mut v: Vec<_> = buckets.into_iter().collect();
    v.sort_by_key(|row| std::cmp::Reverse(row.1));
    let parts: Vec<String> = v
        .iter()
        .take(4)
        .map(|(k, n)| format!("{k} ({n})"))
        .collect();
    let text = format!(
        "## Task mix (from first prompts)\nMost common: {}.",
        parts.join(", ")
    );
    truncate_to_char_budget(&text, budget)
}

fn session_norms_line(budget: usize) -> String {
    let p = crate::user_profile::UserProfile::compute();
    if !p.calibrated && p.session_count < 3 {
        return String::new();
    }
    let text = format!(
        "## Session norms\nTypical sessions run about {} turns. Long runs often exceed {} turns — consider splitting work.",
        p.median_session_turns, p.long_session_threshold
    );
    truncate_to_char_budget(&text, budget)
}

/// Build markdown adaptive prefix, respecting `max_chars` total.
pub fn generate_adaptive_prefix(conn: &Connection, max_chars: usize) -> String {
    let max_chars = max_chars.clamp(200, 2000);
    let b = section_budgets(max_chars);
    let s0 = query_correction_snippets(conn, b[0]);
    let s1 = query_tool_patterns(conn, b[1]);
    let s2 = query_coding_style(conn, b[2]);
    let s3 = query_task_distribution(conn, b[3]);
    let s4 = session_norms_line(b[4]);
    let mut parts: Vec<String> = Vec::new();
    for s in [s0, s1, s2, s3, s4] {
        if !s.trim().is_empty() {
            parts.push(s);
        }
    }
    let mut body = parts.join("\n\n");
    if body.is_empty() {
        body = "(Not enough indexed history yet. Use Claude Code with ctx ingest enabled, then refresh.)".into();
    }
    let header = "# ctx adaptive profile\n\n";
    let mut out = format!("{header}{body}");
    out = truncate_to_char_budget(&out, max_chars);
    out
}

/// Regenerate file from DB.
pub fn refresh_adaptive_prefix() -> Result<()> {
    let cfg = crate::config::Config::load();
    if !cfg.adaptive_prefix_enabled {
        return Ok(());
    }
    regenerate_adaptive_prefix_file()
}

/// Rewrite `adaptive_prefix.md` from SQLite regardless of `adaptive_prefix_enabled` (dashboard **Regenerate**).
pub fn regenerate_adaptive_prefix_file() -> Result<()> {
    let conn = crate::db::open_db()?;
    crate::db::ensure_schema(&conn)?;
    let max = rebuild_max_chars_for_db(&conn);
    let text = generate_adaptive_prefix(&conn, max);
    crate::config::ensure_dir()?;
    std::fs::write(crate::config::adaptive_prefix_path(), &text)?;
    Ok(())
}

/// Cached file from disk (hook hot path, no DB).
pub fn load_adaptive_prefix() -> Option<String> {
    let p = crate::config::adaptive_prefix_path();
    let s = std::fs::read_to_string(&p).ok()?;
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_budget() {
        let s = "a".repeat(50);
        let t = truncate_to_char_budget(&s, 10);
        assert!(t.chars().count() <= 10);
    }

    #[test]
    fn max_chars_hook_haiku() {
        std::env::remove_var("CTX_HOME");
        // default config file may exist on dev machine; test logic only
        let m = max_chars_for_hook_input(Some("claude-3-5-haiku"));
        assert!(m <= 2000);
    }
}
