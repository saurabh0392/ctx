use anyhow::Result;
use chrono::{DateTime, Utc};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tool schemas are almost always cache reads after request 1.
pub const CACHE_READ_RATE_PER_MTOK: f64 = 0.30;
/// Worst-case (first request, full input pricing) for the same token count.
pub const WORST_CASE_INPUT_RATE_PER_MTOK: f64 = 3.00;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Record {
    pub ts: String,
    #[serde(default)]
    pub tools_removed: usize,
    #[serde(default)]
    pub tokens_saved: usize,
    #[serde(default)]
    pub compress_chars_saved: usize,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub removed_servers: Vec<String>,
    #[serde(default)]
    pub kept_servers: Vec<String>,
    #[serde(default)]
    pub auto_selected: bool,
    #[serde(default)]
    pub auto_trigger: Option<String>,
    #[serde(default)]
    pub inject_fired: bool,
    #[serde(default)]
    pub inject_chars: usize,
    #[serde(default)]
    pub adaptive_chars: usize,
    #[serde(default)]
    pub budget_blocked: bool,
    #[serde(default)]
    pub coach_kind: Option<String>,
    #[serde(default)]
    pub budget_fired: bool,
    #[serde(default)]
    pub behavior_kind: Option<String>,
    /// Primary working directory parsed from the system prompt, if any.
    #[serde(default)]
    pub working_directory: String,
    /// MCP tools remaining in the request body after filtering.
    #[serde(default)]
    pub tools_sent_count: usize,
    /// Tool names from the assistant message (non-stream responses only).
    #[serde(default)]
    pub mcp_tools_invoked: Vec<String>,
    /// Per-server counts of tools still attached to the request.
    #[serde(default)]
    pub tools_sent_by_server: HashMap<String, usize>,
}

#[derive(Serialize)]
pub struct Session {
    pub started_at: String,
    pub duration_mins: i64,
    pub requests: usize,
    pub tools_removed: usize,
    pub tokens_saved: usize,
    pub cost: f64,
    pub profile: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub working_directory: String,
}

pub struct TraceInfo {
    pub removed_servers: Vec<String>,
    pub kept_servers: Vec<String>,
    pub auto_selected: bool,
    pub auto_trigger: Option<String>,
    pub inject_fired: bool,
    pub inject_chars: usize,
    pub adaptive_chars: usize,
    pub budget_blocked: bool,
    pub coach_kind: Option<String>,
    pub budget_fired: bool,
    pub behavior_kind: Option<String>,
    pub working_directory: String,
}

pub fn record_compress(compress_chars_saved: usize, profile: &str, working_directory: &str) {
    let _ = crate::config::ensure_dir();
    append(&Record {
        ts: Utc::now().to_rfc3339(),
        tools_removed: 0,
        tokens_saved: 0,
        compress_chars_saved,
        profile: profile.to_string(),
        removed_servers: vec![],
        kept_servers: vec![],
        auto_selected: false,
        auto_trigger: None,
        inject_fired: false,
        inject_chars: 0,
        adaptive_chars: 0,
        budget_blocked: false,
        coach_kind: None,
        budget_fired: false,
        behavior_kind: None,
        working_directory: working_directory.to_string(),
        tools_sent_count: 0,
        mcp_tools_invoked: vec![],
        tools_sent_by_server: HashMap::new(),
    });
}

pub fn record(tools_removed: usize, tokens_saved: usize, profile: &str, trace: TraceInfo) {
    let _ = crate::config::ensure_dir();
    append(&Record {
        ts: Utc::now().to_rfc3339(),
        tools_removed,
        tokens_saved,
        compress_chars_saved: 0,
        profile: profile.to_string(),
        removed_servers: trace.removed_servers,
        kept_servers: trace.kept_servers,
        auto_selected: trace.auto_selected,
        auto_trigger: trace.auto_trigger,
        inject_fired: trace.inject_fired,
        inject_chars: trace.inject_chars,
        adaptive_chars: trace.adaptive_chars,
        budget_blocked: trace.budget_blocked,
        coach_kind: trace.coach_kind,
        budget_fired: trace.budget_fired,
        behavior_kind: trace.behavior_kind,
        working_directory: trace.working_directory,
        tools_sent_count: 0,
        mcp_tools_invoked: vec![],
        tools_sent_by_server: HashMap::new(),
    });
}

fn append(rec: &Record) {
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let _ = crate::db::insert_request(&conn, rec);
    }
}

pub fn load_records() -> Vec<Record> {
    if let Ok(conn) = crate::db::open_db() {
        if crate::db::ensure_schema(&conn).is_ok() {
            if let Ok(v) = crate::db::load_requests_ordered(&conn) {
                return v;
            }
        }
    }
    vec![]
}

