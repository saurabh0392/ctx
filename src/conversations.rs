use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use crate::user_profile::UserProfile;

// ---------------------------------------------------------------------------
// Pricing
// ---------------------------------------------------------------------------

struct ModelPricing {
    input: f64,
    cache_read: f64,
    cache_creation: f64,
    output: f64,
}

fn pricing_for(model: &str) -> ModelPricing {
    if model.contains("opus") {
        ModelPricing { input: 15.0, cache_read: 1.5, cache_creation: 18.75, output: 75.0 }
    } else if model.contains("haiku") {
        ModelPricing { input: 0.80, cache_read: 0.08, cache_creation: 1.00, output: 4.0 }
    } else {
        ModelPricing { input: 3.0, cache_read: 0.30, cache_creation: 3.75, output: 15.0 }
    }
}

fn compute_cost(
    input: usize,
    cache_read: usize,
    cache_creation: usize,
    output: usize,
    model: &str,
) -> f64 {
    let p = pricing_for(model);
    (input as f64 * p.input
        + cache_read as f64 * p.cache_read
        + cache_creation as f64 * p.cache_creation
        + output as f64 * p.output)
        / 1_000_000.0
}

// ---------------------------------------------------------------------------
// Public output structs
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
pub struct TurnDetail {
    pub turn_index: usize,
    pub human_text: String,
    #[serde(default)]
    pub user_ts: Option<String>,
    pub input_tokens: usize,
    pub output_tokens: usize,
    #[serde(default)]
    pub cache_read_tokens: usize,
    #[serde(default)]
    pub cache_creation_tokens: usize,
    #[serde(default)]
    pub model: String,
    pub cost_usd: f64,
    pub flags: Vec<String>,
    pub tip: String,
}

#[derive(Serialize, Clone)]
pub struct SessionCost {
    pub session_id: String,
    pub project: String,
    pub started_at: String,
    #[serde(default)]
    pub first_user_message: String,
    pub turn_count: usize,
    pub total_usd: f64,
    pub input_tokens: usize,
    pub cache_creation_tokens: usize,
    pub cache_read_tokens: usize,
    pub output_tokens: usize,
    pub models_used: Vec<String>,
    pub hit_compact: bool,
    pub clarifying_turns: usize,
    pub correction_turns: usize,
    pub top_turns: Vec<TurnDetail>,
}

/// Full parse result including every turn row (for SQLite ingest).
pub struct ParsedSession {
    pub session: SessionCost,
    pub turns: Vec<TurnDetail>,
}

#[derive(Serialize)]
pub struct MonthlySpend {
    pub month: String,
    pub total_usd: f64,
    pub input_tokens: usize,
    pub cache_creation_tokens: usize,
    pub cache_read_tokens: usize,
    pub output_tokens: usize,
    pub sessions: usize,
    pub ctx_saved_usd: f64,
    /// User-entered actual spend from Anthropic billing. None = not yet entered.
    pub actual_spend_usd: Option<f64>,
    /// User-configured monthly budget cap.
    pub budget_usd: Option<f64>,
    /// Session total snapshotted when actual_spend_usd was last set.
    /// live_spend = actual_spend_usd + max(0, total_usd - actual_spend_baseline_usd)
    pub actual_spend_baseline_usd: Option<f64>,
}

#[derive(Serialize)]
pub struct AdvisorTip {
    pub title: String,
    pub detail: String,
    pub value: f64,
    pub kind: String,
}

/// Dashboard "Repeat patterns" card (rule-based, no LLM).
#[derive(Serialize)]
pub struct PatternAlert {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub value_usd: f64,
}

// ---------------------------------------------------------------------------
// Internal JSONL row
// ---------------------------------------------------------------------------

struct AssistantRow {
    request_id: Option<String>,
    _timestamp: Option<String>,
    model: String,
    input_tokens: usize,
    cache_read: usize,
    cache_creation: usize,
    output_tokens: usize,
    output_text: Option<String>,
    is_text_content: bool,
}

struct UserRow {
    timestamp: Option<String>,
    is_meta: bool,
    human_text: Option<String>,
}

enum Row {
    User(UserRow),
    Assistant(AssistantRow),
    Compact,
    Other,
}

fn extract_human_text(msg: &Value) -> Option<String> {
    let content = msg.get("content")?;
    if let Some(arr) = content.as_array() {
        for item in arr {
            if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(txt) = item.get("text").and_then(|v| v.as_str()) {
                    return Some(txt.chars().take(1000).collect());
                }
            }
        }
    }
    if let Some(s) = content.as_str() {
        return Some(s.chars().take(1000).collect());
    }
    None
}

fn extract_output_text(content: &Value) -> (Option<String>, bool) {
    if let Some(arr) = content.as_array() {
        for item in arr {
            let ctype = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if ctype == "text" {
                let txt = item.get("text").and_then(|v| v.as_str())
                    .map(|s| s.chars().take(600).collect());
                return (txt, true);
            }
        }
    }
    (None, false)
}

fn usage_block_from_value(v: &Value) -> Option<&Value> {
    v.get("usage")
        .or_else(|| v.get("message").and_then(|m| m.get("usage")))
}

