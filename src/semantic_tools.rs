//! Semantic tool mix from similar sessions and access-friction recovery.

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::config::{Config, FilterMode};
use crate::profiles::{self, Profile};

pub const META_TOOL_MIX_LAST: &str = "semantic_tool_mix_last";
pub const META_ACCESS_FRICTION: &str = "access_friction_counts";
pub const MAX_PROACTIVE_EXPANSIONS: usize = 6;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExpansionReason {
    Keyword,
    Semantic,
    AccessFriction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolExpansionEntry {
    pub target: String,
    pub reason: ExpansionReason,
    pub display: String,
}

impl ToolExpansionEntry {
    pub fn new(target: impl Into<String>, reason: ExpansionReason) -> Self {
        let target = target.into();
        let display = display_name_for_target(&target);
        Self {
            target,
            reason,
            display,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMixSummary {
    pub enabled: bool,
    pub source: String,
    pub neighbor_count: usize,
    pub tools: Vec<String>,
}

impl Default for ToolMixSummary {
    fn default() -> Self {
        Self {
            enabled: false,
            source: "profile-only".into(),
            neighbor_count: 0,
            tools: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessFrictionRow {
    pub tool: String,
    pub tool_display: String,
    pub count: u32,
}

pub fn display_name_for_target(target: &str) -> String {
    if target.starts_with("mcp__") {
        target
            .rsplit("__")
            .next()
            .unwrap_or(target)
            .replace('_', " ")
    } else {
        target.replace('_', " ")
    }
}

/// Recommend MCP tools to un-deny based on similar past sessions.
pub fn recommend_tools_from_similar_sessions(
    conn: &Connection,
    cwd: &str,
    prompt: &str,
    profile: &Profile,
) -> Result<Vec<String>> {
    let cfg = Config::load();
    if !cfg.embeddings_enabled() || !cfg.semantic_tool_mix_enabled || !profile.filtering_enabled() {
        return Ok(vec![]);
    }

    let embedding_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM session_embeddings", [], |r| r.get(0))
        .unwrap_or(0);
    if embedding_rows < 2 {
        return Ok(vec![]);
    }

    let query_text = format!("[dir: {}] {}", cwd.trim(), prompt.trim());
    let embedding = crate::embedder::embed_text(&query_text)?;
    let top_k = cfg.semantic_tool_mix_top_k.clamp(1, 20);
    let sims = crate::embedder::similar_sessions_by_query(conn, &embedding, top_k, None)?;

    let min_sim = cfg.semantic_tool_mix_min_similarity as f64;
    let min_frac = cfg.semantic_tool_mix_min_neighbor_fraction as f64;

    let qualifying: Vec<(i64, f32)> = sims
        .into_iter()
        .filter(|(_, sim)| *sim as f64 >= min_sim)
        .collect();
    if qualifying.len() < 2 {
        return Ok(vec![]);
    }

    let mut tool_weights: HashMap<String, f64> = HashMap::new();
    let mut tool_sessions: HashMap<String, HashSet<i64>> = HashMap::new();

    for (session_pk, sim) in &qualifying {
        let tools = tools_for_session(conn, *session_pk)?;
        let weight = *sim as f64;
        for tool in tools {
            *tool_weights.entry(tool.clone()).or_insert(0.0) += weight;
            tool_sessions.entry(tool).or_default().insert(*session_pk);
        }
    }

    let neighbor_n = qualifying.len() as f64;
    let min_sessions = (neighbor_n * min_frac).ceil() as usize;

    let profile_kept: HashSet<String> = if profile.uses_tool_level() {
        profile.keep_tools.iter().cloned().collect()
    } else {
        HashSet::new()
    };

    let mut recommended: Vec<String> = tool_weights
        .into_keys()
        .filter_map(|tool| {
            let sessions_with = tool_sessions.get(&tool).map(|s| s.len()).unwrap_or(0);
            if sessions_with < min_sessions {
                return None;
            }
            if profile.uses_tool_level() && profile_kept.contains(&tool) {
                return None;
            }
            if profile.uses_tool_level() && !profile.filters_tool(&tool) {
                return None;
            }
            Some(tool)
        })
        .collect();

    recommended.sort();
    recommended.dedup();
    recommended.truncate(MAX_PROACTIVE_EXPANSIONS);
    Ok(recommended)
}

fn tools_for_session(conn: &Connection, session_pk: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT tool_name FROM tool_invocations WHERE session_id = ?1 ORDER BY tool_name",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![session_pk], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn persist_tool_mix_summary(conn: &Connection, summary: &ToolMixSummary) -> Result<()> {
    let json = serde_json::to_string(summary)?;
    conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES (?1, ?2)",
        rusqlite::params![META_TOOL_MIX_LAST, json],
    )?;
    Ok(())
}

fn resync_soft_filter_settings() -> Result<()> {
    let cfg = Config::load();
    if cfg.filter_mode != FilterMode::Soft {
        return Ok(());
    }
    let slug = cfg.active_profile.as_deref().unwrap_or("all");
    let dash = cfg.dashboard_port.unwrap_or(8789);
    crate::claude_settings::write_native_ctx_to_user_settings(slug, dash)?;
    Ok(())
}

/// Add session expansion targets and resync deny rules when anything new was added.
pub fn add_session_expansions(
    targets: impl IntoIterator<Item = (String, ExpansionReason)>,
) -> Result<Vec<ToolExpansionEntry>> {
    let mut cfg = Config::load();
    if cfg.filter_mode != FilterMode::Soft {
        return Ok(vec![]);
    }

    let mut added = Vec::new();
    let mut changed = false;

    for (target, reason) in targets {
        let key = target.trim();
        if key.is_empty() {
            continue;
        }
        let already = cfg
            .session_expansion
            .iter()
            .any(|s| s.eq_ignore_ascii_case(key))
            || cfg
                .session_semantic_tools
                .iter()
                .any(|s| s.eq_ignore_ascii_case(key));
        if already {
            continue;
        }
        if reason == ExpansionReason::Semantic {
            cfg.session_semantic_tools.push(key.to_string());
        }
        cfg.session_expansion.push(key.to_string());
        added.push(ToolExpansionEntry::new(key, reason));
        changed = true;
    }

    if changed {
        cfg.save()?;
        resync_soft_filter_settings()?;
    }
    Ok(added)
}

/// Proactive keyword expansion from prompt text (Tier 2).
pub fn expand_from_prompt_keywords(
    prompt: &str,
    cwd: &str,
    profile: &Profile,
) -> Result<Vec<ToolExpansionEntry>> {
    if !profile.filtering_enabled() {
        return Ok(vec![]);
    }
    let mut candidates = profiles::detect_expansion_candidates(prompt, cwd, profile);
    candidates.sort();
    candidates.dedup();
    candidates.truncate(MAX_PROACTIVE_EXPANSIONS);
    add_session_expansions(
        candidates
            .into_iter()
            .map(|t| (t, ExpansionReason::Keyword)),
    )
}

/// Hook entry: compute semantic overlay, persist summary, return newly expanded tools.
pub fn apply_hook_semantic_tool_mix(
    new_slug: &str,
    prompt: &str,
    cwd: &str,
) -> Result<Vec<ToolExpansionEntry>> {
    let cfg = Config::load();
    if !cfg.semantic_tool_mix_enabled || cfg.filter_mode != FilterMode::Soft {
        return Ok(vec![]);
    }

    let profile = profiles::get(new_slug)?;
    let Ok(conn) = crate::db::open_db() else {
        return Ok(vec![]);
    };
    let _ = crate::db::ensure_schema(&conn);

    let tools =
        recommend_tools_from_similar_sessions(&conn, cwd, prompt, &profile).unwrap_or_default();
    let added = add_session_expansions(
        tools
            .iter()
            .cloned()
            .map(|t| (t, ExpansionReason::Semantic)),
    )?;

    let summary = if tools.is_empty() {
        ToolMixSummary {
            enabled: true,
            source: "profile-only".into(),
            neighbor_count: 0,
            tools: vec![],
        }
    } else {
        let query_text = format!("[dir: {}] {}", cwd.trim(), prompt.trim());
        let neighbor_count = if cfg.embeddings_enabled() {
            crate::embedder::embed_text(&query_text)
                .ok()
                .and_then(|emb| {
                    crate::embedder::similar_sessions_by_query(
                        &conn,
                        &emb,
                        cfg.semantic_tool_mix_top_k.clamp(1, 20),
                        None,
                    )
                    .ok()
                })
                .map(|s| {
                    s.iter()
                        .filter(|(_, sim)| {
                            *sim as f64 >= cfg.semantic_tool_mix_min_similarity as f64
                        })
                        .count()
                })
                .unwrap_or(0)
        } else {
            0
        };
        ToolMixSummary {
            enabled: true,
            source: "semantic".into(),
            neighbor_count,
            tools: tools.clone(),
        }
    };

    let _ = persist_tool_mix_summary(&conn, &summary);
    Ok(added)
}

const ACCESS_PATTERNS: &[&str] = &[
    "don't have access to",
    "do not have access to",
    "don't have access",
    "cannot access",
    "can't access",
    "can't use the",
    "cannot use the",
    "isn't available",
    "is not available",
    "not available",
    "don't have permission",
    "no access to",
];

/// Scan assistant text for access-friction signals against denied tools. Covers both the active
/// profile's denied tools and any servers pruned from the tool menu (CTX-64), so a reach for a
/// pruned server re-adds it for the session even when the profile itself would have kept it.
pub fn detect_access_friction_tools(text: &str, profile: &Profile) -> Vec<String> {
    if text.trim().is_empty() {
        return vec![];
    }
    let lower = text.to_lowercase();
    if !ACCESS_PATTERNS.iter().any(|p| lower.contains(p)) {
        return vec![];
    }
    let mut out = if profile.filtering_enabled() {
        profiles::detect_expansion_candidates(text, "", profile)
    } else {
        vec![]
    };
    for prefix in &Config::load().pruned_servers {
        let display = profiles::mcp_prefix_to_server_display(prefix).to_lowercase();
        let id = profiles::mcp_prefix_to_server_id(prefix)
            .to_lowercase()
            .replace('_', " ");
        if (!display.is_empty() && lower.contains(&display))
            || (!id.is_empty() && lower.contains(&id))
        {
            out.push(prefix.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

fn load_friction_counts(conn: &Connection) -> HashMap<String, u32> {
    let Some(json) = crate::db::get_meta(conn, META_ACCESS_FRICTION) else {
        return HashMap::new();
    };
    serde_json::from_str(&json).unwrap_or_default()
}

fn save_friction_counts(conn: &Connection, counts: &HashMap<String, u32>) -> Result<()> {
    let json = serde_json::to_string(counts)?;
    conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES (?1, ?2)",
        rusqlite::params![META_ACCESS_FRICTION, json],
    )?;
    Ok(())
}

/// Record access friction and expand session targets immediately.
pub fn record_access_friction(tools: &[String]) -> Result<Vec<ToolExpansionEntry>> {
    if tools.is_empty() {
        return Ok(vec![]);
    }
    let Ok(conn) = crate::db::open_db() else {
        return Ok(vec![]);
    };
    let _ = crate::db::ensure_schema(&conn);

    let mut counts = load_friction_counts(&conn);
    for tool in tools {
        *counts.entry(tool.clone()).or_insert(0) += 1;
    }
    let _ = save_friction_counts(&conn, &counts);

    add_session_expansions(
        tools
            .iter()
            .cloned()
            .map(|t| (t, ExpansionReason::AccessFriction)),
    )
}

fn extract_text_from_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b.get("text").and_then(|t| t.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Pull the latest assistant prose from a Stop/SessionEnd hook payload.
pub fn extract_assistant_text_from_hook_payload(payload: &Value) -> String {
    for key in [
        "assistant_message",
        "assistantMessage",
        "last_assistant_message",
        "lastAssistantMessage",
    ] {
        if let Some(v) = payload.get(key) {
            if let Some(s) = v.as_str() {
                if !s.trim().is_empty() {
                    return s.to_string();
                }
            }
            if let Some(msg) = v.get("content") {
                let text = extract_text_from_content(msg);
                if !text.trim().is_empty() {
                    return text;
                }
            }
        }
    }
    if let Some(msg) = payload.get("message") {
        if let Some(content) = msg.get("content") {
            let text = extract_text_from_content(content);
            if !text.trim().is_empty() {
                return text;
            }
        }
    }
    if let Some(arr) = payload.get("transcript").and_then(|v| v.as_array()) {
        for row in arr.iter().rev() {
            let role = row
                .get("role")
                .or_else(|| row.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if role == "assistant" {
                if let Some(content) = row.get("content") {
                    let text = extract_text_from_content(content);
                    if !text.trim().is_empty() {
                        return text;
                    }
                }
                if let Some(msg) = row.get("message").and_then(|m| m.get("content")) {
                    let text = extract_text_from_content(msg);
                    if !text.trim().is_empty() {
                        return text;
                    }
                }
            }
        }
    }
    String::new()
}

/// Tier 1: on Stop/SessionEnd, un-deny tools Claude said it could not access.
pub fn process_stop_hook_recovery(payload: &Value) -> Result<Vec<ToolExpansionEntry>> {
    let cfg = Config::load();
    if cfg.filter_mode != FilterMode::Soft {
        return Ok(vec![]);
    }

    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("sessionId"))
        .and_then(|v| v.as_str());

    let mut text = extract_assistant_text_from_hook_payload(payload);
    if text.trim().is_empty() {
        if let Some(sid) = session_id {
            text = crate::hook::latest_assistant_text_for_session(sid).unwrap_or_default();
        }
    }
    if text.trim().is_empty() {
        return Ok(vec![]);
    }

    let slug = cfg.active_profile.as_deref().unwrap_or("all");
    let profile = profiles::get(slug)?;
    let tools = detect_access_friction_tools(&text, &profile);
    if tools.is_empty() {
        return Ok(vec![]);
    }

    // Record the tool-miss harm signal (CTX-66 / M-D): each reach for a hidden tool. Recorded here
    // in the live Stop hook only, so the ingest replay path never double-counts the same reach.
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let ts = chrono::Utc::now().to_rfc3339();
        for tool in &tools {
            let prefix =
                crate::filter::server_prefix_from_tool(tool).unwrap_or_else(|| tool.clone());
            let hidden_by = if cfg
                .pruned_servers
                .iter()
                .any(|p| profiles::prefix_covers_expansion_entry(p, &prefix))
            {
                "prune"
            } else {
                "profile"
            };
            let _ = crate::db::insert_tool_miss(&conn, session_id, tool, &prefix, hidden_by, &ts);
        }
    }

    let added = record_access_friction(&tools)?;
    if !added.is_empty() {
        if let (Some(sid), Ok(conn)) = (session_id, crate::db::open_db()) {
            let _ = crate::db::ensure_schema(&conn);
            let _ = crate::db::append_hook_trace_expansions(&conn, sid, &added);
        }
        eprintln!(
            "[ctx] access-friction recovery: un-denied {} tool(s) for next turn",
            added.len()
        );
    }
    Ok(added)
}

pub fn load_tool_mix_summary(conn: &Connection) -> ToolMixSummary {
    let Some(json) = crate::db::get_meta(conn, META_TOOL_MIX_LAST) else {
        return ToolMixSummary::default();
    };
    serde_json::from_str(&json).unwrap_or_default()
}

pub fn list_access_friction(conn: &Connection, promote_threshold: u32) -> Vec<AccessFrictionRow> {
    let counts = load_friction_counts(conn);
    let mut rows: Vec<AccessFrictionRow> = counts
        .into_iter()
        .filter(|(_, c)| *c >= 1)
        .map(|(tool, count)| AccessFrictionRow {
            tool: tool.clone(),
            tool_display: display_name_for_target(&tool),
            count,
        })
        .collect();
    rows.retain(|r| r.count >= promote_threshold.min(1));
    rows.sort_by_key(|row| std::cmp::Reverse(row.count));
    rows
}

/// Promote a tool to profile keep_tools permanently.
pub fn promote_tool_to_profile(tool: &str) -> Result<()> {
    profiles::append_keep_tool_to_active_profile(tool)?;

    let _ = add_session_expansions([(tool.to_string(), ExpansionReason::Keyword)]);

    let cfg = Config::load();
    let slug = cfg.active_profile.as_deref().unwrap_or("all");
    let dash = cfg.dashboard_port.unwrap_or(8789);
    crate::claude_settings::write_native_ctx_to_user_settings(slug, dash)?;

    if let Ok(conn) = crate::db::open_db() {
        let mut counts = load_friction_counts(&conn);
        counts.remove(tool);
        let _ = save_friction_counts(&conn, &counts);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::Profile;
    use crate::test_lock::CTX_ENV_LOCK;

    fn with_ctx_home<F: FnOnce(&tempfile::TempDir)>(f: F) {
        let _guard = CTX_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        f(&tmp);
        std::env::remove_var("CTX_HOME");
    }

    #[test]
    fn detect_access_friction_finds_notion_in_denial_text() {
        with_ctx_home(|_tmp| {
            let conn = crate::db::open_db().unwrap();
            crate::db::ensure_schema(&conn).unwrap();
            let session_id = conn
                .execute(
                    "INSERT INTO sessions (external_key, project, started_at, profile) VALUES ('t', 'p', '2026-01-01', 'design')",
                    [],
                )
                .ok();
            let sid = conn.last_insert_rowid();
            let ts = chrono::Utc::now().to_rfc3339();
            crate::db::insert_tool_invocation(
                &conn,
                sid,
                None,
                "mcp__claude_ai_Notion__notion-search",
                "mcp__claude_ai_Notion__",
                &ts,
            )
            .unwrap();

            let profile = Profile {
                display: "Design".into(),
                description: "test".into(),
                keep_tools: vec!["mcp__claude_ai_Figma__use_figma".into()],
                ..Default::default()
            };

            let tools = detect_access_friction_tools(
                "I don't have access to Notion for this task.",
                &profile,
            );
            assert!(
                tools.iter().any(|t| t.contains("Notion")),
                "expected Notion tool, got {tools:?}"
            );
            let _ = session_id;
        });
    }

    #[test]
    fn pruned_server_is_recovered_from_denial_text() {
        with_ctx_home(|_tmp| {
            let mut cfg = Config::load();
            cfg.filter_mode = FilterMode::Soft;
            cfg.pruned_servers = vec!["mcp__claude_ai_Canva__".into()];
            cfg.save().unwrap();

            // Profile "all" keeps everything (filtering disabled), yet a reach for the pruned Canva
            // server must still surface so the session re-adds it.
            let profile = Profile::default();
            let tools = detect_access_friction_tools(
                "Sorry, I don't have access to Canva right now.",
                &profile,
            );
            assert!(
                tools.iter().any(|t| t.contains("Canva")),
                "pruned Canva should be recoverable on a reach, got {tools:?}"
            );
        });
    }

    #[test]
    fn extract_assistant_text_from_transcript() {
        let payload = serde_json::json!({
            "transcript": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [{"type": "text", "text": "I don't have access to Figma."}]}
            ]
        });
        let text = extract_assistant_text_from_hook_payload(&payload);
        assert!(text.contains("don't have access"));
    }

    #[test]
    fn add_session_expansions_dedupes() {
        with_ctx_home(|_tmp| {
            let mut cfg = Config::load();
            cfg.filter_mode = FilterMode::Soft;
            cfg.active_profile = Some("all".into());
            cfg.save().unwrap();

            let first = add_session_expansions([(
                "mcp__claude_ai_Figma__use_figma".to_string(),
                ExpansionReason::Keyword,
            )])
            .unwrap();
            assert_eq!(first.len(), 1);

            let second = add_session_expansions([(
                "mcp__claude_ai_Figma__use_figma".to_string(),
                ExpansionReason::AccessFriction,
            )])
            .unwrap();
            assert!(second.is_empty());
        });
    }
}