fn record_belongs_to_session(rec: &Record) -> bool {
    rec.tools_removed > 0 || rec.tokens_saved > 0
}

pub fn group_into_sessions(records: &[Record]) -> Vec<Session> {
    let gap_mins = crate::config::Config::load()
        .session_gap_minutes
        .unwrap_or(30) as i64;
    let gap = chrono::Duration::minutes(gap_mins);
    let mut sessions: Vec<Session> = Vec::new();
    let mut current: Option<(
        DateTime<Utc>,
        DateTime<Utc>,
        usize,
        usize,
        usize,
        String,
        String,
    )> = None;

    for rec in records.iter().filter(|r| record_belongs_to_session(r)) {
        let Ok(ts) = rec.ts.parse::<DateTime<Utc>>() else {
            continue;
        };
        let wd = rec.working_directory.clone();
        match current {
            None => {
                current = Some((
                    ts,
                    ts,
                    rec.tools_removed,
                    rec.tokens_saved,
                    1,
                    rec.profile.clone(),
                    wd,
                ));
            }
            Some((start, last, tools, tokens, count, ref profile, ref workdir)) => {
                if ts - last > gap {
                    let duration_mins = (last - start).num_minutes();
                    sessions.push(Session {
                        started_at: start.to_rfc3339(),
                        duration_mins,
                        requests: count,
                        tools_removed: tools,
                        tokens_saved: tokens,
                        cost: (tokens as f64 / 1_000_000.0) * CACHE_READ_RATE_PER_MTOK,
                        profile: profile.clone(),
                        working_directory: workdir.clone(),
                    });
                    current = Some((
                        ts,
                        ts,
                        rec.tools_removed,
                        rec.tokens_saved,
                        1,
                        rec.profile.clone(),
                        wd,
                    ));
                } else {
                    let merged_wd = if !wd.is_empty() {
                        wd.clone()
                    } else {
                        workdir.clone()
                    };
                    current = Some((
                        start,
                        ts,
                        tools + rec.tools_removed,
                        tokens + rec.tokens_saved,
                        count + 1,
                        if rec.profile.is_empty() {
                            profile.clone()
                        } else {
                            rec.profile.clone()
                        },
                        merged_wd,
                    ));
                }
            }
        }
    }
    if let Some((start, last, tools, tokens, count, profile, workdir)) = current {
        let duration_mins = (last - start).num_minutes();
        sessions.push(Session {
            started_at: start.to_rfc3339(),
            duration_mins,
            requests: count,
            tools_removed: tools,
            tokens_saved: tokens,
            cost: (tokens as f64 / 1_000_000.0) * CACHE_READ_RATE_PER_MTOK,
            profile,
            working_directory: workdir,
        });
    }
    sessions.reverse();
    sessions
}

pub fn show() -> Result<()> {
    let records = load_records();

    if records.is_empty() {
        println!("No data yet. Use Claude Code with ctx filtering enabled.");
        println!("  ctx setup");
        return Ok(());
    }

    let filter_recs: Vec<&Record> = records.iter().filter(|r| r.tools_removed > 0).collect();

    let n_filter = filter_recs.len();
    let total_tools: usize = filter_recs.iter().map(|r| r.tools_removed).sum();
    let total_filter_tokens: usize = filter_recs.iter().map(|r| r.tokens_saved).sum();
    let cost_saved = (total_filter_tokens as f64 / 1_000_000.0) * CACHE_READ_RATE_PER_MTOK;

    let fmt = crate::profiles::fmt_k;

    if n_filter > 0 {
        let avg_tools = total_tools / n_filter;
        println!(
            "ctx stripped {} tool definitions across {} requests ({avg_tools} avg/req),",
            total_tools.to_string().green().bold(),
            n_filter,
        );
        println!(
            "saving {} tokens — about {} in API costs (cache-read rate ${}/MTok).",
            fmt(total_filter_tokens).green().bold(),
            format!("${cost_saved:.2}").green().bold(),
            CACHE_READ_RATE_PER_MTOK,
        );
    }

    Ok(())
}

pub fn show_brief() -> Result<()> {
    let records = load_records();
    let last = records.iter().filter(|r| r.tools_removed > 0).last();
    if let Some(rec) = last {
        let fmt = crate::profiles::fmt_k;
        eprintln!(
            "[ctx] last req: -{} tools, {} tokens saved",
            rec.tools_removed,
            fmt(rec.tokens_saved),
        );
    }
    Ok(())
}