fn parse_row_with_context(line: &str, last_model: &mut Option<String>) -> Row {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return Row::Other };
    let type_ = v.get("type").and_then(|x| x.as_str()).unwrap_or("");

    if type_ == "system" {
        if v.get("compactMetadata").is_some() {
            return Row::Compact;
        }
        let subtype = v.get("subtype").and_then(|x| x.as_str()).unwrap_or("");
        if subtype == "init" {
            if let Some(m) = v.get("model").and_then(|x| x.as_str()) {
                if !m.is_empty() {
                    *last_model = Some(m.to_string());
                }
            } else if let Some(m) = v
                .get("message")
                .and_then(|msg| msg.get("model"))
                .and_then(|x| x.as_str())
            {
                if !m.is_empty() {
                    *last_model = Some(m.to_string());
                }
            }
        }
        return Row::Other;
    }

    if type_ == "user" {
        let is_meta = v.get("isMeta").and_then(|x| x.as_bool()).unwrap_or(false);
        let timestamp = v.get("timestamp").and_then(|x| x.as_str()).map(|s| s.to_string());
        let human_text = v.get("message").and_then(|m| extract_human_text(m));
        return Row::User(UserRow { timestamp, is_meta, human_text });
    }

    if type_ == "result" {
        let subtype = v.get("subtype").and_then(|x| x.as_str()).unwrap_or("");
        if subtype != "success" && subtype != "error" {
            return Row::Other;
        }
        let Some(u) = usage_block_from_value(&v) else {
            return Row::Other;
        };
        let input_tokens = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let cache_read = u
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let cache_creation = u
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let output_tokens = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && cache_creation == 0 {
            return Row::Other;
        }
        let request_id = v
            .get("uuid")
            .or_else(|| v.get("id"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let timestamp = v.get("timestamp").and_then(|x| x.as_str()).map(|s| s.to_string());
        let model = last_model
            .clone()
            .unwrap_or_else(|| "claude-sonnet".to_string());
        return Row::Assistant(AssistantRow {
            request_id,
            _timestamp: timestamp,
            model,
            input_tokens,
            cache_read,
            cache_creation,
            output_tokens,
            output_text: None,
            is_text_content: false,
        });
    }

    if type_ == "assistant" {
        let request_id = v.get("requestId").and_then(|x| x.as_str()).map(|s| s.to_string());
        let timestamp = v.get("timestamp").and_then(|x| x.as_str()).map(|s| s.to_string());
        if let Some(msg) = v.get("message") {
            let model_raw = msg.get("model").and_then(|x| x.as_str()).unwrap_or("");
            let u = msg.get("usage");
            let input_tokens = u.and_then(|u| u.get("input_tokens")).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let cache_read = u
                .and_then(|u| u.get("cache_read_input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let cache_creation = u
                .and_then(|u| u.get("cache_creation_input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let output_tokens = u.and_then(|u| u.get("output_tokens")).and_then(|v| v.as_u64()).unwrap_or(0) as usize;

            let model = if model_raw.is_empty() || model_raw == "<synthetic>" {
                last_model.clone().unwrap_or_else(|| "claude-sonnet".to_string())
            } else {
                model_raw.to_string()
            };

            if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && cache_creation == 0 {
                return Row::Other;
            }

            let content = msg.get("content").unwrap_or(&Value::Null);
            let (output_text, is_text_content) = extract_output_text(content);

            return Row::Assistant(AssistantRow {
                request_id,
                _timestamp: timestamp,
                model,
                input_tokens,
                cache_read,
                cache_creation,
                output_tokens,
                output_text,
                is_text_content,
            });
        }
        return Row::Other;
    }

    Row::Other
}

// ---------------------------------------------------------------------------
// Flag detection
// ---------------------------------------------------------------------------

fn detect_flags(
    human_text: &str,
    input_tokens: usize,
    prev_output_tokens: usize,
    prev_output_text: &Option<String>,
    model: &str,
    profile: &UserProfile,
    is_pre_compact: bool,
) -> Vec<String> {
    let mut flags = Vec::new();
    // Correction signal, scored through the shared lexical guard so go-aheads and menu
    // picks ("lets do 1") are not mislabeled as harm (SAU-148 audit). Two confidence tiers:
    //   - Explicit: the turn carried complaint language ("wrong", "revert", "undo"). High
    //     confidence, so it flags even after a short assistant reply.
    //   - Terse: a short non-complaint follow-up after substantial work. Low confidence,
    //     kept for the fail-safe gate but tagged so training can down-weight it.
    let substantial_prior = prev_output_tokens > 150;
    match crate::outcome_signals::classify_correction(human_text, profile.correction_threshold) {
        crate::outcome_signals::CorrectionClass::Explicit => {
            flags.push("correction".to_string());
            flags.push("correction_explicit".to_string());
        }
        crate::outcome_signals::CorrectionClass::Terse if substantial_prior => {
            flags.push("correction".to_string());
        }
        _ => {}
    }

    if prev_output_tokens > 0 && prev_output_tokens < 400 {
        if let Some(prev) = prev_output_text {
            if prev.contains('?') {
                flags.push("clarification".to_string());
            }
        }
    }

    if human_text.len() > 800 || input_tokens > 4000 {
        flags.push("long_dump".to_string());
    }

    if is_pre_compact {
        flags.push("pre_compact".to_string());
    }

    if model.contains("opus") {
        flags.push("opus".to_string());
    }

    flags
}

fn build_tip(flags: &[String], cost_usd: f64, input_tokens: usize, output_tokens: usize) -> String {
    for flag in flags {
        return match flag.as_str() {
            "correction" => format!(
                "You corrected Claude here (+${:.2} on this exchange). Adding output format and constraints upfront typically cuts correction rounds in half.",
                cost_usd
            ),
            "clarification" => format!(
                "Claude asked for more detail before responding ({output_tokens} output tokens, short response). A 'Goal / Context / Output format' structure eliminates most clarifying rounds."
            ),
            "long_dump" => format!(
                "This prompt was {input_tokens} input tokens, a large context load. Breaking multi-part asks into sequential focused messages tends to reduce total session cost by 30%."
            ),
            "pre_compact" => "The context window ran out shortly after this point. Sessions that stay focused on one task avoid mid-session resets.".to_string(),
            "opus" => format!(
                "This turn ran on Opus (${cost_usd:.3}). Sonnet handles most tasks at 1/5 the cost, same quality for standard engineering and writing tasks."
            ),
            _ => continue,
        };
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Session parser
// ---------------------------------------------------------------------------

fn model_short(model: &str) -> &'static str {
    if model.contains("opus") { "opus" }
    else if model.contains("haiku") { "haiku" }
    else { "sonnet" }
}

fn session_id(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in s.as_bytes() {
        let idx = ((crc ^ b as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[idx];
    }
    format!("{:08x}", crc ^ 0xFFFF_FFFF)
}

fn parse_session(path: &Path, project: &str, profile: &UserProfile) -> Option<ParsedSession> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut last_model: Option<String> = None;
    let raw_rows: Vec<Row> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| parse_row_with_context(l, &mut last_model))
        .collect();

    // Find compact event row indices
    let compact_indices: Vec<usize> = raw_rows.iter().enumerate()
        .filter_map(|(i, r)| if matches!(r, Row::Compact) { Some(i) } else { None })
        .collect();

    // For assistant deduplication: requestId -> last row index with text content (prefer text over thinking)
    // Build: requestId -> Vec<(row_idx, is_text_content)>, then pick last text or last thinking
    let mut req_to_idx: HashMap<String, usize> = HashMap::new();
    let mut req_text_idx: HashMap<String, usize> = HashMap::new();
    for (i, row) in raw_rows.iter().enumerate() {
        if let Row::Assistant(a) = row {
            let key = a.request_id.clone().unwrap_or_else(|| format!("__{i}"));
            req_to_idx.insert(key.clone(), i);
            if a.is_text_content {
                req_text_idx.insert(key, i);
            }
        }
    }
    // Canonical indices: prefer text-content row; fall back to last row
    let canonical: std::collections::HashSet<usize> = req_to_idx.keys()
        .map(|k| *req_text_idx.get(k).unwrap_or_else(|| req_to_idx.get(k).unwrap()))
        .collect();

    let mut turns: Vec<TurnDetail> = Vec::new();
    let mut started_at: Option<String> = None;
    let mut first_user_message = String::new();
    let mut total_input = 0usize;
    let mut total_cache_creation = 0usize;
    let mut total_cache_read = 0usize;
    let mut total_output = 0usize;
    let mut total_cost = 0.0f64;
    let mut models: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    let mut clarifying = 0usize;
    let mut corrections = 0usize;
    let hit_compact = !compact_indices.is_empty();

    let mut prev_output_tokens: usize = 0;
    let mut prev_output_text: Option<String> = None;
    let mut pending_human: Option<(usize, String, Option<String>)> = None;

    for (i, row) in raw_rows.iter().enumerate() {
        match row {
            Row::User(u) if !u.is_meta => {
                if let Some(text) = &u.human_text {
                    if started_at.is_none() {
                        started_at = u.timestamp.clone();
                    }
                    if crate::outcome_signals::is_user_interrupt(text) {
                        // The user hit ESC to stop the agent. Emit this as its own turn now:
                        // the standard path waits for a following assistant reply, but the
                        // next real prompt overwrites this row, so the signal would be lost.
                        // Flagged "aborted" (a distinct high-precision signal type) plus
                        // "correction" so the existing windowed outcome join attributes it.
                        corrections += 1;
                        turns.push(TurnDetail {
                            turn_index: turns.len(),
                            human_text: text.clone(),
                            user_ts: u.timestamp.clone(),
                            input_tokens: 0,
                            output_tokens: 0,
                            cache_read_tokens: 0,
                            cache_creation_tokens: 0,
                            model: String::new(),
                            cost_usd: 0.0,
                            flags: vec!["aborted".to_string(), "correction".to_string()],
                            tip: String::new(),
                        });
                    } else {
                        pending_human = Some((i, text.clone(), u.timestamp.clone()));
                    }
                }
            }
            Row::Assistant(a) if canonical.contains(&i) => {
                let cost = compute_cost(a.input_tokens, a.cache_read, a.cache_creation, a.output_tokens, &a.model);
                total_input += a.input_tokens;
                total_cache_creation += a.cache_creation;
                total_cache_read += a.cache_read;
                total_output += a.output_tokens;
                total_cost += cost;
                models.insert(model_short(&a.model));

                if let Some((human_row_idx, human_text, user_ts)) = pending_human.take() {
                    if first_user_message.is_empty() {
                        first_user_message = human_text.chars().take(2000).collect();
                    }
                    let is_pre_compact = compact_indices.iter().any(|&ci| {
                        ci > human_row_idx && ci <= human_row_idx + 10
                    });

                    let flags = detect_flags(
                        &human_text,
                        a.input_tokens,
                        prev_output_tokens,
                        &prev_output_text,
                        &a.model,
                        profile,
                        is_pre_compact,
                    );
                    let tip = build_tip(&flags, cost, a.input_tokens, a.output_tokens);

                    if flags.contains(&"correction".to_string()) { corrections += 1; }
                    if flags.contains(&"clarification".to_string()) { clarifying += 1; }

                    turns.push(TurnDetail {
                        turn_index: turns.len(),
                        human_text,
                        user_ts,
                        input_tokens: a.input_tokens,
                        output_tokens: a.output_tokens,
                        cache_read_tokens: a.cache_read,
                        cache_creation_tokens: a.cache_creation,
                        model: a.model.clone(),
                        cost_usd: cost,
                        flags,
                        tip,
                    });
                }

                prev_output_tokens = a.output_tokens;
                prev_output_text = a.output_text.clone();

                if a.is_text_content {
                    if let Some(ref text) = a.output_text {
                        let slug = crate::config::Config::load()
                            .active_profile
                            .unwrap_or_else(|| "all".into());
                        if let Ok(p) = crate::profiles::get(&slug) {
                            let tools = crate::semantic_tools::detect_access_friction_tools(text, &p);
                            let _ = crate::semantic_tools::record_access_friction(&tools);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if turns.is_empty() { return None; }

    let mut top_turns = turns.clone();
    top_turns.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap_or(std::cmp::Ordering::Equal));
    top_turns.truncate(3);
    top_turns.sort_by_key(|t| t.turn_index);

    let mut model_vec: Vec<String> = models.iter().map(|s| s.to_string()).collect();
    model_vec.sort();

    Some(ParsedSession {
        session: SessionCost {
            session_id: session_id(path),
            project: project.to_string(),
            started_at: started_at.unwrap_or_default(),
            first_user_message,
            turn_count: turns.len(),
            total_usd: total_cost,
            input_tokens: total_input,
            cache_creation_tokens: total_cache_creation,
            cache_read_tokens: total_cache_read,
            output_tokens: total_output,
            models_used: model_vec,
            hit_compact,
            clarifying_turns: clarifying,
            correction_turns: corrections,
            top_turns,
        },
        turns,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

fn try_load_sessions_from_db(conn: &Connection) -> rusqlite::Result<Vec<SessionCost>> {
    let _ = crate::db::ensure_schema(conn);
    let mut stmt = conn.prepare(
        r#"SELECT external_key, project, started_at, first_user_message, turn_count, total_usd,
                  input_tokens, cache_creation_tokens, cache_read_tokens, output_tokens,
                  models_used, hit_compact, clarifying_turns, correction_turns, top_turns_json
           FROM sessions
           ORDER BY datetime(started_at) DESC
           LIMIT 8000"#,
    )?;
    let rows = stmt.query_map([], |r| {
        let models_s: String = r.get(10)?;
        let top_s: String = r.get(14)?;
        let models_used: Vec<String> = serde_json::from_str(&models_s).unwrap_or_default();
        let top_turns: Vec<TurnDetail> = serde_json::from_str(&top_s).unwrap_or_default();
        Ok(SessionCost {
            session_id: r.get(0)?,
            project: r.get(1)?,
            started_at: r.get(2)?,
            first_user_message: r.get::<_, String>(3).unwrap_or_default(),
            turn_count: r.get::<_, i64>(4)? as usize,
            total_usd: r.get(5)?,
            input_tokens: r.get::<_, i64>(6)? as usize,
            cache_creation_tokens: r.get::<_, i64>(7)? as usize,
            cache_read_tokens: r.get::<_, i64>(8)? as usize,
            output_tokens: r.get::<_, i64>(9)? as usize,
            models_used,
            hit_compact: r.get::<_, i64>(11)? != 0,
            clarifying_turns: r.get::<_, i64>(12)? as usize,
            correction_turns: r.get::<_, i64>(13)? as usize,
            top_turns,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn all_sessions() -> Vec<SessionCost> {
    if crate::db::db_exists() {
        if let Ok(conn) = crate::db::open_db() {
            if crate::db::ensure_schema(&conn).is_ok() {
                if let Ok(v) = try_load_sessions_from_db(&conn) {
                    if !v.is_empty() {
                        return v;
                    }
                }
            }
        }
    }
    all_sessions_from_filesystem()
}

fn all_sessions_from_filesystem() -> Vec<SessionCost> {
    let profile = UserProfile::compute();
    let home = dirs::home_dir().unwrap_or_default();
    let projects_dir = home.join(".claude").join("projects");
    let mut sessions: Vec<SessionCost> = Vec::new();

    let Ok(proj_entries) = std::fs::read_dir(&projects_dir) else { return sessions };
    for proj_entry in proj_entries.flatten() {
        let proj_path = proj_entry.path();
        if !proj_path.is_dir() { continue; }
        let dir_name = proj_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if dir_name == "subagents" { continue; }

        let proj_display = {
            let parts: Vec<&str> = dir_name.split('-').filter(|s| !s.is_empty()).collect();
            if let Some(doc_idx) = parts.iter().position(|&s| s == "Documents") {
                parts[doc_idx + 1..].join(" ")
            } else if parts.len() > 2 {
                parts[2..].join(" ")
            } else {
                dir_name.trim_start_matches('-').replace('-', " ")
            }
        };

        let Ok(file_entries) = std::fs::read_dir(&proj_path) else { continue };
        for file_entry in file_entries.flatten() {
            let fpath = file_entry.path();
            if !fpath.is_file() { continue; }
            let fname = fpath
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if !fname.ends_with(".jsonl") { continue; }
            if fname.contains("compact") { continue; }

            let mut session = match parse_session(&fpath, &proj_display, &profile) {
                Some(p) => p.session,
                None => continue,
            };

            let uuid = fname.trim_end_matches(".jsonl");
            let subagents_dir = proj_path.join(uuid).join("subagents");
            if subagents_dir.is_dir() {
                if let Ok(sub_entries) = std::fs::read_dir(&subagents_dir) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                            continue;
                        }
                        if let Some(sub) = parse_session(&sub_path, &proj_display, &profile) {
                            session.total_usd += sub.session.total_usd;
                            session.input_tokens += sub.session.input_tokens;
                            session.cache_creation_tokens += sub.session.cache_creation_tokens;
                            session.cache_read_tokens += sub.session.cache_read_tokens;
                            session.output_tokens += sub.session.output_tokens;
                            session.turn_count += sub.session.turn_count;
                            for m in sub.session.models_used {
                                if !session.models_used.contains(&m) {
                                    session.models_used.push(m);
                                }
                            }
                            if sub.session.hit_compact {
                                session.hit_compact = true;
                            }
                            session.clarifying_turns += sub.session.clarifying_turns;
                            session.correction_turns += sub.session.correction_turns;
                        }
                    }
                }
            }

            sessions.push(session);
        }
    }

    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    sessions
}

fn desktop_dir_component_excluded(name: &std::ffi::OsStr) -> bool {
    let s = name.to_string_lossy();
    matches!(s.as_ref(), ".claude" | "skills-plugin" | "rpm")
}

fn collect_desktop_audit_jsonl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        if desktop_dir_component_excluded(&ent.file_name()) {
            continue;
        }
        let p = ent.path();
        if p.is_dir() {
            collect_desktop_audit_jsonl_files(&p, out);
        } else if p.file_name().and_then(|n| n.to_str()) == Some("audit.jsonl") {
            out.push(p);
        }
    }
}

/// Canonical `audit.jsonl` paths under Claude Desktop local-agent session roots (content-probed).
pub fn discover_desktop_sessions() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in crate::config::claude_desktop_session_roots() {
        if root.is_dir() {
            collect_desktop_audit_jsonl_files(&root, &mut out);
        }
    }
    out.sort();
    out.dedup();
    out.retain(|p| desktop_audit_jsonl_probe_valid(p));
    out
}

fn desktop_audit_jsonl_probe_valid(path: &Path) -> bool {
    let Ok(s) = std::fs::read_to_string(path) else {
        return false;
    };
    let mut picked: Vec<&str> = Vec::new();
    for line in s.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        picked.push(t);
        if picked.len() >= 3 {
            break;
        }
    }
    let mut got_parse = false;
    let mut got_type = false;
    let mut got_sid = false;
    for t in picked {
        let Ok(val) = serde_json::from_str::<Value>(t) else {
            continue;
        };
        got_parse = true;
        if let Some(ty) = val.get("type").and_then(|x| x.as_str()) {
            if matches!(ty, "user" | "assistant" | "system" | "result") {
                got_type = true;
            }
        }
        if val.get("session_id").and_then(|x| x.as_str()).is_some() {
            got_sid = true;
        }
    }
    got_parse && got_type && got_sid
}

fn read_desktop_init_cwd(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    for line in reader.lines().take(2000) {
        let line = line.ok()?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(t).ok()?;
        if v.get("type").and_then(|x| x.as_str()) == Some("system")
            && v.get("subtype").and_then(|x| x.as_str()) == Some("init")
        {
            if let Some(c) = v.get("cwd").and_then(|x| x.as_str()) {
                return Some(c.to_string());
            }
            if let Some(c) = v.get("message").and_then(|m| m.get("cwd")).and_then(|x| x.as_str()) {
                return Some(c.to_string());
            }
            return None;
        }
    }
    None
}

fn desktop_project_label_from_cwd(cwd: &str) -> String {
    let lower = cwd.to_lowercase();
    if lower.contains("/sessions/") || lower.contains("\\sessions\\") {
        return "Claude Desktop".to_string();
    }
    let norm = cwd.replace('\\', "/");
    let parts: Vec<&str> = norm.split('/').filter(|s| !s.is_empty()).collect();
    if let Some(doc_idx) = parts.iter().position(|&s| s.eq_ignore_ascii_case("documents")) {
        if doc_idx + 1 < parts.len() {
            return parts[doc_idx + 1..].join(" ");
        }
    }
    parts.last().map(|s| (*s).to_string()).unwrap_or_else(|| "Claude Desktop".to_string())
}

fn ingest_one_jsonl_session(
    tx: &rusqlite::Transaction<'_>,
    fpath: &Path,
    project_display: &str,
    profile: &UserProfile,
    store_prompt_text: bool,
) -> anyhow::Result<bool> {
    let Some(parsed) = parse_session(fpath, project_display, profile) else {
        return Ok(false);
    };
    let external_key = fpath.to_string_lossy().to_string();
    let models_json = serde_json::to_string(&parsed.session.models_used)?;
    let tool_uses = tool_uses_from_jsonl_file(fpath);
    // Collect distinct server display names for embedding (deduplicated, sorted for stability)
    let mut seen_servers: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (_, server_prefix, _) in &tool_uses {
        // Convert prefix back to a short display form: strip mcp__claude_ai_ prefix and trailing __
        let display = server_prefix
            .strip_prefix("mcp__claude_ai_")
            .unwrap_or(server_prefix)
            .trim_end_matches("__")
            .replace('_', " ");
        if !display.is_empty() {
            seen_servers.insert(display);
        }
    }
    let (first_msg, embed_text, top_json) = if store_prompt_text {
        let top_json = serde_json::to_string(&parsed.session.top_turns)?;
        let invoked_servers: Vec<String> = seen_servers.iter().cloned().collect();
        let embed_text = crate::embedder::compose_embed_text(
            &parsed.session.first_user_message,
            &parsed.session.project,
            "",
            &invoked_servers,
        );
        (
            parsed.session.first_user_message.clone(),
            embed_text,
            top_json,
        )
    } else {
        (String::new(), String::new(), "[]".to_string())
    };
    let sid = crate::db::upsert_claude_session(
        &*tx,
        &external_key,
        &parsed.session.project,
        &parsed.session.started_at,
        Some(parsed.session.started_at.as_str()),
        parsed.session.turn_count as i64,
        parsed.session.total_usd,
        parsed.session.input_tokens as i64,
        parsed.session.cache_creation_tokens as i64,
        parsed.session.cache_read_tokens as i64,
        parsed.session.output_tokens as i64,
        &models_json,
        if parsed.session.hit_compact { 1 } else { 0 },
        parsed.session.clarifying_turns as i64,
        parsed.session.correction_turns as i64,
        &first_msg,
        &embed_text,
        &parsed.session.project,
        &top_json,
    )?;
    crate::db::replace_session_turns(&*tx, sid)?;
    for t in &parsed.turns {
        let flags_json = serde_json::to_string(&t.flags)?;
        let prefix = if store_prompt_text {
            t.human_text.chars().take(500).collect::<String>()
        } else {
            String::new()
        };
        let turn_ts = t
            .user_ts
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(Some(parsed.session.started_at.as_str()));
        let _tid = crate::db::insert_turn(
            &*tx,
            sid,
            t.turn_index as i64,
            "turn",
            t.cost_usd,
            t.input_tokens as i64,
            t.output_tokens as i64,
            t.cache_read_tokens as i64,
            t.cache_creation_tokens as i64,
            &t.model,
            &flags_json,
            &prefix,
            turn_ts,
        )?;
    }
    for (tool_name, server_prefix, ts) in &tool_uses {
        crate::db::insert_tool_invocation(&*tx, sid, None, tool_name, server_prefix, ts)?;
    }
    Ok(true)
}

/// Returns true when `path`'s mtime is at or before `cutoff`, meaning the file has not been
/// modified since the last ingest and can be skipped. When `cutoff` is None (first-ever ingest)
/// or when the mtime cannot be read, returns false (process the file).
fn file_unchanged_since(path: &std::path::Path, cutoff: Option<std::time::SystemTime>) -> bool {
    let Some(cutoff) = cutoff else { return false };
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|mtime| mtime <= cutoff)
        .unwrap_or(false)
}

/// Walk `~/.claude/projects/**/*.jsonl`, then Claude Desktop `audit.jsonl` logs, and upsert into SQLite.
/// Returns number of main session files written (Claude Code project JSONL plus Desktop audits).
///
/// Skips files whose filesystem mtime is at or before the previous `last_ingest_at` timestamp
/// stored in the meta table. This makes per-turn ingest calls cheap: on an active session only
/// the current session file is newer than the last run, so the full scan stays O(1) in practice.
pub fn ingest_claude_jsonl() -> anyhow::Result<usize> {
    let conn = crate::db::open_db()?;
    crate::db::ensure_schema(&conn)?;
    let cfg = crate::config::Config::load();
    let store_prompt_text = cfg.store_prompt_text_enabled();
    let embeddings_on = cfg.embeddings_enabled();
    let profile = UserProfile::compute();

    // Read the previous ingest timestamp once and convert to SystemTime for mtime comparisons.
    let last_ingest_cutoff: Option<std::time::SystemTime> = conn
        .query_row("SELECT v FROM meta WHERE k = 'last_ingest_at'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| {
            std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(dt.timestamp().max(0) as u64)
        });

    let tx = conn.unchecked_transaction()?;
    let mut count = 0usize;
    let home = dirs::home_dir().unwrap_or_default();
    let projects_dir = home.join(".claude").join("projects");

    if projects_dir.is_dir() {
        if let Ok(proj_entries) = std::fs::read_dir(&projects_dir) {
            for proj_entry in proj_entries.flatten() {
                let proj_path = proj_entry.path();
                if !proj_path.is_dir() {
                    continue;
                }
                let dir_name = proj_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if dir_name == "subagents" {
                    continue;
                }

                let proj_display = {
                    let parts: Vec<&str> = dir_name.split('-').filter(|s| !s.is_empty()).collect();
                    if let Some(doc_idx) = parts.iter().position(|&s| s == "Documents") {
                        parts[doc_idx + 1..].join(" ")
                    } else if parts.len() > 2 {
                        parts[2..].join(" ")
                    } else {
                        dir_name.trim_start_matches('-').replace('-', " ")
                    }
                };

                let Ok(file_entries) = std::fs::read_dir(&proj_path) else {
                    continue;
                };
                for file_entry in file_entries.flatten() {
                    let fpath = file_entry.path();
                    if !fpath.is_file() {
                        continue;
                    }
                    let fname = fpath
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !fname.ends_with(".jsonl") {
                        continue;
                    }
                    if fname.contains("compact") {
                        continue;
                    }
                    if file_unchanged_since(&fpath, last_ingest_cutoff) {
                        continue;
                    }

                    if ingest_one_jsonl_session(&tx, &fpath, &proj_display, &profile, store_prompt_text)? {
                        count += 1;
                    }
                }
            }
        }
    }

    for audit_path in discover_desktop_sessions() {
        if file_unchanged_since(&audit_path, last_ingest_cutoff) {
            continue;
        }
        let cwd = read_desktop_init_cwd(&audit_path);
        let project = cwd
            .as_deref()
            .map(desktop_project_label_from_cwd)
            .unwrap_or_else(|| "Claude Desktop".to_string());
        if ingest_one_jsonl_session(&tx, &audit_path, &project, &profile, store_prompt_text)? {
            count += 1;
        }
    }

    tx.commit()?;

    let _ = crate::db::enrich_hook_traces(&conn);
    // Act 0: back-fill outcome labels (correction / re-read) onto shadow decision rows
    // now that downstream turns and tool calls for those sessions have landed.
    let _ = crate::db::join_compress_outcomes(&conn);
    // Act 3 (cross-surface): join outcomes for agents whose transcripts carry no
    // timestamps (Cursor) using the ordinal/fingerprint timeline. Disjoint from the
    // Claude join above; runs before training so fresh labels are included.
    let _ = crate::surface::ingest::join_transcript_outcomes(&conn, &home);
    // Act 1: re-train the local outcome model on the freshly labeled data (online
    // improvement). No-op until enough labels accrue; never fails ingest.
    let _ = crate::learn::train();
    let _ = crate::tuning::run_tuning_after_ingest(&conn);
    let _ = crate::experiment_plan::ensure_pending_phase_applied();
    let _ = crate::claude_settings::sync_experiment_hooks_from_config();

    if crate::config::Config::load().adaptive_prefix_enabled {
        let _ = crate::adaptive::refresh_adaptive_prefix();
    }

    let ts = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (k, v) VALUES ('last_ingest_at', ?1)",
        rusqlite::params![ts],
    );

    if embeddings_on {
        let current_backend = if crate::embedder::onnx_available() { "onnx" } else { "hash" };
        let prev_backend: String = conn
            .query_row("SELECT v FROM meta WHERE k = 'embed_backend'", [], |r| r.get(0))
            .unwrap_or_else(|_| "hash".to_string());

        if current_backend == "onnx" && prev_backend != "onnx" {
            let n = crate::embedder::reembed_all_sessions(&conn).unwrap_or(0);
            if n > 0 {
                eprintln!("Re-embedded {n} sessions with ONNX MiniLM (was: {prev_backend})");
            }
        } else {
            let _ = crate::embedder::embed_sessions_incremental(&conn);
        }

        let _ = conn.execute(
            "INSERT OR REPLACE INTO meta (k, v) VALUES ('embed_backend', ?1)",
            rusqlite::params![current_backend],
        );
    } else {
        let _ = conn.execute("DELETE FROM session_embeddings", []);
    }

    let _ = crate::profiles::after_ingest_profile_sync();

    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::maybe_reset_stale_install_watermark(&conn);
    }

    Ok(count)
}

fn tool_uses_from_jsonl_file(path: &Path) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return out;
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("type").and_then(|x| x.as_str()) != Some("assistant") {
            continue;
        }
        let ts = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let Some(msg) = v.get("message") else { continue };
        let Some(content) = msg.get("content") else { continue };
        let Some(arr) = content.as_array() else { continue };
        for item in arr {
            if item.get("type").and_then(|x| x.as_str()) != Some("tool_use") {
                continue;
            }
            let Some(name) = item.get("name").and_then(|x| x.as_str()) else { continue };
            if let Some(prefix) = crate::filter::server_prefix_from_tool(name) {
                out.push((name.to_string(), prefix, ts.clone()));
            }
        }
    }
    out
}

fn month_of(ts: &str) -> Option<String> {
    let dt: DateTime<Utc> = ts.parse().ok()?;
    Some(format!("{}-{:02}", dt.year(), dt.month()))
}

pub fn monthly_spend(sessions: &[SessionCost]) -> Vec<MonthlySpend> {
    let analytics = crate::analytics::load_records();
    let mut ctx_by_month: HashMap<String, f64> = HashMap::new();
    for rec in &analytics {
        if let Ok(dt) = rec.ts.parse::<DateTime<Utc>>() {
            let m = format!("{}-{:02}", dt.year(), dt.month());
            let saved = (rec.tokens_saved as f64 / 1_000_000.0) * 0.30;
            *ctx_by_month.entry(m).or_default() += saved;
        }
    }

    let mut by_month: HashMap<String, MonthlySpend> = HashMap::new();
    for s in sessions {
        let Some(month) = month_of(&s.started_at) else { continue };
        let ctx_saved = ctx_by_month.get(&month).copied().unwrap_or(0.0);
        let e = by_month.entry(month.clone()).or_insert(MonthlySpend {
            month: month.clone(),
            total_usd: 0.0,
            input_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            output_tokens: 0,
            sessions: 0,
            ctx_saved_usd: ctx_saved,
            actual_spend_usd: None,
            budget_usd: None,
            actual_spend_baseline_usd: None,
        });
        e.total_usd += s.total_usd;
        e.input_tokens += s.input_tokens;
        e.cache_creation_tokens += s.cache_creation_tokens;
        e.cache_read_tokens += s.cache_read_tokens;
        e.output_tokens += s.output_tokens;
        e.sessions += 1;
    }

    let mut result: Vec<MonthlySpend> = by_month.into_values().collect();
    result.sort_by(|a, b| b.month.cmp(&a.month));
    result.truncate(3);
    result
}

pub fn generate_tips(sessions: &[SessionCost]) -> Vec<AdvisorTip> {
    let mut tips: Vec<AdvisorTip> = Vec::new();
    if sessions.is_empty() { return tips; }

    // Long session cost spike
    let long: Vec<_> = sessions.iter().filter(|s| s.turn_count >= 15).collect();
    let short_: Vec<_> = sessions.iter().filter(|s| s.turn_count < 15 && s.turn_count > 1).collect();
    if !long.is_empty() && !short_.is_empty() {
        let avg_long = long.iter().map(|s| s.total_usd).sum::<f64>() / long.len() as f64;
        let avg_short = short_.iter().map(|s| s.total_usd).sum::<f64>() / short_.len() as f64;
        if avg_long > avg_short * 1.8 {
            tips.push(AdvisorTip {
                title: "Your longest sessions are your most expensive".to_string(),
                detail: format!(
                    "Sessions with 15+ turns cost ${avg_long:.2} on average vs ${avg_short:.2} for shorter ones. Starting a fresh session for a new task instead of extending costs significantly less."
                ),
                value: avg_long - avg_short,
                kind: "session_length".to_string(),
            });
        }
    }

    // Opus usage
    let opus_sessions: Vec<_> = sessions.iter().filter(|s| s.models_used.contains(&"opus".to_string())).collect();
    if !opus_sessions.is_empty() {
        let avg_opus = opus_sessions.iter().map(|s| s.total_usd).sum::<f64>() / opus_sessions.len() as f64;
        let projected_save = avg_opus * opus_sessions.len() as f64 * 0.8;
        tips.push(AdvisorTip {
            title: format!("You used Opus in {} sessions this month", opus_sessions.len()),
            detail: format!(
                "Opus averaged ${avg_opus:.2} per session. Sonnet handles most tasks at 1/5 the cost. Projected saving if you switch: ${projected_save:.2}."
            ),
            value: projected_save,
            kind: "model_cost".to_string(),
        });
    }

    // Context fatigue
    let compact_sessions: Vec<_> = sessions.iter().filter(|s| s.hit_compact).collect();
    let non_compact: Vec<_> = sessions.iter().filter(|s| !s.hit_compact && s.turn_count > 1).collect();
    if compact_sessions.len() >= 2 && !non_compact.is_empty() {
        let avg_compact = compact_sessions.iter().map(|s| s.total_usd).sum::<f64>() / compact_sessions.len() as f64;
        let avg_non = non_compact.iter().map(|s| s.total_usd).sum::<f64>() / non_compact.len() as f64;
        let extra = (avg_compact - avg_non).max(0.0);
        if extra > 0.5 {
            tips.push(AdvisorTip {
                title: format!("{} sessions ran out of working memory", compact_sessions.len()),
                detail: format!(
                    "These sessions cost ${extra:.2} more on average than sessions that stayed under the context limit. Focused, single-task sessions avoid mid-session resets."
                ),
                value: extra * compact_sessions.len() as f64,
                kind: "context_fatigue".to_string(),
            });
        }
    }

    // High correction rate
    let total_turns: usize = sessions.iter().map(|s| s.turn_count).sum();
    let total_corrections: usize = sessions.iter().map(|s| s.correction_turns).sum();
    if total_turns > 5 && total_corrections as f64 / total_turns as f64 > 0.15 {
        let pct = (total_corrections * 100) / total_turns;
        tips.push(AdvisorTip {
            title: "A lot of your messages are corrections".to_string(),
            detail: format!(
                "{pct}% of your turns ({total_corrections} of {total_turns}) were corrections. Prompts that specify output format, constraints, and scope upfront typically halve the correction rounds."
            ),
            value: total_corrections as f64,
            kind: "correction_rate".to_string(),
        });
    }

    tips.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
    tips.truncate(3);
    tips
}

fn pattern_month_key_now() -> String {
    let n = Utc::now();
    format!("{}-{:02}", n.year(), n.month())
}

fn format_local_day_label(iso_rfc: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(iso_rfc) {
        return dt.with_timezone(&Local).format("%Y-%m-%d").to_string();
    }
    if let Ok(dt) = iso_rfc.parse::<DateTime<Utc>>() {
        return dt.with_timezone(&Local).format("%Y-%m-%d").to_string();
    }
    iso_rfc.chars().take(10).collect()
}

/// Month-scoped alerts using embedding similarity and SQL only (no LLM).
pub fn detect_patterns(conn: &Connection) -> Vec<PatternAlert> {
    let mut alerts = Vec::new();
    if crate::db::ensure_schema(conn).is_err() {
        return alerts;
    }
    let month_key = pattern_month_key_now();

    // 1. Recurring costly shape (embeddings)
    if let Ok(Some((top_pk, project, started_at, top_cost, _top_turns))) = conn
        .query_row(
            r#"SELECT id, project, started_at, total_usd, turn_count
               FROM sessions
               WHERE strftime('%Y-%m', started_at) = ?1
               ORDER BY total_usd DESC
               LIMIT 1"#,
            rusqlite::params![&month_key],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()
    {
        if top_cost > 0.01 {
            if let Ok(sims) = crate::embedder::similar_sessions(conn, top_pk, 15) {
                let mut match_rows: Vec<(f64, i64)> = Vec::new();
                for (sid, sim) in sims {
                    if sim < 0.7 {
                        continue;
                    }
                    if let Ok(Some((c, t))) = conn
                        .query_row(
                            r#"SELECT total_usd, turn_count FROM sessions
                               WHERE id = ?1 AND strftime('%Y-%m', started_at) = ?2"#,
                            rusqlite::params![sid, &month_key],
                            |r| Ok((r.get::<_, f64>(0)?, r.get::<_, i64>(1)?)),
                        )
                        .optional()
                    {
                        match_rows.push((c, t));
                    }
                }
                let n = match_rows.len();
                if n >= 3 {
                    let sum_cost: f64 = match_rows.iter().map(|(c, _)| *c).sum();
                    let avg_cost = sum_cost / n as f64;
                    let sum_turns: i64 = match_rows.iter().map(|(_, t)| *t).sum();
                    let avg_turns = (sum_turns as f64 / n as f64).round().max(1.0) as i64;
                    if avg_cost > 5.0 {
                        let day = format_local_day_label(&started_at);
                        let proj_label = if project.trim().is_empty() {
                            "Unlabeled"
                        } else {
                            project.trim()
                        };
                        alerts.push(PatternAlert {
                            id: "recurring_shape".into(),
                            title: "Recurring costly session shape".into(),
                            detail: format!(
                                "You've had {n} sessions this month like your ${:.2} session on {} in {}. They averaged ${:.2} each and ~{} turns. Start a new session per subtask to trim context buildup.",
                                top_cost, day, proj_label, avg_cost, avg_turns
                            ),
                            value_usd: sum_cost,
                        });
                    }
                }
            }
        }
    }

    // 2. Project concentration
    if let Ok(mut stmt) = conn.prepare(
        r#"SELECT project, SUM(total_usd), COUNT(*)
           FROM sessions
           WHERE strftime('%Y-%m', started_at) = ?1
           GROUP BY project"#,
    ) {
        if let Ok(iter) = stmt.query_map(rusqlite::params![&month_key], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        }) {
            let groups: Vec<(String, f64, i64)> = iter.filter_map(|x| x.ok()).collect();
            if !groups.is_empty() {
                let total_spend: f64 = groups.iter().map(|(_, s, _)| *s).sum();
                let total_sessions: i64 = groups.iter().map(|(_, _, c)| *c).sum();
                if total_spend > 0.01 && total_sessions > 3 {
                    if let Some((pname, pspend, pcnt)) = groups.iter().max_by(|a, b| {
                        a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                    }) {
                        let share = *pspend / total_spend;
                        if share > 0.6 && *pcnt > 3 {
                            let other_spend = total_spend - pspend;
                            let other_sessions = total_sessions - pcnt;
                            let other_avg = if other_sessions > 0 {
                                other_spend / other_sessions as f64
                            } else {
                                0.0
                            };
                            let proj_avg = pspend / *pcnt as f64;
                            let plabel = if pname.trim().is_empty() {
                                "Unlabeled"
                            } else {
                                pname.trim()
                            };
                            let pct = (share * 100.0).round() as i32;
                            alerts.push(PatternAlert {
                                id: "project_concentration".into(),
                                title: "Spend concentrated on one project".into(),
                                detail: format!(
                                    "{} accounts for about {}% of your spend this month (${:.2} across {} sessions). Other projects average ${:.2} per session vs ${:.2} here.",
                                    plabel,
                                    pct,
                                    pspend,
                                    pcnt,
                                    other_avg,
                                    proj_avg
                                ),
                                value_usd: *pspend,
                            });
                        }
                    }
                }
            }
        }
    }

    // 3. Evening vs daytime (local clock on stored timestamps)
    if let Ok(mut stmt) = conn.prepare(
        r#"SELECT started_at, total_usd, correction_turns, turn_count
           FROM sessions
           WHERE strftime('%Y-%m', started_at) = ?1"#,
    ) {
        if let Ok(iter) = stmt.query_map(rusqlite::params![&month_key], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        }) {
            let mut evening_costs: Vec<f64> = Vec::new();
            let mut evening_corr_turns: i64 = 0;
            let mut evening_turns: i64 = 0;
            let mut day_costs: Vec<f64> = Vec::new();
            let mut day_corr_turns: i64 = 0;
            let mut day_turns: i64 = 0;
            for row in iter.flatten() {
                let (st, cost, corrs, turns) = row;
                let hour_opt = DateTime::parse_from_rfc3339(&st)
                    .map(|d| d.with_timezone(&Local).hour())
                    .or_else(|_| st.parse::<DateTime<Utc>>().map(|d| d.with_timezone(&Local).hour()));
                let Ok(h) = hour_opt else { continue };
                if (8..20).contains(&h) {
                    day_costs.push(cost);
                    day_corr_turns += corrs;
                    day_turns += turns;
                } else if h >= 20 {
                    evening_costs.push(cost);
                    evening_corr_turns += corrs;
                    evening_turns += turns;
                }
            }
            if evening_costs.len() >= 2 && day_costs.len() >= 2 {
                let ev_avg = evening_costs.iter().copied().sum::<f64>() / evening_costs.len() as f64;
                let day_avg = day_costs.iter().copied().sum::<f64>() / day_costs.len() as f64;
                if day_avg > 0.01 && ev_avg > day_avg * 1.5 {
                    let ev_cr = if evening_turns > 0 {
                        evening_corr_turns as f64 / evening_turns as f64
                    } else {
                        0.0
                    };
                    let day_cr = if day_turns > 0 {
                        day_corr_turns as f64 / day_turns as f64
                    } else {
                        0.0
                    };
                    let tail = if ev_cr > day_cr + 0.05 && evening_turns > 0 {
                        format!(
                            " Evening sessions showed a higher correction share ({:.0}% of turns) than daytime ({:.0}%).",
                            ev_cr * 100.0,
                            day_cr * 100.0
                        )
                    } else {
                        String::new()
                    };
                    alerts.push(PatternAlert {
                        id: "evening_spike".into(),
                        title: "Evening sessions cost more".into(),
                        detail: format!(
                            "Sessions started after 8pm local averaged ${:.2} vs ${:.2} during the day (8am–8pm).{}",
                            ev_avg, day_avg, tail
                        ),
                        value_usd: (ev_avg - day_avg) * evening_costs.len() as f64,
                    });
                }
            }
        }
    }

    alerts.sort_by(|a, b| {
        b.value_usd
            .partial_cmp(&a.value_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    alerts.truncate(3);
    alerts
}

// ---------------------------------------------------------------------------
// CRC-32 for session IDs
// ---------------------------------------------------------------------------

const CRC32_TABLE: [u32; 256] = build_crc32_table();

const fn build_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            if c & 1 != 0 { c = 0xEDB8_8320 ^ (c >> 1); } else { c >>= 1; }
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}
