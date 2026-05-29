use axum::{
    Json, Router,
    body::Body,
    extract::Query,
    http::StatusCode,
    http::header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    response::IntoResponse,
    response::Response,
    routing::{get, post},
};
use chrono::{DateTime, Datelike, Duration, Utc};
use rusqlite::params;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Guards concurrent ingest runs triggered by per-turn requests.
/// compare_exchange from false→true to acquire; store(false) to release.
static INGEST_RUNNING: OnceLock<AtomicBool> = OnceLock::new();

fn ingest_running() -> &'static AtomicBool {
    INGEST_RUNNING.get_or_init(|| AtomicBool::new(false))
}

/// Run JSONL ingest once and notify SSE clients when finished.
fn spawn_background_ingest(hook_type: Option<String>) {
    if ingest_running()
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    tokio::spawn(async move {
        let _ = tokio::task::spawn_blocking(|| {
            let _ = crate::conversations::ingest_claude_jsonl();
        })
        .await;
        ingest_running().store(false, Ordering::Release);
        crate::dashboard_push::notify(crate::dashboard_push::DashboardEvent {
            kind: "ingest_complete".into(),
            hook_type,
        });
    });
}

use crate::analytics::{group_into_sessions, load_records};

const HTML: &str = include_str!("dashboard.html");

/// Returns true when a session's `started_at` is at or after the ctx activation date.
/// If no activation date is recorded, returns true (no filtering).
fn session_after_ctx(started_at: &str, ctx_since: Option<&str>) -> bool {
    let Some(since) = ctx_since else { return true };
    started_at >= since
}

/// `?since=all` disables the install watermark filter for this request.
#[derive(Deserialize, Default, Clone)]
struct SinceQuery {
    since: Option<String>,
}

fn use_ctx_watermark(q: &SinceQuery) -> bool {
    q.since.as_deref() != Some("all")
}

fn watermark_ts(conn: &rusqlite::Connection, q: &SinceQuery) -> Option<String> {
    if !use_ctx_watermark(q) {
        return None;
    }
    crate::db::get_ctx_active_since(conn)
}

fn record_ts_after_watermark(ts: &str, wm: Option<&str>) -> bool {
    match wm {
        None => true,
        Some(s) => ts >= s,
    }
}

fn fmt_tok(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn open_ctx_db() -> Option<rusqlite::Connection> {
    let c = crate::db::open_db().ok()?;
    crate::db::ensure_schema(&c).ok()?;
    Some(c)
}

fn timeline_from_sessions(conn: &rusqlite::Connection, cutoff_iso: &str, ctx_since: Option<&str>) -> Vec<TimelinePoint> {
    let effective_cutoff = match ctx_since {
        Some(s) if s > cutoff_iso => s,
        _ => cutoff_iso,
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT substr(started_at, 1, 10) AS d,
                CAST(COALESCE(SUM(cache_read_tokens), 0) AS INTEGER) AS tok,
                COALESCE(SUM(total_usd), 0.0) AS spend,
                CAST(COALESCE(SUM(turn_count), 0) AS INTEGER) AS turns
         FROM sessions
         WHERE started_at >= ?1
         GROUP BY d
         ORDER BY d ASC",
    ) else {
        return vec![];
    };
    let rows = stmt.query_map(params![effective_cutoff], |r| {
        Ok(TimelinePoint {
            date: r.get(0)?,
            tokens: r.get::<_, i64>(1)? as usize,
            cost: r.get(2)?,
            requests: r.get::<_, i64>(3)? as usize,
        })
    });
    let Ok(rows) = rows else { return vec![] };
    rows.filter_map(|x| x.ok()).collect()
}

fn map_savings_session_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<crate::analytics::Session> {
    Ok(crate::analytics::Session {
        started_at: r.get(0)?,
        duration_mins: r.get::<_, i64>(1)?,
        requests: r.get::<_, i64>(2)? as usize,
        tools_removed: r.get::<_, i64>(3)? as usize,
        tokens_saved: r.get::<_, i64>(4)? as usize,
        cost: r.get(5)?,
        profile: r.get(6)?,
        working_directory: r.get(7)?,
    })
}

fn savings_sessions_from_db(conn: &rusqlite::Connection, watermark: Option<&str>) -> Vec<crate::analytics::Session> {
    let mut out = Vec::new();
    if let Some(since) = watermark {
        let batch: Vec<crate::analytics::Session> = {
            let Ok(mut stmt) = conn.prepare(
                "SELECT started_at, COALESCE(duration_mins, 0), COALESCE(turn_count, 0),
                COALESCE(tools_removed, 0), COALESCE(cache_read_tokens, 0), COALESCE(total_usd, 0.0),
                COALESCE(profile, ''), COALESCE(NULLIF(TRIM(working_directory), ''), project)
         FROM sessions
         WHERE started_at >= ?1
         ORDER BY started_at DESC
         LIMIT 20",
            ) else {
                return vec![];
            };
            let x = match stmt.query_map(params![since], map_savings_session_row) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            };
            x
        };
        out.extend(batch);
    } else {
        let batch: Vec<crate::analytics::Session> = {
            let Ok(mut stmt) = conn.prepare(
                "SELECT started_at, COALESCE(duration_mins, 0), COALESCE(turn_count, 0),
                COALESCE(tools_removed, 0), COALESCE(cache_read_tokens, 0), COALESCE(total_usd, 0.0),
                COALESCE(profile, ''), COALESCE(NULLIF(TRIM(working_directory), ''), project)
         FROM sessions
         ORDER BY started_at DESC
         LIMIT 20",
            ) else {
                return vec![];
            };
            let x = match stmt.query_map([], map_savings_session_row) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            };
            x
        };
        out.extend(batch);
    }
    out
}

fn savings_sessions_from_hook_traces(
    conn: &rusqlite::Connection,
    watermark: Option<&str>,
) -> Vec<crate::analytics::Session> {
    let sql = "SELECT MIN(ts) AS started_at,
                      CAST((julianday(MAX(ts)) - julianday(MIN(ts))) * 24 * 60 AS INTEGER) AS duration_mins,
                      COUNT(*) AS requests,
                      COALESCE(SUM(tools_removed), 0) AS tools_removed,
                      COALESCE(SUM(tokens_saved), 0) AS tokens_saved,
                      COALESCE(SUM(cost_usd), 0.0) AS cost,
                      COALESCE(MAX(profile), '') AS profile,
                      COALESCE(MAX(working_directory), '') AS working_directory
               FROM hook_traces
               WHERE (?1 IS NULL OR ts >= ?1)
               GROUP BY COALESCE(session_id, id)
               ORDER BY started_at DESC
               LIMIT 20";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return vec![];
    };
    let rows = stmt.query_map(params![watermark], map_savings_session_row);
    let Ok(rows) = rows else {
        return vec![];
    };
    rows.filter_map(|r| r.ok()).collect()
}

fn spend_sessions_from_hook_traces(
    conn: &rusqlite::Connection,
    watermark: Option<&str>,
) -> Vec<crate::conversations::SessionCost> {
    let sql = "SELECT COALESCE(session_id, CAST(id AS TEXT)) AS session_id,
                      COALESCE(MAX(working_directory), '') AS project,
                      MIN(ts) AS started_at,
                      COUNT(*) AS turn_count,
                      COALESCE(SUM(cost_usd), 0.0) AS total_usd,
                      COALESCE(SUM(input_tokens), 0) AS input_tokens,
                      COALESCE(SUM(cache_creation_tokens), 0) AS cache_creation_tokens,
                      COALESCE(SUM(cache_read_tokens), 0) AS cache_read_tokens,
                      COALESCE(SUM(output_tokens), 0) AS output_tokens,
                      COALESCE(MAX(model), '') AS model
               FROM hook_traces
               WHERE (?1 IS NULL OR ts >= ?1)
               GROUP BY COALESCE(session_id, id)
               ORDER BY started_at DESC
               LIMIT 20";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return vec![];
    };
    let Ok(rows) = stmt.query_map(params![watermark], |r| {
        let model: String = r.get(9)?;
        let models_used = if model.is_empty() {
            vec![]
        } else {
            vec![model]
        };
        Ok(crate::conversations::SessionCost {
            session_id: r.get(0)?,
            project: r.get(1)?,
            started_at: r.get(2)?,
            first_user_message: String::new(),
            turn_count: r.get::<_, i64>(3)? as usize,
            total_usd: r.get(4)?,
            input_tokens: r.get::<_, i64>(5)? as usize,
            cache_creation_tokens: r.get::<_, i64>(6)? as usize,
            cache_read_tokens: r.get::<_, i64>(7)? as usize,
            output_tokens: r.get::<_, i64>(8)? as usize,
            models_used,
            hit_compact: false,
            clarifying_turns: 0,
            correction_turns: 0,
            top_turns: vec![],
        })
    }) else {
        return vec![];
    };
    rows.filter_map(|r| r.ok()).collect()
}

fn timeline_from_hook_traces(
    conn: &rusqlite::Connection,
    cutoff_iso: &str,
    watermark: Option<&str>,
) -> Vec<TimelinePoint> {
    let effective_cutoff = match watermark {
        Some(s) if s > cutoff_iso => s,
        _ => cutoff_iso,
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT substr(ts, 1, 10) AS d,
                CAST(COALESCE(SUM(tokens_saved), 0) AS INTEGER) AS tok,
                COALESCE(SUM(cost_usd), 0.0) AS spend,
                COUNT(*) AS turns
         FROM hook_traces
         WHERE ts >= ?1
         GROUP BY d
         ORDER BY d ASC",
    ) else {
        return vec![];
    };
    let rows = stmt.query_map(params![effective_cutoff], |r| {
        Ok(TimelinePoint {
            date: r.get(0)?,
            tokens: r.get::<_, i64>(1)? as usize,
            cost: r.get(2)?,
            requests: r.get::<_, i64>(3)? as usize,
        })
    });
    let Ok(rows) = rows else {
        return vec![];
    };
    rows.filter_map(|x| x.ok()).collect()
}

fn hook_trace_month_spend(conn: &rusqlite::Connection, watermark: Option<&str>, month: &str) -> f64 {
    let pattern = format!("{month}%");
    conn.query_row(
        "SELECT COALESCE(SUM(cost_usd), 0.0)
         FROM hook_traces
         WHERE ts LIKE ?1 AND (?2 IS NULL OR ts >= ?2)",
        params![pattern, watermark],
        |r| r.get(0),
    )
    .unwrap_or(0.0)
}

fn hook_trace_session_count(conn: &rusqlite::Connection, watermark: Option<&str>, month: &str) -> usize {
    let pattern = format!("{month}%");
    conn.query_row(
        "SELECT COUNT(DISTINCT COALESCE(session_id, CAST(id AS TEXT)))
         FROM hook_traces
         WHERE ts LIKE ?1 AND (?2 IS NULL OR ts >= ?2)",
        params![pattern, watermark],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
    .max(0) as usize
}

/// Aggregate filter stats from hook_traces when proxy `requests` table is empty.
fn hook_trace_filter_totals(conn: &rusqlite::Connection, watermark: Option<&str>) -> (usize, usize, usize, usize) {
    conn.query_row(
        "SELECT COUNT(*) AS requests,
                COALESCE(SUM(tokens_saved), 0) AS tokens,
                COALESCE(SUM(tools_removed), 0) AS tools_removed,
                COALESCE(SUM(tools_kept), 0) AS tools_kept
         FROM hook_traces
         WHERE (?1 IS NULL OR ts >= ?1)",
        params![watermark],
        |r| {
            Ok((
                r.get::<_, i64>(0)? as usize,
                r.get::<_, i64>(1)? as usize,
                r.get::<_, i64>(2)? as usize,
                r.get::<_, i64>(3)? as usize,
            ))
        },
    )
    .unwrap_or((0, 0, 0, 0))
}

/// Per-profile breakdown from hook_traces (slug -> (requests, tokens, auto_count)).
fn profiles_analytics_from_hook_traces(
    conn: &rusqlite::Connection,
    watermark: Option<&str>,
) -> HashMap<String, (usize, usize, usize)> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(NULLIF(TRIM(effective_profile), ''), NULLIF(TRIM(profile), ''), 'all') AS slug,
                COUNT(*) AS requests,
                COALESCE(SUM(tokens_saved), 0) AS tokens,
                COALESCE(SUM(CASE WHEN auto_selected != 0 THEN 1 ELSE 0 END), 0) AS auto_count
         FROM hook_traces
         WHERE (?1 IS NULL OR ts >= ?1)
         GROUP BY slug",
    ) else {
        return HashMap::new();
    };
    let Ok(rows) = stmt.query_map(params![watermark], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)? as usize,
            r.get::<_, i64>(2)? as usize,
            r.get::<_, i64>(3)? as usize,
        ))
    }) else {
        return HashMap::new();
    };
    rows.filter_map(|r| r.ok())
        .map(|(slug, requests, tokens, auto_count)| (slug, (requests, tokens, auto_count)))
        .collect()
}

fn map_project_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectRow> {
    Ok(ProjectRow {
        working_directory: r.get(0)?,
        requests: r.get::<_, i64>(1)? as usize,
        tokens_saved: r.get::<_, i64>(2)? as usize,
        cost_saved: r.get(3)?,
    })
}

fn projects_from_sessions(conn: &rusqlite::Connection, watermark: Option<&str>) -> Vec<ProjectRow> {
    let mut out = Vec::new();
    if let Some(since) = watermark {
        let batch: Vec<ProjectRow> = {
            let Ok(mut stmt) = conn.prepare(
                "SELECT COALESCE(NULLIF(TRIM(working_directory), ''), '(unknown)') AS wd,
                CAST(COALESCE(SUM(turn_count), 0) AS INTEGER) AS turns,
                CAST(COALESCE(SUM(cache_read_tokens), 0) AS INTEGER) AS toks,
                COALESCE(SUM(total_usd), 0.0) AS spend
         FROM sessions
         WHERE started_at >= ?1
         GROUP BY wd
         ORDER BY spend DESC
         LIMIT 40",
            ) else {
                return vec![];
            };
            let x = match stmt.query_map(params![since], map_project_row) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            };
            x
        };
        out.extend(batch);
    } else {
        let batch: Vec<ProjectRow> = {
            let Ok(mut stmt) = conn.prepare(
                "SELECT COALESCE(NULLIF(TRIM(working_directory), ''), '(unknown)') AS wd,
                CAST(COALESCE(SUM(turn_count), 0) AS INTEGER) AS turns,
                CAST(COALESCE(SUM(cache_read_tokens), 0) AS INTEGER) AS toks,
                COALESCE(SUM(total_usd), 0.0) AS spend
         FROM sessions
         GROUP BY wd
         ORDER BY spend DESC
         LIMIT 40",
            ) else {
                return vec![];
            };
            let x = match stmt.query_map([], map_project_row) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            };
            x
        };
        out.extend(batch);
    }
    out
}

fn map_server_heat_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ServerHeat> {
    let server: String = r.get(0)?;
    let n: i64 = r.get(1)?;
    Ok(ServerHeat {
        server,
        tools_sent: 0,
        tools_invoked: n as usize,
    })
}

fn tool_usage_from_invocations(conn: &rusqlite::Connection, watermark: Option<&str>) -> Vec<ServerHeat> {
    let mut out = Vec::new();
    if let Some(since) = watermark {
        let batch: Vec<ServerHeat> = {
            let Ok(mut stmt) = conn.prepare(
                "SELECT server_prefix, CAST(COUNT(*) AS INTEGER) AS n
         FROM tool_invocations
         WHERE ts >= ?1
         GROUP BY server_prefix
         ORDER BY n DESC
         LIMIT 40",
            ) else {
                return vec![];
            };
            let x = match stmt.query_map(params![since], map_server_heat_row) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            };
            x
        };
        out.extend(batch);
    } else {
        let batch: Vec<ServerHeat> = {
            let Ok(mut stmt) = conn.prepare(
                "SELECT server_prefix, CAST(COUNT(*) AS INTEGER) AS n
         FROM tool_invocations
         GROUP BY server_prefix
         ORDER BY n DESC
         LIMIT 40",
            ) else {
                return vec![];
            };
            let x = match stmt.query_map([], map_server_heat_row) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            };
            x
        };
        out.extend(batch);
    }
    out
}

const VERDICT_MIN_PROMPTS: usize = 20;

#[derive(Default)]
struct HookGateTotals {
    trace_count: usize,
    filter_count: usize,
    filter_tokens: usize,
    inject_count: usize,
    adaptive_count: usize,
    auto_count: usize,
    coach_count: usize,
    budget_count: usize,
    budget_blocked_count: usize,
    inject_chars: usize,
    adaptive_chars: usize,
}

fn prompt_word(n: usize) -> &'static str {
    if n == 1 {
        "prompt"
    } else {
        "prompts"
    }
}

fn too_early_verdict(feature: &str, count: usize) -> String {
    format!(
        "{feature} on {count} {} today — need about {VERDICT_MIN_PROMPTS} before a recommendation.",
        prompt_word(count)
    )
}

fn hook_trace_gate_totals(
    conn: &rusqlite::Connection,
    today: &str,
    wm: Option<&str>,
) -> HookGateTotals {
    let row: (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) = if let Some(since) = wm {
        conn.query_row(
            "SELECT
                COUNT(*),
                COALESCE(SUM(CASE WHEN tools_removed > 0 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(tokens_saved), 0),
                COALESCE(SUM(CASE WHEN inject_fired = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN adaptive_fired = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN auto_selected = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN coach_kind IS NOT NULL THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN budget_fired = 1 OR budget_blocked = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN budget_blocked = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(inject_chars), 0),
                COALESCE(SUM(adaptive_chars), 0)
             FROM hook_traces
             WHERE substr(ts, 1, 10) = ?1 AND ts >= ?2",
            params![today, since],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                ))
            },
        )
    } else {
        conn.query_row(
            "SELECT
                COUNT(*),
                COALESCE(SUM(CASE WHEN tools_removed > 0 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(tokens_saved), 0),
                COALESCE(SUM(CASE WHEN inject_fired = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN adaptive_fired = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN auto_selected = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN coach_kind IS NOT NULL THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN budget_fired = 1 OR budget_blocked = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN budget_blocked = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(inject_chars), 0),
                COALESCE(SUM(adaptive_chars), 0)
             FROM hook_traces
             WHERE substr(ts, 1, 10) = ?1",
            params![today],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                ))
            },
        )
    }
    .unwrap_or((0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0));

    HookGateTotals {
        trace_count: row.0.max(0) as usize,
        filter_count: row.1.max(0) as usize,
        filter_tokens: row.2.max(0) as usize,
        inject_count: row.3.max(0) as usize,
        adaptive_count: row.4.max(0) as usize,
        auto_count: row.5.max(0) as usize,
        coach_count: row.6.max(0) as usize,
        budget_count: row.7.max(0) as usize,
        budget_blocked_count: row.8.max(0) as usize,
        inject_chars: row.9.max(0) as usize,
        adaptive_chars: row.10.max(0) as usize,
    }
}

fn merge_request_prefix_totals(
    totals: &mut HookGateTotals,
    today_recs: &[&crate::analytics::Record],
) {
    for r in today_recs {
        totals.inject_chars += r.inject_chars;
        totals.adaptive_chars += r.adaptive_chars;
        if r.budget_fired || r.budget_blocked {
            totals.budget_count = totals.budget_count.saturating_add(1);
        }
        if r.budget_blocked {
            totals.budget_blocked_count = totals.budget_blocked_count.saturating_add(1);
        }
    }
}

fn correction_rate_7d(conn: &rusqlite::Connection, wm: Option<&str>) -> Option<f64> {
    let rate: f64 = if let Some(since) = wm {
        conn.query_row(
            "SELECT AVG(CAST(correction_turns AS REAL) / MAX(turn_count, 1))
             FROM sessions
             WHERE started_at >= datetime('now', '-7 days')
               AND started_at >= ?1
               AND turn_count > 0",
            params![since],
            |r| r.get(0),
        )
    } else {
        conn.query_row(
            "SELECT AVG(CAST(correction_turns AS REAL) / MAX(turn_count, 1))
             FROM sessions
             WHERE started_at >= datetime('now', '-7 days')
               AND turn_count > 0",
            [],
            |r| r.get(0),
        )
    }
    .unwrap_or(None)
    .unwrap_or(0.0);
    if rate > 0.0 {
        Some(rate)
    } else {
        None
    }
}

fn correction_rate_with_coaching(
    conn: &rusqlite::Connection,
    wm: Option<&str>,
) -> Option<f64> {
    let rate: f64 = if let Some(since) = wm {
        conn.query_row(
            "SELECT AVG(CAST(s.correction_turns AS REAL) / MAX(s.turn_count, 1))
             FROM sessions s
             WHERE s.started_at >= datetime('now', '-7 days')
               AND s.started_at >= ?1
               AND s.turn_count > 0
               AND EXISTS (
                 SELECT 1 FROM hook_traces h
                 WHERE h.coach_kind IS NOT NULL
                   AND h.session_id IS NOT NULL
                   AND s.external_key LIKE '%' || h.session_id || '%'
               )",
            params![since],
            |r| r.get(0),
        )
    } else {
        conn.query_row(
            "SELECT AVG(CAST(s.correction_turns AS REAL) / MAX(s.turn_count, 1))
             FROM sessions s
             WHERE s.started_at >= datetime('now', '-7 days')
               AND s.turn_count > 0
               AND EXISTS (
                 SELECT 1 FROM hook_traces h
                 WHERE h.coach_kind IS NOT NULL
                   AND h.session_id IS NOT NULL
                   AND s.external_key LIKE '%' || h.session_id || '%'
               )",
            [],
            |r| r.get(0),
        )
    }
    .unwrap_or(None)
    .unwrap_or(0.0);
    if rate > 0.0 {
        Some(rate)
    } else {
        None
    }
}

fn enrich_gate_stats(
    gates: &mut [GateStat],
    active_profile: &str,
    hook_only: bool,
    personal_ready: bool,
    corr_rate_7d: Option<f64>,
    coach_rate_with_coaching: Option<f64>,
    budget_threshold: f64,
    auto_tokens_saved: usize,
    auto_tools_removed: usize,
    totals: &HookGateTotals,
) {
    for g in gates.iter_mut() {
        match g.id.as_str() {
            "filter" => {
                g.impact_kind = "cost".into();
                if g.today_tokens > 0 && g.today_count >= VERDICT_MIN_PROMPTS {
                    g.impact_primary = format!("~{} tokens stripped today", fmt_tok(g.today_tokens));
                    g.impact_secondary = Some(format!("{} prompts with fewer tools", g.today_count));
                    g.verdict = "keep".into();
                    g.verdict_detail =
                        "Profile filtering is saving context — keep your active profile on.".into();
                } else if g.today_tokens > 0 {
                    g.impact_primary = format!(
                        "~{} tokens stripped on {} {} today",
                        fmt_tok(g.today_tokens),
                        g.today_count,
                        prompt_word(g.today_count)
                    );
                    g.impact_secondary = Some(
                        "Pinned profile strips tools — separate from auto-profile picking".into(),
                    );
                    g.verdict = "early".into();
                    g.verdict_detail = too_early_verdict("Filtering", g.today_count);
                } else if active_profile == "all" && !personal_ready {
                    g.impact_primary =
                        "Measuring your MCP tool universe — no strip while on `all`".into();
                    g.impact_secondary = Some(g.detail.clone());
                    g.verdict = "early".into();
                    g.verdict_detail =
                        "ctx is collecting usage until your personal profile auto-activates.".into();
                } else if active_profile == "all" {
                    g.impact_primary = "On `all` — every tool stays visible".into();
                    g.verdict = "review".into();
                    g.verdict_detail =
                        "Switch to a narrower profile when you want token savings.".into();
                } else if g.today_count > 0 {
                    g.impact_primary = "Filtering active — token savings may be small today".into();
                    g.verdict = "early".into();
                    g.verdict_detail = too_early_verdict("Filtering", g.today_count);
                } else {
                    g.impact_primary = "No tool strips recorded today".into();
                    g.verdict = "early".into();
                    g.verdict_detail = "Use MCP tools normally — activity shows up after prompts.".into();
                }
            }
            "auto" => {
                g.impact_kind = "quality".into();
                if !g.enabled {
                    g.impact_primary = "Auto-profile is off in config".into();
                    g.verdict = "off".into();
                    g.verdict_detail = "Enable auto_profile in ~/.ctx/config.toml to switch by folder."
                        .into();
                } else if g.today_count > 0 {
                    g.impact_primary = format!(
                        "{} auto match{} today",
                        g.today_count,
                        if g.today_count == 1 { "" } else { "es" }
                    );
                    if auto_tokens_saved > 0 {
                        g.impact_secondary = Some(format!(
                            "~{} tok saved on matched prompts (−{} tools)",
                            fmt_tok(auto_tokens_saved),
                            auto_tools_removed
                        ));
                    } else {
                        g.impact_secondary =
                            Some("Matched via similar past work or path/keyword rules".into());
                    }
                    if g.today_count >= VERDICT_MIN_PROMPTS {
                        g.verdict = "keep".into();
                        g.verdict_detail =
                            "Auto-profile is routing you — review matches in the feed below.".into();
                    } else {
                        g.verdict = "early".into();
                        g.verdict_detail = too_early_verdict("Auto-profile", g.today_count);
                    }
                } else if totals.filter_count > 0 && active_profile != "all" {
                    g.impact_primary = format!(
                        "No auto match today — pinned `{active_profile}` is filtering"
                    );
                    g.impact_secondary = Some(format!(
                        "{} {} stripped tools today (filter ≠ auto)",
                        totals.filter_count,
                        prompt_word(totals.filter_count)
                    ));
                    g.verdict = "early".into();
                    g.verdict_detail = "Auto picks a profile from similar sessions or path rules. \
                        Filtering always uses your pinned profile."
                        .into();
                } else {
                    g.impact_primary = "No auto matches today".into();
                    g.verdict = "early".into();
                    g.verdict_detail = "Matches when cwd fits a path pattern or past sessions \
                        vote for a profile."
                        .into();
                }
            }
            "inject" => {
                g.impact_kind = "cost".into();
                if !g.enabled {
                    g.impact_primary = "Static prefix not active".into();
                    g.verdict = "off".into();
                    g.verdict_detail =
                        "Add system_prefix.md or enable inject in config.".into();
                } else if g.today_count > 0 {
                    g.impact_primary = format!(
                        "Prefix applied on {} prompt{}",
                        g.today_count,
                        if g.today_count == 1 { "" } else { "s" }
                    );
                    if totals.inject_chars > 0 {
                        g.chars_added = Some(totals.inject_chars);
                        g.impact_secondary = Some(format!(
                            "~{} chars (~{} tok) added today",
                            totals.inject_chars,
                            fmt_tok(totals.inject_chars / 4)
                        ));
                    } else {
                        g.impact_secondary =
                            Some("Adds context each turn — check Experiment for cost delta".into());
                    }
                    if g.today_count >= VERDICT_MIN_PROMPTS {
                        g.verdict = "review".into();
                        g.verdict_detail =
                            "Enough samples — use Experiment tab to see if the prefix pays for itself."
                                .into();
                    } else {
                        g.verdict = "early".into();
                        g.verdict_detail = too_early_verdict("System prefix", g.today_count);
                    }
                } else {
                    g.impact_primary = "Enabled but no fires yet today".into();
                    g.verdict = "early".into();
                    g.verdict_detail = "Send a prompt through Claude Code with ctx active.".into();
                }
            }
            "adaptive" => {
                g.impact_kind = "cost".into();
                if !g.enabled {
                    g.impact_primary = "Adaptive prefix disabled".into();
                    g.verdict = "off".into();
                    g.verdict_detail = "Turn on adaptive_prefix_enabled in config.".into();
                } else if g.today_count > 0 {
                    g.impact_primary = format!(
                        "Learned prefix on {} prompt{}",
                        g.today_count,
                        if g.today_count == 1 { "" } else { "s" }
                    );
                    if totals.adaptive_chars > 0 {
                        g.chars_added = Some(totals.adaptive_chars);
                        g.impact_secondary = Some(format!(
                            "~{} chars (~{} tok) added today",
                            totals.adaptive_chars,
                            fmt_tok(totals.adaptive_chars / 4)
                        ));
                    } else {
                        g.impact_secondary =
                            Some("Built from indexed sessions — quality impact in Experiment".into());
                    }
                    if g.today_count >= VERDICT_MIN_PROMPTS {
                        g.verdict = "review".into();
                        g.verdict_detail =
                            "Enough samples — use Experiment tab before deciding to keep adaptive on."
                                .into();
                    } else {
                        g.verdict = "early".into();
                        g.verdict_detail = too_early_verdict("Adaptive prefix", g.today_count);
                    }
                } else {
                    g.impact_primary = "Waiting for enough session history".into();
                    g.verdict = "early".into();
                    g.verdict_detail =
                        "Adaptive prefix fires once ctx has indexed sessions to learn from.".into();
                }
            }
            "coach" => {
                g.impact_kind = "quality".into();
                if let Some(rate) = corr_rate_7d {
                    g.impact_primary =
                        format!("{:.0}% correction rate over the last 7 days", rate * 100.0);
                } else {
                    g.impact_primary = "Correction rate baseline building".into();
                }
                if let (Some(base), Some(coach)) = (corr_rate_7d, coach_rate_with_coaching) {
                    let delta = coach - base;
                    g.quality_delta = Some(delta as f32);
                    g.impact_secondary = Some(format!(
                        "{:.0}% on coached sessions vs {:.0}% baseline (7d)",
                        coach * 100.0,
                        base * 100.0
                    ));
                }
                if g.today_count > 0 {
                    if g.impact_secondary.is_none() {
                        g.impact_secondary = Some(format!(
                            "{} coaching signal{} today",
                            g.today_count,
                            if g.today_count == 1 { "" } else { "s" }
                        ));
                    }
                    if g.today_count >= VERDICT_MIN_PROMPTS {
                        g.verdict = "keep".into();
                        g.verdict_detail =
                            "Coaching is intervening on heavy correction patterns.".into();
                    } else {
                        g.verdict = "early".into();
                        g.verdict_detail = too_early_verdict("Coaching", g.today_count);
                    }
                } else {
                    g.verdict = "early".into();
                    g.verdict_detail =
                        "No coaching fires today — that's normal on smooth sessions.".into();
                }
            }
            "behavior" => {
                g.impact_kind = "quality".into();
                if hook_only {
                    g.enabled = false;
                    g.impact_primary = "Not available on hook-only installs".into();
                    g.verdict = "unavailable".into();
                    g.verdict_detail =
                        "Behavior Guard runs on the ctx proxy path, not filter.js hooks.".into();
                } else if g.today_count > 0 {
                    g.impact_primary = format!("{} pattern hint{} today", g.today_count, if g.today_count == 1 { "" } else { "s" });
                    if g.today_count >= VERDICT_MIN_PROMPTS {
                        g.verdict = "keep".into();
                        g.verdict_detail =
                            "Behavior Guard warned about repeating costly patterns.".into();
                    } else {
                        g.verdict = "early".into();
                        g.verdict_detail = too_early_verdict("Behavior guard", g.today_count);
                    }
                } else {
                    g.impact_primary = "Monitoring session patterns".into();
                    g.verdict = "early".into();
                    g.verdict_detail = "Hints appear when history suggests a risky repeat.".into();
                }
            }
            "budget" => {
                g.impact_kind = "control".into();
                if totals.budget_blocked_count > 0 {
                    g.impact_primary = format!(
                        "{} hard block{} today",
                        totals.budget_blocked_count,
                        if totals.budget_blocked_count == 1 { "" } else { "s" }
                    );
                    if g.today_count > totals.budget_blocked_count {
                        g.impact_secondary = Some(format!(
                            "Plus {} soft budget hint{}",
                            g.today_count - totals.budget_blocked_count,
                            if g.today_count - totals.budget_blocked_count == 1 {
                                ""
                            } else {
                                "s"
                            }
                        ));
                    }
                    g.verdict = "keep".into();
                    g.verdict_detail =
                        "Budget Guard blocked or warned on costly sessions.".into();
                } else if g.today_count > 0 {
                    g.impact_primary = format!(
                        "{} budget hint{} today",
                        g.today_count,
                        if g.today_count == 1 { "" } else { "s" }
                    );
                    if g.today_count >= VERDICT_MIN_PROMPTS {
                        g.verdict = "keep".into();
                        g.verdict_detail =
                            "Budget Guard is pacing spend — review threshold in config.".into();
                    } else {
                        g.verdict = "early".into();
                        g.verdict_detail = too_early_verdict("Budget guard", g.today_count);
                    }
                } else {
                    g.impact_primary =
                        format!("~${budget_threshold:.0} session threshold (from monthly budget)");
                    g.verdict = "armed".into();
                    g.verdict_detail =
                        "No threshold crossings today — guard is on and waiting.".into();
                }
            }
            "compress" => {
                g.enabled = false;
                g.impact_kind = "none".into();
                g.impact_primary = "Coming soon — not shipped yet".into();
                g.verdict = "unavailable".into();
                g.verdict_detail =
                    "Bash output compression is on the roadmap; ctx does not compress yet.".into();
                g.today_count = 0;
                g.today_tokens = 0;
            }
            _ => {}
        }
    }
}

fn gate_activity_from_hook_trace(h: &crate::db::HookTraceRow) -> Option<GateActivity> {
    let mut events: Vec<GateEvent> = Vec::new();
    if h.tools_removed > 0 {
        events.push(GateEvent {
            id: "filter".into(),
            label: format!("-{} tools -{}", h.tools_removed, fmt_tok(h.tokens_saved)),
        });
    }
    if h.auto_selected {
        let trig = h.auto_trigger.as_deref().unwrap_or("matched");
        events.push(GateEvent {
            id: "auto".into(),
            label: format!("switched to {} ({})", h.profile, trig),
        });
    }
    if h.inject_fired {
        events.push(GateEvent {
            id: "inject".into(),
            label: "prefix applied".into(),
        });
    }
    if h.adaptive_fired {
        events.push(GateEvent {
            id: "adaptive".into(),
            label: "adaptive prefix".into(),
        });
    }
    if let Some(ref k) = h.coach_kind {
        events.push(GateEvent {
            id: "coach".into(),
            label: k.clone(),
        });
    }
    if h.budget_blocked {
        events.push(GateEvent {
            id: "budget".into(),
            label: "budget block".into(),
        });
    } else if h.budget_fired {
        events.push(GateEvent {
            id: "budget".into(),
            label: "budget hint".into(),
        });
    }
    if events.is_empty() {
        return None;
    }
    Some(GateActivity {
        ts: h.ts.clone(),
        gates: events,
        session_id: h.session_id.clone(),
        working_directory: if h.working_directory.is_empty() {
            None
        } else {
            Some(h.working_directory.clone())
        },
        profile: if h.profile.is_empty() {
            None
        } else {
            Some(h.profile.clone())
        },
        auto_trigger: h.auto_trigger.clone(),
    })
}

fn gates_when_no_requests(
    conn: &rusqlite::Connection,
    today: &str,
    config: &crate::config::Config,
    wm: Option<&str>,
) -> GatesResponse {
    let inject_on = config.inject_enabled && crate::config::system_prefix_path().exists();
    let adaptive_on = config.adaptive_prefix_enabled;
    let auto_on = config.auto_profile_enabled;
    let budget_threshold = crate::budget_guard::session_threshold_usd();

    let (sess_today, corr_sum, compact_sum, turn_sum): (i64, i64, i64, i64) = if let Some(since) = wm {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(correction_turns),0), COALESCE(SUM(hit_compact),0), COALESCE(SUM(turn_count),0)
             FROM sessions WHERE substr(started_at, 1, 10) = ?1 AND started_at >= ?2",
            params![today, since],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap_or((0, 0, 0, 0))
    } else {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(correction_turns),0), COALESCE(SUM(hit_compact),0), COALESCE(SUM(turn_count),0)
             FROM sessions WHERE substr(started_at, 1, 10) = ?1",
            params![today],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap_or((0, 0, 0, 0))
    };

    let totals = hook_trace_gate_totals(conn, today, wm);
    let active_profile = config
        .active_profile
        .clone()
        .unwrap_or_else(|| "all".into());
    let personal_ready = crate::profiles::personal_ready(&crate::profiles::usage_stats());
    let corr_rate = correction_rate_7d(conn, wm);
    let coach_rate = correction_rate_with_coaching(conn, wm);
    let (auto_tokens_saved, auto_tools_removed) = auto_switch_savings(conn, today, wm);

    let corr_n = corr_sum.max(0) as usize;
    let compact_n = compact_sum.max(0) as usize;

    let sessions_note = if wm.is_some() {
        format!(
            "No per-request filter events in ctx.db. Post-ctx session data: {sess_today} sessions today, {turn_sum} turns, {corr_n} correction turns, {compact_n} context compacts (from ingest, filtered to after ctx activation)."
        )
    } else {
        "No per-request filter events in ctx.db. Showing all historical sessions (watermark off).".into()
    };

    let filter_detail = if totals.filter_count > 0 {
        format!("{active_profile} profile · {} strip rows", totals.filter_count)
    } else {
        format!("{active_profile} profile · hook traces only")
    };

    let coach_detail = if totals.coach_count > 0 {
        format!("{} coaching signal{} from hooks", totals.coach_count, if totals.coach_count == 1 { "" } else { "s" })
    } else if corr_n > 0 {
        format!("{corr_n} correction turns today (session ingest)")
    } else {
        "no coaching signals today".into()
    };

    let mut gates = vec![
        make_gate_stat(
            "filter",
            "Profile Filter",
            true,
            filter_detail,
            totals.filter_count,
            totals.filter_tokens,
        ),
        make_gate_stat(
            "auto",
            "Auto-Profile",
            auto_on,
            if totals.auto_count > 0 {
                format!("switched {}× today", totals.auto_count)
            } else {
                "watching cwd".into()
            },
            totals.auto_count,
            0,
        ),
        make_gate_stat(
            "inject",
            "Inject",
            inject_on,
            if inject_on {
                "system_prefix.md"
            } else {
                "no prefix file"
            },
            totals.inject_count,
            0,
        ),
        make_gate_stat(
            "adaptive",
            "Adaptive prefix",
            adaptive_on,
            "Learned from ctx.db session index",
            totals.adaptive_count,
            0,
        ),
        make_gate_stat(
            "coach",
            "Coaching",
            true,
            coach_detail,
            totals.coach_count,
            0,
        ),
        make_gate_stat(
            "behavior",
            "Behavior Guard",
            true,
            "session-derived signals only without request log",
            0,
            0,
        ),
        make_gate_stat(
            "budget",
            "Budget Guard",
            true,
            format!("~${budget_threshold:.0} session threshold (from monthly budget)"),
            totals.budget_count,
            0,
        ),
        make_gate_stat(
            "compress",
            "Bash Compress",
            false,
            "coming soon",
            0,
            0,
        ),
    ];

    enrich_gate_stats(
        &mut gates,
        &active_profile,
        true,
        personal_ready,
        corr_rate,
        coach_rate,
        budget_threshold,
        auto_tokens_saved,
        auto_tools_removed,
        &totals,
    );

    let mut activity: Vec<GateActivity> = Vec::new();
    fn map_sess_gate_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, i64, i64)> {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    }
    if let Some(since) = wm {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT started_at, correction_turns, hit_compact
         FROM sessions
         WHERE (correction_turns > 0 OR hit_compact > 0)
           AND started_at >= datetime('now', '-14 days')
           AND started_at >= ?1
         ORDER BY started_at DESC
         LIMIT 25",
        ) {
            if let Ok(rows) = stmt.query_map(params![since], map_sess_gate_row) {
                for row in rows.flatten() {
                    let (ts, corr, hit) = row;
                    let mut gates_e: Vec<GateEvent> = Vec::new();
                    if corr > 0 {
                        gates_e.push(GateEvent {
                            id: "coach".into(),
                            label: format!("{corr} correction turns"),
                        });
                    }
                    if hit > 0 {
                        gates_e.push(GateEvent {
                            id: "compress".into(),
                            label: "context compact".into(),
                        });
                    }
                    if !gates_e.is_empty() {
                        activity.push(GateActivity {
                            ts,
                            gates: gates_e,
                            session_id: None,
                            working_directory: None,
                            profile: None,
                            auto_trigger: None,
                        });
                    }
                }
            }
        }
    } else if let Ok(mut stmt) = conn.prepare(
        "SELECT started_at, correction_turns, hit_compact
         FROM sessions
         WHERE (correction_turns > 0 OR hit_compact > 0)
           AND started_at >= datetime('now', '-14 days')
         ORDER BY started_at DESC
         LIMIT 25",
    ) {
        if let Ok(rows) = stmt.query_map([], map_sess_gate_row) {
            for row in rows.flatten() {
                let (ts, corr, hit) = row;
                let mut gates_e: Vec<GateEvent> = Vec::new();
                if corr > 0 {
                    gates_e.push(GateEvent {
                        id: "coach".into(),
                        label: format!("{corr} correction turns"),
                    });
                }
                if hit > 0 {
                    gates_e.push(GateEvent {
                        id: "compress".into(),
                        label: "context compact".into(),
                    });
                }
                if !gates_e.is_empty() {
                    activity.push(GateActivity {
                        ts,
                        gates: gates_e,
                        session_id: None,
                        working_directory: None,
                        profile: None,
                        auto_trigger: None,
                    });
                }
            }
        }
    }

    if let Ok(rows) = crate::db::load_hook_traces(conn, 50, 0, wm) {
        for h in rows {
            if let Some(a) = gate_activity_from_hook_trace(&h) {
                activity.push(a);
            }
        }
    }

    activity.sort_by(|a, b| b.ts.cmp(&a.ts));
    activity.truncate(45);

    GatesResponse {
        gates,
        activity,
        sessions_fallback_note: Some(sessions_note),
        hook_only: true,
        active_profile,
        correction_rate_7d: corr_rate,
        prompts_today: totals.trace_count,
        verdict_min_prompts: VERDICT_MIN_PROMPTS,
    }
}

pub async fn serve(port: u16, no_open: bool) -> anyhow::Result<()> {
    let _ = crate::filter_hook::sync_filter_config_from_active_config();
    let _ = crate::behavior_guard::write_behavior_hints_file();

    let _ = crate::db::open_db().and_then(|c| {
        crate::db::ensure_schema(&c)?;
        crate::db::maybe_backfill_requests_from_jsonl(&c)?;
        Ok::<(), anyhow::Error>(())
    });

    // Run JSONL ingest in background so the server binds immediately.
    tokio::spawn(async {
        let _ = tokio::task::spawn_blocking(|| {
            let _ = crate::conversations::ingest_claude_jsonl();
        })
        .await;
    });

    let app = Router::new()
        .route("/", get(serve_html))
        // Tab 1: savings
        .route("/api/stats", get(api_stats))
        .route("/api/ingest-request", post(api_ingest_request))
        .route("/api/hook/event", post(api_hook_event))
        .route("/api/allowance/snapshot", post(api_allowance_snapshot))
        .route("/api/allowance/current", get(api_allowance_current))
        .route("/api/allowance/burn-rate", get(api_allowance_burn_rate))
        .route("/api/savings/tool-mix", get(api_savings_tool_mix))
        .route("/api/savings/access-friction", get(api_savings_access_friction))
        .route("/api/savings/keep-tool", post(api_savings_keep_tool))
        .route("/api/trigger-ingest", post(api_trigger_ingest))
        .route("/api/events/stream", get(crate::dashboard_push::api_events_stream))
        .route("/api/dashboard/push", post(crate::dashboard_push::api_dashboard_push))
        .route("/api/timeline", get(api_timeline))
        .route("/api/sessions", get(api_sessions))
        .route("/api/gates", get(api_gates))
        // Tab 2: prompt stats
        .route("/api/spend/monthly", get(api_spend_monthly))
        .route("/api/spend/sessions", get(api_spend_sessions))
        .route("/api/spend/tips", get(api_spend_tips))
        .route("/api/budget", post(api_set_budget))
        .route("/api/settings", get(api_settings_get).post(api_settings_post))
        .route(
            "/api/settings/refresh-adaptive-prefix",
            post(api_settings_refresh_adaptive_prefix),
        )
        .route("/api/settings/reset-watermark", post(api_settings_reset_watermark))
        .route("/api/settings/mode", post(api_settings_mode))
        .route("/api/settings/purge-prompts", post(api_settings_purge_prompts))
        .route("/api/settings/delete-data", post(api_settings_delete_data))
        .route("/api/settings/export", get(api_settings_export))
        // Tab 3: profiles
        .route("/api/profiles", get(api_profiles))
        .route("/api/profiles/tools", get(api_profiles_tools))
        .route("/api/profiles/switch", post(api_profiles_switch))
        .route("/api/profiles/create", post(api_profiles_create))
        .route("/api/profiles/analytics", get(api_profiles_analytics))
        // Request trace
        .route("/api/requests", get(api_requests))
        .route("/api/hook-events", get(api_hook_events))
        .route("/api/hook-traces", get(api_hook_traces))
        .route("/api/task-costs", get(api_task_costs))
        .route("/api/simulate", post(api_simulate))
        .route("/api/ab-report", get(api_ab_report))
        .route("/api/ab-daily", get(api_ab_daily))
        // Projects + tool heatmap
        .route("/api/projects", get(api_projects))
        .route("/api/tool-usage", get(api_tool_usage))
        // User profile (calibration)
        .route("/api/user-profile", get(api_user_profile))
        .route("/api/similar-sessions", get(api_similar_sessions))
        .route("/api/profile-suggest", post(api_profile_suggest))
        .route("/api/pattern-alerts", get(api_pattern_alerts))
        .route("/api/quality-alerts", get(api_quality_alerts))
        .route("/api/profiles/auto", post(api_profiles_auto))
        .route("/api/profiles/generate", post(api_profiles_generate))
        .route("/api/profiles/readiness", get(api_profiles_readiness))
        .route("/api/project-health", get(api_project_health))
        .route("/api/prompt-clusters", get(api_prompt_clusters));

    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    let url = format!("http://{addr}");
    println!("ctx dashboard running at {url}");
    println!(
        "Event stream: ~/.ctx/ctx.sock (profile, budget, experiment, last-trace, adaptive-status)"
    );
    if !no_open {
        let _ = open::that(&url);
    }

    crate::socket::spawn_socket_task();

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    crate::socket::cleanup_socket_file();
    Ok(())
}

async fn serve_html() -> axum::response::Html<&'static str> {
    axum::response::Html(HTML)
}

/// Claude Code async HTTP hooks (PostToolUse, SessionStart, SessionEnd, Stop).
/// Stop and SessionEnd fire after the turn completes and JSONL is written,
/// so they trigger ingest to enrich pending hook_trace rows.
async fn api_hook_event(Json(payload): Json<serde_json::Value>) -> impl IntoResponse {
    let hook_type = payload
        .get("hook_event_name")
        .or_else(|| payload.get("hookEventName"))
        .and_then(|x| x.as_str())
        .unwrap_or("unknown");
    let payload_s = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let _ = crate::db::insert_hook_event(&conn, hook_type, &payload_s);
    }

    crate::dashboard_push::notify(crate::dashboard_push::DashboardEvent {
        kind: "hook_event".into(),
        hook_type: Some(hook_type.to_string()),
    });

    if matches!(hook_type, "Stop" | "SessionEnd") {
        spawn_background_ingest(Some(hook_type.to_string()));
    }

    StatusCode::OK
}

async fn api_allowance_snapshot(Json(payload): Json<serde_json::Value>) -> impl IntoResponse {
    let Ok(conn) = crate::db::open_db() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let _ = crate::db::ensure_schema(&conn);
    match crate::allowance::ingest_statusline_payload(&conn, &payload) {
        Ok(n) if n > 0 => {
            crate::dashboard_push::notify(crate::dashboard_push::DashboardEvent {
                kind: "allowance_snapshot".into(),
                hook_type: None,
            });
            StatusCode::OK
        }
        Ok(_) => StatusCode::OK,
        Err(e) => {
            eprintln!("allowance snapshot: {e}");
            StatusCode::BAD_REQUEST
        }
    }
}

async fn api_allowance_current() -> Json<crate::allowance::AllowanceCurrentResponse> {
    let Some(conn) = open_ctx_db() else {
        return Json(crate::allowance::AllowanceCurrentResponse {
            configured: false,
            statusline_wired: crate::claude_settings::ctx_statusline_wired_in_settings(),
            stale: true,
            last_statusline_at: None,
            setup_hint: Some("ctx database unavailable.".into()),
            windows: std::collections::HashMap::new(),
        });
    };
    Json(crate::allowance::current_allowance(&conn))
}

async fn api_allowance_burn_rate() -> Json<crate::allowance::AllowanceBurnRateResponse> {
    let Some(conn) = open_ctx_db() else {
        return Json(crate::allowance::AllowanceBurnRateResponse {
            metrics_ready: false,
            window: crate::allowance::PRIMARY_WINDOW.into(),
            ctx_active_since: None,
            baseline_pct_per_hour: None,
            recent_pct_per_hour: None,
            delta_pct: None,
            direction: None,
            message: Some("ctx database unavailable.".into()),
        });
    };
    Json(crate::allowance::burn_rate(&conn))
}

async fn api_savings_tool_mix() -> Json<crate::semantic_tools::ToolMixSummary> {
    let Some(conn) = open_ctx_db() else {
        return Json(crate::semantic_tools::ToolMixSummary::default());
    };
    Json(crate::semantic_tools::load_tool_mix_summary(&conn))
}

async fn api_savings_access_friction() -> Json<Vec<crate::semantic_tools::AccessFrictionRow>> {
    let Some(conn) = open_ctx_db() else {
        return Json(vec![]);
    };
    Json(crate::semantic_tools::list_access_friction(&conn, 1))
}

#[derive(serde::Deserialize)]
struct KeepToolBody {
    tool: String,
}

async fn api_savings_keep_tool(Json(body): Json<KeepToolBody>) -> impl IntoResponse {
    match crate::semantic_tools::promote_tool_to_profile(body.tool.trim()) {
        Ok(()) => StatusCode::OK,
        Err(e) => {
            eprintln!("keep-tool: {e}");
            StatusCode::BAD_REQUEST
        }
    }
}

async fn api_ingest_request(Json(rec): Json<crate::analytics::Record>) -> impl IntoResponse {
    let res: Result<(), anyhow::Error> = (|| {
        let conn = crate::db::open_db()?;
        crate::db::ensure_schema(&conn)?;
        crate::db::insert_request(&conn, &rec)?;
        Ok(())
    })();
    match res {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /api/trigger-ingest
///
/// Called by filter.js after every turn. Spawns `ingest_claude_jsonl()` in a background
/// blocking task so the dashboard reflects the just-completed turn immediately.
///
/// The AtomicBool gate ensures at most one ingest runs at a time — if a previous turn's
/// ingest is still in progress this returns 202 and the in-flight run will pick up the
/// new data (because `ingest_claude_jsonl` rescans modified files on each invocation).
async fn api_trigger_ingest() -> impl IntoResponse {
    spawn_background_ingest(None);
    StatusCode::ACCEPTED
}

// ---------------------------------------------------------------------------
// Tab 1 — Savings (existing)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct Stats {
    total_tokens_saved: usize,
    total_tools_removed: usize,
    /// Tools kept after filtering (sent to Claude).
    total_tools_kept: usize,
    cost_saved: f64,
    /// Same token count at full input pricing (first-request style).
    cost_saved_worst_case: f64,
    session_count: usize,
    request_count: usize,
    active_profile: String,
    proxy_listening: bool,
    session_budget_threshold_usd: f64,
    monthly_burn_projection_usd: Option<f64>,
    /// True when `requests` table is empty and charts use session ingest data.
    #[serde(default)]
    sessions_fallback: bool,
    /// Sum of `total_usd` for sessions in the current calendar month.
    #[serde(default)]
    current_month_session_spend_usd: f64,
    /// Install watermark from `meta.ctx_active_since` when present.
    #[serde(default)]
    ctx_active_since: Option<String>,
    /// True when `?since=all` was not passed and the DB has a watermark (default view hides pre-install rows).
    #[serde(default)]
    dashboard_watermark_filtering: bool,
}

async fn api_stats(Query(q): Query<SinceQuery>) -> Json<Stats> {
    let records = load_records();
    let config = crate::config::Config::load();
    let conn = open_ctx_db();
    let wm = conn.as_ref().and_then(|c| watermark_ts(c, &q));
    let wm_ref = wm.as_deref();

    let filter_recs: Vec<_> = records
        .iter()
        .filter(|r| r.tools_removed > 0)
        .filter(|r| record_ts_after_watermark(&r.ts, wm_ref))
        .collect();
    let mut total_tokens: usize = filter_recs.iter().map(|r| r.tokens_saved).sum();
    let mut total_tools: usize = filter_recs.iter().map(|r| r.tools_removed).sum();
    let mut total_kept: usize = filter_recs.iter().map(|r| r.tools_sent_count).sum();
    let mut request_count = filter_recs.len();
    let rec_filtered: Vec<_> = records
        .iter()
        .filter(|r| record_ts_after_watermark(&r.ts, wm_ref))
        .cloned()
        .collect();
    let sessions = group_into_sessions(&rec_filtered);

    let spend_ctx = if use_ctx_watermark(&q) {
        wm.clone()
            .or_else(|| conn.as_ref().and_then(|c| crate::db::get_ctx_active_since(c)))
    } else {
        None
    };

    let spend_sessions = crate::conversations::all_sessions();
    let now = chrono::Utc::now();
    let current_month = format!("{}-{:02}", now.year(), now.month());
    let mut month_spend: f64 = spend_sessions
        .iter()
        .filter(|s| s.started_at.starts_with(&current_month))
        .filter(|s| session_after_ctx(&s.started_at, spend_ctx.as_deref()))
        .map(|s| s.total_usd)
        .sum();
    if month_spend <= 0.0 && records.is_empty() {
        if let Some(ref c) = conn {
            month_spend = hook_trace_month_spend(c, wm_ref, &current_month);
        }
    }
    let day = now.day().max(1) as f64;
    use chrono::NaiveDate;
    let month_start = NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap();
    let next_month_start = if now.month() == 12 {
        NaiveDate::from_ymd_opt(now.year() + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(now.year(), now.month() + 1, 1).unwrap()
    };
    let days_in_month = (next_month_start - month_start).num_days() as f64;
    let projection = if month_spend > 0.0 && days_in_month > 0.0 {
        Some(month_spend / day * days_in_month)
    } else {
        None
    };

    let mut effective_session_count = if sessions.is_empty() {
        spend_sessions
            .iter()
            .filter(|s| s.started_at.starts_with(&current_month))
            .filter(|s| session_after_ctx(&s.started_at, spend_ctx.as_deref()))
            .count()
    } else {
        sessions.len()
    };
    if effective_session_count == 0 && records.is_empty() {
        if let Some(ref c) = conn {
            effective_session_count = hook_trace_session_count(c, wm_ref, &current_month);
        }
    }

    let sessions_fallback = records.is_empty();
    if sessions_fallback {
        if let Some(ref c) = conn {
            let (ht_req, ht_tokens, ht_tools, ht_kept) = hook_trace_filter_totals(c, wm_ref);
            if ht_req > 0 {
                request_count = ht_req;
                total_tokens = ht_tokens;
                total_tools = ht_tools;
                total_kept = ht_kept;
            }
        }
    }

    let ctx_active_since = conn.as_ref().and_then(|c| crate::db::get_ctx_active_since(c));
    let dashboard_watermark_filtering =
        use_ctx_watermark(&q) && ctx_active_since.is_some();

    Json(Stats {
        total_tokens_saved: total_tokens,
        total_tools_removed: total_tools,
        total_tools_kept: total_kept,
        cost_saved: (total_tokens as f64 / 1_000_000.0) * crate::analytics::CACHE_READ_RATE_PER_MTOK,
        cost_saved_worst_case: (total_tokens as f64 / 1_000_000.0)
            * crate::analytics::WORST_CASE_INPUT_RATE_PER_MTOK,
        session_count: effective_session_count,
        request_count,
        active_profile: config.active_profile.unwrap_or_else(|| "all".into()),
        proxy_listening: std::net::TcpStream::connect(format!(
            "127.0.0.1:{}",
            config.proxy_port.unwrap_or(8788)
        ))
        .is_ok(),
        session_budget_threshold_usd: crate::budget_guard::session_threshold_usd(),
        monthly_burn_projection_usd: projection,
        sessions_fallback,
        current_month_session_spend_usd: month_spend,
        ctx_active_since,
        dashboard_watermark_filtering,
    })
}

#[derive(Serialize)]
struct TimelinePoint {
    date: String,
    tokens: usize,
    cost: f64,
    requests: usize,
}

async fn api_timeline(Query(q): Query<SinceQuery>) -> Json<Vec<TimelinePoint>> {
    let records = load_records();
    let now = Utc::now();
    let cutoff = now - Duration::days(30);
    let cutoff_iso = cutoff.to_rfc3339();
    let conn = open_ctx_db();
    let wm = conn.as_ref().and_then(|c| watermark_ts(c, &q));
    let wm_ref = wm.as_deref();

    if !records.is_empty() {
        let mut by_day: HashMap<String, (usize, usize)> = HashMap::new();
        for rec in records.iter().filter(|r| r.tools_removed > 0) {
            if !record_ts_after_watermark(&rec.ts, wm_ref) {
                continue;
            }
            let Ok(ts) = rec.ts.parse::<DateTime<Utc>>() else { continue };
            if ts < cutoff {
                continue;
            }
            let day = format!("{}-{:02}-{:02}", ts.year(), ts.month(), ts.day());
            let e = by_day.entry(day).or_default();
            e.0 += rec.tokens_saved;
            e.1 += 1;
        }

        let mut points: Vec<TimelinePoint> = by_day
            .into_iter()
            .map(|(date, (tokens, requests))| TimelinePoint {
                date,
                tokens,
                cost: (tokens as f64 / 1_000_000.0) * crate::analytics::CACHE_READ_RATE_PER_MTOK,
                requests,
            })
            .collect();
        points.sort_by(|a, b| a.date.cmp(&b.date));
        return Json(points);
    }

    if let Some(ref c) = conn {
        let ctx_line = if use_ctx_watermark(&q) {
            wm_ref
        } else {
            None
        };
        let points = timeline_from_sessions(c, &cutoff_iso, ctx_line);
        if !points.is_empty() {
            return Json(points);
        }
        let hook_points = timeline_from_hook_traces(c, &cutoff_iso, ctx_line);
        if !hook_points.is_empty() {
            return Json(hook_points);
        }
    }

    Json(vec![])
}

async fn api_sessions(Query(q): Query<SinceQuery>) -> Json<Vec<crate::analytics::Session>> {
    let records = load_records();
    let conn = open_ctx_db();
    let wm = conn.as_ref().and_then(|c| watermark_ts(c, &q));
    let wm_ref = wm.as_deref();
    if !records.is_empty() {
        let rec_filtered: Vec<_> = records
            .iter()
            .filter(|r| record_ts_after_watermark(&r.ts, wm_ref))
            .cloned()
            .collect();
        let mut sessions = group_into_sessions(&rec_filtered);
        sessions.truncate(20);
        return Json(sessions);
    }
    if let Some(ref c) = conn {
        let rows = savings_sessions_from_db(c, wm_ref);
        if !rows.is_empty() {
            return Json(rows);
        }
        let hook_rows = savings_sessions_from_hook_traces(c, wm_ref);
        if !hook_rows.is_empty() {
            return Json(hook_rows);
        }
    }
    Json(vec![])
}

// ---------------------------------------------------------------------------
// Tab 2 — Prompt Stats
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct SpendSessionsQuery {
    month: Option<String>,
    since: Option<String>,
}

async fn api_spend_monthly(Query(q): Query<SinceQuery>) -> Json<Vec<crate::conversations::MonthlySpend>> {
    let all = crate::conversations::all_sessions();
    let conn = open_ctx_db();
    let wm = conn.as_ref().and_then(|c| watermark_ts(c, &q));
    let spend_ctx = if use_ctx_watermark(&q) {
        wm.or_else(|| conn.as_ref().and_then(|c| crate::db::get_ctx_active_since(c)))
    } else {
        None
    };
    let sessions: Vec<_> = all
        .into_iter()
        .filter(|s| session_after_ctx(&s.started_at, spend_ctx.as_deref()))
        .collect();
    let config = crate::config::Config::load();
    let mut months = crate::conversations::monthly_spend(&sessions);
    let now = chrono::Utc::now();
    let current = format!("{}-{:02}", now.year(), now.month());
    if let Some(actual) = config.monthly_actual_spend_usd {
        if let Some(m) = months.iter_mut().find(|m| m.month == current) {
            m.actual_spend_usd = Some(actual);
            m.actual_spend_baseline_usd = config.monthly_actual_spend_baseline_usd;
        }
    }
    if let Some(budget) = config.monthly_budget_usd {
        if let Some(m) = months.iter_mut().find(|m| m.month == current) {
            m.budget_usd = Some(budget);
        }
    }
    Json(months)
}

async fn api_spend_sessions(
    Query(q): Query<SpendSessionsQuery>,
) -> Json<Vec<crate::conversations::SessionCost>> {
    let since_q = SinceQuery {
        since: q.since.clone(),
    };
    let conn = open_ctx_db();
    let wm = conn.as_ref().and_then(|c| watermark_ts(c, &since_q));
    let spend_ctx = if use_ctx_watermark(&since_q) {
        wm.or_else(|| conn.as_ref().and_then(|c| crate::db::get_ctx_active_since(c)))
    } else {
        None
    };
    let mut sessions = crate::conversations::all_sessions();
    sessions.retain(|s| session_after_ctx(&s.started_at, spend_ctx.as_deref()));

    if sessions.is_empty() {
        if let Some(ref c) = conn {
            sessions = spend_sessions_from_hook_traces(c, spend_ctx.as_deref());
        }
    }

    if let Some(month) = &q.month {
        sessions.retain(|s| s.started_at.starts_with(month.as_str()));
    }

    sessions.sort_by(|a, b| b.total_usd.partial_cmp(&a.total_usd).unwrap_or(std::cmp::Ordering::Equal));
    sessions.truncate(20);
    Json(sessions)
}

async fn api_spend_tips(Query(q): Query<SinceQuery>) -> Json<Vec<crate::conversations::AdvisorTip>> {
    let conn = open_ctx_db();
    let wm = conn.as_ref().and_then(|c| watermark_ts(c, &q));
    let spend_ctx = if use_ctx_watermark(&q) {
        wm.or_else(|| conn.as_ref().and_then(|c| crate::db::get_ctx_active_since(c)))
    } else {
        None
    };
    let mut sessions = crate::conversations::all_sessions();
    sessions.retain(|s| session_after_ctx(&s.started_at, spend_ctx.as_deref()));
    let now = chrono::Utc::now();
    let current_month = format!("{}-{:02}", now.year(), now.month());
    sessions.retain(|s| s.started_at.starts_with(&current_month));
    Json(crate::conversations::generate_tips(&sessions))
}

#[derive(Deserialize)]
struct BudgetBody {
    budget_usd: Option<f64>,
    actual_usd: Option<f64>,
}

#[derive(Serialize)]
struct BudgetResponse {
    ok: bool,
    monthly_budget_usd: Option<f64>,
    monthly_actual_spend_usd: Option<f64>,
}

async fn api_set_budget(Json(body): Json<BudgetBody>) -> Json<BudgetResponse> {
    let mut config = crate::config::Config::load();
    if let Some(b) = body.budget_usd { config.monthly_budget_usd = Some(b); }
    if let Some(a) = body.actual_usd {
        config.monthly_actual_spend_usd = Some(a);
        // Snapshot the current session total so the running delta is anchored
        // to this moment rather than resetting on every page load.
        let sessions = crate::conversations::all_sessions();
        let now = chrono::Utc::now();
        let current_month = format!("{}-{:02}", now.year(), now.month());
        let month_total: f64 = sessions.iter()
            .filter(|s| s.started_at.starts_with(&current_month))
            .map(|s| s.total_usd)
            .sum();
        config.monthly_actual_spend_baseline_usd = Some(month_total);
    }
    let ok = config.save().is_ok();
    Json(BudgetResponse {
        ok,
        monthly_budget_usd: config.monthly_budget_usd,
        monthly_actual_spend_usd: config.monthly_actual_spend_usd,
    })
}

#[derive(Serialize)]
struct SettingsRowCounts {
    sessions: i64,
    turns: i64,
    tool_invocations: i64,
    session_embeddings: i64,
    requests: i64,
}

#[derive(Serialize)]
struct ModeListEntry {
    name: String,
    profile: String,
    inject_enabled: bool,
    coaching_enabled: bool,
    adaptive_prefix_enabled: bool,
}

#[derive(Serialize)]
struct SettingsFileEntry {
    name: String,
    size_bytes: u64,
}

#[derive(Serialize)]
struct SettingsGetResponse {
    active_profile: Option<String>,
    proxy_port: Option<u16>,
    dashboard_port: Option<u16>,
    proxy_upstream: Option<String>,
    proxy_mode: String,
    proxy_mitm_wired: bool,
    proxy_install_mode: Option<String>,
    auto_profile_enabled: bool,
    filter_mode: String,
    inject_enabled: bool,
    coaching_enabled: bool,
    adaptive_prefix_enabled: bool,
    adaptive_prefix_max_chars: Option<usize>,
    adaptive_prefix_char_budget: usize,
    adaptive_prefix_preview: String,
    adaptive_prefix_char_count: usize,
    monthly_budget_usd: Option<f64>,
    monthly_actual_spend_usd: Option<f64>,
    monthly_actual_spend_baseline_usd: Option<f64>,
    store_prompt_text: bool,
    embeddings_enabled: bool,
    dev_mode: bool,
    ab_test: crate::config::AbTestConfig,
    active_mode: Option<String>,
    modes: Vec<ModeListEntry>,
    auto_apply_recommendations: bool,
    tuning_recommendations: Option<crate::tuning::AbResultsFile>,
    system_prefix_preview: String,
    ctx_home: String,
    ctx_active_since: Option<String>,
    db_size_bytes: u64,
    row_counts: SettingsRowCounts,
    last_ingest_at: Option<String>,
    files_under_ctx: Vec<SettingsFileEntry>,
}

fn count_table(conn: &rusqlite::Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap_or(0)
}

fn list_ctx_dir_files() -> Vec<SettingsFileEntry> {
    let mut out = Vec::new();
    let dir = crate::config::ctx_dir();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() {
                if let Ok(m) = p.metadata() {
                    out.push(SettingsFileEntry {
                        name: e.file_name().to_string_lossy().into_owned(),
                        size_bytes: m.len(),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

async fn api_settings_get() -> impl IntoResponse {
    let Some(conn) = open_ctx_db() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "db unavailable").into_response();
    };
    let cfg = crate::config::Config::load();
    let db_path = crate::config::db_path();
    let db_size_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    let last_ingest_at: Option<String> = conn
        .query_row("SELECT v FROM meta WHERE k = 'last_ingest_at'", [], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    let prefix_path = crate::config::system_prefix_path();
    let system_prefix_preview = std::fs::read_to_string(&prefix_path)
        .unwrap_or_default()
        .chars()
        .take(4000)
        .collect();
    let adaptive_path = crate::config::adaptive_prefix_path();
    let adaptive_full = std::fs::read_to_string(&adaptive_path).unwrap_or_default();
    let adaptive_prefix_char_count = adaptive_full.chars().count();
    let adaptive_prefix_preview = adaptive_full.chars().take(4000).collect::<String>();
    let adaptive_prefix_char_budget = crate::adaptive::rebuild_max_chars_for_db(&conn);
    let ctx_active_since = crate::db::get_ctx_active_since(&conn);
    let proxy_port = cfg.proxy_port.unwrap_or(8788);
    let proxy_mitm_wired = std::fs::read_to_string(crate::config::claude_settings_path())
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map(|settings| {
            let claude = settings
                .pointer("/env/CLAUDE_CODE_HTTPS_PROXY")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let https = settings
                .pointer("/env/HTTPS_PROXY")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let url = format!("http://127.0.0.1:{proxy_port}");
            claude == url || https == url
        })
        .unwrap_or(false);
    let row_counts = SettingsRowCounts {
        sessions: count_table(&conn, "sessions"),
        turns: count_table(&conn, "turns"),
        tool_invocations: count_table(&conn, "tool_invocations"),
        session_embeddings: count_table(&conn, "session_embeddings"),
        requests: count_table(&conn, "requests"),
    };
    let body = SettingsGetResponse {
        active_profile: cfg.active_profile.clone(),
        proxy_port: cfg.proxy_port,
        dashboard_port: cfg.dashboard_port,
        proxy_upstream: cfg.proxy_upstream.clone(),
        proxy_mode: cfg.proxy_mode.as_str().to_string(),
        proxy_mitm_wired,
        proxy_install_mode: cfg.proxy_install_mode.clone(),
        auto_profile_enabled: cfg.auto_profile_enabled,
        filter_mode: cfg.filter_mode.as_str().to_string(),
        inject_enabled: cfg.inject_enabled,
        coaching_enabled: cfg.coaching_enabled,
        adaptive_prefix_enabled: cfg.adaptive_prefix_enabled,
        adaptive_prefix_max_chars: cfg.adaptive_prefix_max_chars,
        adaptive_prefix_char_budget,
        adaptive_prefix_preview,
        adaptive_prefix_char_count,
        monthly_budget_usd: cfg.monthly_budget_usd,
        monthly_actual_spend_usd: cfg.monthly_actual_spend_usd,
        monthly_actual_spend_baseline_usd: cfg.monthly_actual_spend_baseline_usd,
        store_prompt_text: cfg.store_prompt_text_enabled(),
        embeddings_enabled: cfg.embeddings_enabled(),
        dev_mode: cfg.dev_mode,
        ab_test: cfg.ab_test.clone().unwrap_or_default(),
        active_mode: cfg.active_mode.clone(),
        modes: {
            let mut names: Vec<_> = cfg.modes.keys().cloned().collect();
            names.sort();
            names
                .into_iter()
                .filter_map(|name| {
                    cfg.modes.get(&name).map(|m| ModeListEntry {
                        name,
                        profile: m.profile.clone(),
                        inject_enabled: m.inject_enabled,
                        coaching_enabled: m.coaching_enabled,
                        adaptive_prefix_enabled: m.adaptive_prefix_enabled,
                    })
                })
                .collect()
        },
        auto_apply_recommendations: cfg.auto_apply_recommendations,
        tuning_recommendations: crate::tuning::load_ab_results(),
        system_prefix_preview,
        ctx_home: crate::config::ctx_dir().to_string_lossy().into_owned(),
        ctx_active_since,
        db_size_bytes,
        row_counts,
        last_ingest_at,
        files_under_ctx: list_ctx_dir_files(),
    };
    Json(body).into_response()
}

#[derive(Deserialize)]
struct SettingsPostBody {
    active_profile: Option<String>,
    auto_profile_enabled: Option<bool>,
    inject_enabled: Option<bool>,
    coaching_enabled: Option<bool>,
    adaptive_prefix_enabled: Option<bool>,
    /// Omit or use `0` to clear override and use model-based budget.
    adaptive_prefix_max_chars: Option<usize>,
    monthly_budget_usd: Option<f64>,
    monthly_actual_spend_usd: Option<f64>,
    store_prompt_text: Option<bool>,
    embeddings_enabled: Option<bool>,
    dev_mode: Option<bool>,
    ab_test: Option<crate::config::AbTestConfig>,
    auto_apply_recommendations: Option<bool>,
    system_prefix: Option<String>,
}

#[derive(Deserialize)]
struct SettingsModeBody {
    mode: String,
}

async fn api_settings_post(Json(body): Json<SettingsPostBody>) -> impl IntoResponse {
    let mut cfg = crate::config::Config::load();
    if let Some(v) = body.auto_profile_enabled {
        cfg.auto_profile_enabled = v;
    }
    if let Some(v) = body.inject_enabled {
        cfg.inject_enabled = v;
    }
    if let Some(v) = body.coaching_enabled {
        cfg.coaching_enabled = v;
    }
    if let Some(v) = body.adaptive_prefix_enabled {
        cfg.adaptive_prefix_enabled = v;
    }
    if let Some(v) = body.adaptive_prefix_max_chars {
        cfg.adaptive_prefix_max_chars = if v == 0 { None } else { Some(v) };
    }
    if let Some(v) = body.monthly_budget_usd {
        cfg.monthly_budget_usd = Some(v);
    }
    if let Some(a) = body.monthly_actual_spend_usd {
        cfg.monthly_actual_spend_usd = Some(a);
        let sessions = crate::conversations::all_sessions();
        let now = chrono::Utc::now();
        let current_month = format!("{}-{:02}", now.year(), now.month());
        let month_total: f64 = sessions
            .iter()
            .filter(|s| s.started_at.starts_with(&current_month))
            .map(|s| s.total_usd)
            .sum();
        cfg.monthly_actual_spend_baseline_usd = Some(month_total);
    }
    if let Some(v) = body.store_prompt_text {
        cfg.store_prompt_text = Some(v);
    }
    if let Some(v) = body.embeddings_enabled {
        cfg.embeddings_enabled = Some(v);
    }
    if let Some(v) = body.dev_mode {
        cfg.dev_mode = v;
    }
    if let Some(ab) = body.ab_test {
        cfg.ab_test = Some(ab);
    }
    if let Some(v) = body.auto_apply_recommendations {
        cfg.auto_apply_recommendations = v;
    }
    if let Some(prefix) = &body.system_prefix {
        if let Err(e) = (|| -> anyhow::Result<()> {
            crate::config::ensure_dir()?;
            std::fs::write(crate::config::system_prefix_path(), prefix)?;
            Ok(())
        })() {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    }
    if let Some(slug) = &body.active_profile {
        let slug = slug.trim();
        if !slug.is_empty() {
            if let Err(e) = crate::profiles::switch(slug, true) {
                return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
            }
            cfg = crate::config::Config::load();
        }
    }
    if let Err(e) = cfg.save() {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    let _ = crate::filter_hook::sync_filter_config_from_active_config();
    Json(serde_json::json!({ "ok": true })).into_response()
}

async fn api_settings_refresh_adaptive_prefix() -> impl IntoResponse {
    match crate::adaptive::regenerate_adaptive_prefix_file() {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn api_settings_mode(Json(body): Json<SettingsModeBody>) -> impl IntoResponse {
    match crate::modes::switch_mode(body.mode.trim()) {
        Ok(()) => Json(serde_json::json!({ "ok": true, "mode": body.mode })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn api_settings_reset_watermark() -> impl IntoResponse {
    let res: Result<(), anyhow::Error> = (|| {
        let conn = crate::db::open_db()?;
        crate::db::ensure_schema(&conn)?;
        crate::db::reset_ctx_active_since(&conn)?;
        Ok(())
    })();
    match res {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn api_settings_purge_prompts() -> impl IntoResponse {
    let res: Result<(), anyhow::Error> = (|| {
        let conn = crate::db::open_db()?;
        crate::db::ensure_schema(&conn)?;
        crate::db::purge_prompt_text_columns(&conn)?;
        Ok(())
    })();
    match res {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn api_settings_delete_data() -> impl IntoResponse {
    let res: Result<(), anyhow::Error> = (|| {
        let conn = crate::db::open_db()?;
        crate::db::ensure_schema(&conn)?;
        crate::db::delete_all_indexed_data(&conn)?;
        let _ = conn.execute(
            "INSERT OR REPLACE INTO meta (k, v) VALUES ('last_ingest_at', '')",
            [],
        );
        Ok(())
    })();
    match res {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn api_settings_export() -> impl IntoResponse {
    let path = crate::config::db_path();
    match std::fs::read(&path) {
        Ok(bytes) => {
            let disposition = r#"attachment; filename="ctx.db""#;
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/vnd.sqlite3")
                .header(CONTENT_DISPOSITION, disposition)
                .body(Body::from(bytes))
                .unwrap()
                .into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    }
}

async fn api_user_profile() -> Json<crate::user_profile::UserProfile> {
    Json(crate::user_profile::UserProfile::compute())
}

// ---------------------------------------------------------------------------
// Tab 3 — Profiles
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ProfileInfo {
    slug: String,
    display: String,
    description: String,
    tool_count: usize,
    server_count: usize,
    tokens_per_turn: usize,
    savings_pct: f32,
    active: bool,
    servers_included: Vec<String>,
    servers_excluded: Vec<String>,
    keep_tools: Vec<String>,
    uses_tool_level: bool,
    deny_rule_count: usize,
    metrics_pending: bool,
    origin: String,
    filter_mode: String,
}

#[derive(Serialize)]
struct ObservedToolInfo {
    tool_name: String,
    server: String,
    count: u64,
}

async fn api_profiles_tools() -> Json<Vec<ObservedToolInfo>> {
    let catalog = crate::profiles::observed_tool_catalog();
    Json(
        catalog
            .into_iter()
            .map(|(tool_name, server, count)| ObservedToolInfo {
                tool_name,
                server,
                count,
            })
            .collect(),
    )
}

async fn api_profiles() -> Json<Vec<ProfileInfo>> {
    let config = crate::config::Config::load();
    let active = config.active_profile.as_deref().unwrap_or("all");
    let filter_mode = config.filter_mode.as_str().to_string();
    let expansion = config.session_expansion.clone();
    let profiles = crate::profiles::load_all();
    let custom_slugs = crate::profiles::slugs_from_profiles_toml();

    let metrics_ready = crate::profiles::tool_metrics_ready();
    let mut result: Vec<ProfileInfo> = profiles
        .into_iter()
        .filter(|(slug, _)| crate::profiles::is_profile_visible(slug, active))
        .map(|(slug, p)| {
            let (servers_included, servers_excluded) =
                crate::profiles::profile_server_display_lists(&p);
            let deny_rule_count = if !p.filtering_enabled() {
                0
            } else {
                crate::profiles::deny_patterns_for_profile(&p, &expansion, &[]).len()
            };
            let origin = if custom_slugs.contains(&slug) {
                "custom".to_string()
            } else {
                "builtin".to_string()
            };

            ProfileInfo {
                active: slug == active,
                tool_count: if metrics_ready { p.tool_count() } else { 0 },
                server_count: if metrics_ready { p.server_count() } else { 0 },
                tokens_per_turn: if metrics_ready { p.token_cost() } else { 0 },
                savings_pct: if metrics_ready { p.savings_pct() } else { 0.0 },
                metrics_pending: !metrics_ready,
                servers_included,
                servers_excluded,
                keep_tools: p.keep_tools.clone(),
                uses_tool_level: p.uses_tool_level(),
                deny_rule_count,
                origin,
                filter_mode: filter_mode.clone(),
                slug,
                display: p.display,
                description: p.description,
            }
        })
        .collect();

    result.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then_with(|| {
                crate::profiles::generated_profile_sort_key(&a.slug)
                    .cmp(&crate::profiles::generated_profile_sort_key(&b.slug))
            })
            .then(a.slug.cmp(&b.slug))
    });

    Json(result)
}

#[derive(Deserialize)]
struct SwitchBody {
    slug: String,
    #[serde(default)]
    force: bool,
}

#[derive(Serialize)]
struct SwitchResponse {
    ok: bool,
    active: String,
}

async fn api_profiles_switch(Json(body): Json<SwitchBody>) -> Json<SwitchResponse> {
    let ok = crate::profiles::switch(&body.slug, body.force).is_ok();
    Json(SwitchResponse { ok, active: body.slug })
}

#[derive(Deserialize)]
struct CreateProfileBody {
    name: String,
    servers: Vec<String>,
    #[serde(default)]
    tools: Vec<String>,
}

#[derive(Serialize)]
struct CreateResponse {
    ok: bool,
}

async fn api_profiles_create(Json(body): Json<CreateProfileBody>) -> Json<CreateResponse> {
    let ok = if !body.tools.is_empty() {
        crate::profiles::add(&body.name, vec![], body.tools).is_ok()
    } else {
        crate::profiles::add(&body.name, body.servers, vec![]).is_ok()
    };
    Json(CreateResponse { ok })
}

// ---------------------------------------------------------------------------
// Profile analytics — per-profile request / token breakdown
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ProfileStat {
    slug: String,
    display: String,
    requests: usize,
    tokens_saved: usize,
    cost_saved: f64,
    auto_selected_count: usize,
    pct_of_total: f64,
}

async fn api_profiles_analytics(Query(q): Query<SinceQuery>) -> Json<Vec<ProfileStat>> {
    let records = load_records();
    let conn = open_ctx_db();
    let wm = conn.as_ref().and_then(|c| watermark_ts(c, &q));
    let wm_ref = wm.as_deref();
    let filter_recs: Vec<_> = records
        .iter()
        .filter(|r| r.tools_removed > 0)
        .filter(|r| record_ts_after_watermark(&r.ts, wm_ref))
        .collect();

    let mut by_profile: HashMap<String, (usize, usize, usize)> = HashMap::new(); // (requests, tokens, auto_count)
    if !filter_recs.is_empty() {
        for rec in &filter_recs {
            let slug = if rec.profile.is_empty() {
                "all".to_string()
            } else {
                rec.profile.clone()
            };
            let e = by_profile.entry(slug).or_default();
            e.0 += 1;
            e.1 += rec.tokens_saved;
            if rec.auto_selected {
                e.2 += 1;
            }
        }
    } else if let Some(ref c) = conn {
        by_profile = profiles_analytics_from_hook_traces(c, wm_ref);
    }

    let total: usize = by_profile.values().map(|(r, _, _)| r).sum();

    let profiles = crate::profiles::load_all();
    let mut stats: Vec<ProfileStat> = by_profile.into_iter().map(|(slug, (requests, tokens, auto_count))| {
        let display = profiles.get(&slug).map(|p| p.display.clone()).unwrap_or_else(|| slug.clone());
        let cost_saved = (tokens as f64 / 1_000_000.0) * crate::analytics::CACHE_READ_RATE_PER_MTOK;
        let pct = if total > 0 { requests as f64 / total as f64 * 100.0 } else { 0.0 };
        ProfileStat { slug, display, requests, tokens_saved: tokens, cost_saved, auto_selected_count: auto_count, pct_of_total: pct }
    }).collect();

    stats.sort_by(|a, b| b.requests.cmp(&a.requests));
    Json(stats)
}

// ---------------------------------------------------------------------------
// Request trace — recent per-request records with server breakdown
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct RequestsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    since: Option<String>,
}

#[derive(Serialize)]
struct RequestTrace {
    ts: String,
    profile: String,
    tools_removed: usize,
    tokens_saved: usize,
    cost_saved: f64,
    removed_servers: Vec<String>,
    kept_servers: Vec<String>,
    auto_selected: bool,
    auto_trigger: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    working_directory: String,
    tools_sent_count: usize,
    inject_fired: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    coach_kind: Option<String>,
    budget_fired: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    behavior_kind: Option<String>,
    compress_chars_saved: usize,
    tools_sent_by_server: HashMap<String, usize>,
    mcp_tools_invoked: Vec<String>,
}

async fn api_requests(Query(q): Query<RequestsQuery>) -> Json<Vec<RequestTrace>> {
    let records = load_records();
    let since_q = SinceQuery {
        since: q.since.clone(),
    };
    let conn = open_ctx_db();
    let wm = conn.as_ref().and_then(|c| watermark_ts(c, &since_q));
    let wm_ref = wm.as_deref();
    let limit = q.limit.unwrap_or(50).min(200);
    let offset = q.offset.unwrap_or(0);

    let traces: Vec<RequestTrace> = records
        .into_iter()
        .filter(|r| r.tools_removed > 0)
        .filter(|r| record_ts_after_watermark(&r.ts, wm_ref))
        .rev()
        .skip(offset)
        .take(limit)
        .map(|r| {
            let cost = (r.tokens_saved as f64 / 1_000_000.0) * crate::analytics::CACHE_READ_RATE_PER_MTOK;
            RequestTrace {
                ts: r.ts,
                profile: r.profile,
                tools_removed: r.tools_removed,
                tokens_saved: r.tokens_saved,
                cost_saved: cost,
                removed_servers: r.removed_servers,
                kept_servers: r.kept_servers,
                auto_selected: r.auto_selected,
                auto_trigger: r.auto_trigger,
                working_directory: r.working_directory.clone(),
                tools_sent_count: r.tools_sent_count,
                inject_fired: r.inject_fired,
                coach_kind: r.coach_kind,
                budget_fired: r.budget_fired,
                behavior_kind: r.behavior_kind,
                compress_chars_saved: r.compress_chars_saved,
                tools_sent_by_server: r.tools_sent_by_server,
                mcp_tools_invoked: r.mcp_tools_invoked,
            }
        })
        .collect();

    Json(traces)
}

// ---------------------------------------------------------------------------
// Hook events — companion to request trace for v2 hook architecture
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct HookEventsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    since: Option<String>,
}

async fn api_hook_events(Query(q): Query<HookEventsQuery>) -> Json<Vec<crate::db::HookEventRow>> {
    let limit = q.limit.unwrap_or(100).min(500);
    let offset = q.offset.unwrap_or(0);
    let since_q = SinceQuery {
        since: q.since.clone(),
    };
    let Some(conn) = open_ctx_db() else {
        return Json(vec![]);
    };
    let wm = watermark_ts(&conn, &since_q);
    Json(
        crate::db::load_hook_events(&conn, limit, offset, wm.as_deref()).unwrap_or_default(),
    )
}

// ---------------------------------------------------------------------------
// Hook traces — hybrid rows from hooks, enriched by JSONL ingest
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SimulateBody {
    prompt: Option<String>,
    cwd: Option<String>,
    session_id: Option<String>,
    model: Option<String>,
    profile: Option<String>,
    #[serde(default)]
    all_profiles: bool,
}

#[derive(Serialize)]
struct SimulateResponse {
    result: Option<crate::simulate::SimulateResult>,
    all_profiles: Option<Vec<crate::simulate::SimulateResult>>,
}

async fn api_simulate(Json(body): Json<SimulateBody>) -> impl IntoResponse {
    let cwd = body.cwd.as_deref().unwrap_or(".");
    let prompt = body.prompt.as_deref().unwrap_or("");
    let session_id = body.session_id.as_deref();
    let model = body.model.as_deref();
    let profile = body.profile.as_deref();

    if body.all_profiles {
        match crate::simulate::simulate_all_profiles(cwd, prompt, session_id, model) {
            Ok(results) => Json(SimulateResponse {
                result: None,
                all_profiles: Some(results),
            })
            .into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    } else {
        match crate::simulate::simulate_pipeline(cwd, prompt, session_id, model, profile) {
            Ok(r) => Json(SimulateResponse {
                result: Some(r),
                all_profiles: None,
            })
            .into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    }
}

async fn api_task_costs() -> Json<Vec<crate::db::TaskCostGroup>> {
    let Some(conn) = open_ctx_db() else {
        return Json(vec![]);
    };
    Json(crate::db::load_task_costs(&conn).unwrap_or_default())
}

#[derive(Deserialize, Default)]
struct HookTracesQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    since: Option<String>,
}

async fn api_hook_traces(Query(q): Query<HookTracesQuery>) -> Json<Vec<crate::db::HookTraceRow>> {
    let limit = q.limit.unwrap_or(100).min(500);
    let offset = q.offset.unwrap_or(0);
    let since_q = SinceQuery {
        since: q.since.clone(),
    };
    let Some(conn) = open_ctx_db() else {
        return Json(vec![]);
    };
    let wm = watermark_ts(&conn, &since_q);
    Json(crate::db::load_hook_traces(&conn, limit, offset, wm.as_deref()).unwrap_or_default())
}

// ---------------------------------------------------------------------------
// A/B experiment reports
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AbCohortMetrics {
    count: i64,
    avg_cost_usd: f64,
    avg_input_tokens: f64,
    avg_output_tokens: f64,
    avg_cache_read_tokens: f64,
    avg_tokens_saved: f64,
    avg_tools_removed: f64,
    avg_inject_chars: f64,
    avg_adaptive_chars: f64,
    /// Session-level avg(correction_turns / turn_count) for sessions in this arm.
    correction_rate_pct: f64,
    /// Prompt-level: share of turns where coaching injected a nudge.
    coach_fire_rate_pct: f64,
}

fn ab_session_correction_rate(
    conn: &rusqlite::Connection,
    group_pattern: &str,
    wm: Option<&str>,
) -> f64 {
    let sql = if wm.is_some() {
        "SELECT AVG(CAST(s.correction_turns AS REAL) / MAX(s.turn_count, 1))
         FROM sessions s
         WHERE s.turn_count > 0
           AND EXISTS (
             SELECT 1 FROM hook_traces h
             WHERE h.session_id IS NOT NULL
               AND h.enriched = 1
               AND h.ab_group LIKE ?1
               AND h.ts >= ?2
               AND s.external_key LIKE '%' || h.session_id || '%'
           )"
    } else {
        "SELECT AVG(CAST(s.correction_turns AS REAL) / MAX(s.turn_count, 1))
         FROM sessions s
         WHERE s.turn_count > 0
           AND EXISTS (
             SELECT 1 FROM hook_traces h
             WHERE h.session_id IS NOT NULL
               AND h.enriched = 1
               AND h.ab_group LIKE ?1
               AND s.external_key LIKE '%' || h.session_id || '%'
           )"
    };
    let rate: Option<f64> = if let Some(since) = wm {
        conn.query_row(sql, rusqlite::params![group_pattern, since], |r| r.get(0))
            .unwrap_or(None)
    } else {
        conn.query_row(sql, rusqlite::params![group_pattern], |r| r.get(0))
            .unwrap_or(None)
    };
    rate.unwrap_or(0.0) * 100.0
}

#[derive(Serialize)]
struct AbFeatureReport {
    feature: String,
    treatment: AbCohortMetrics,
    control: AbCohortMetrics,
    cost_delta_pct: Option<f64>,
}

#[derive(Serialize)]
struct AbDailyRow {
    date: String,
    feature: String,
    group: String,
    count: i64,
    avg_cost: f64,
    avg_tokens: f64,
}

fn ab_cohort_metrics(
    conn: &rusqlite::Connection,
    group_pattern: &str,
    wm: Option<&str>,
) -> AbCohortMetrics {
    let mut sql = String::from(
        "SELECT COUNT(*),
                AVG(cost_usd),
                AVG(input_tokens),
                AVG(output_tokens),
                SUM(CASE WHEN coach_kind IS NOT NULL AND coach_kind != '' THEN 1 ELSE 0 END),
                AVG(tokens_saved),
                AVG(tools_removed),
                AVG(cache_read_tokens),
                AVG(inject_chars),
                AVG(adaptive_chars)
         FROM hook_traces
         WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE ?1",
    );
    if wm.is_some() {
        sql.push_str(" AND ts >= ?2");
    }
    let row: (
        i64,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        i64,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    ) = if let Some(since) = wm {
        conn.query_row(&sql, rusqlite::params![group_pattern, since], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
            ))
        })
        .unwrap_or((0, None, None, None, 0, None, None, None, None, None))
    } else {
        conn.query_row(&sql, rusqlite::params![group_pattern], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
            ))
        })
        .unwrap_or((0, None, None, None, 0, None, None, None, None, None))
    };
    let count = row.0;
    let coach_fire_rate_pct = if count > 0 {
        (row.4 as f64 / count as f64) * 100.0
    } else {
        0.0
    };
    let correction_rate_pct = ab_session_correction_rate(conn, group_pattern, wm);
    AbCohortMetrics {
        count,
        avg_cost_usd: row.1.unwrap_or(0.0),
        avg_input_tokens: row.2.unwrap_or(0.0),
        avg_output_tokens: row.3.unwrap_or(0.0),
        avg_cache_read_tokens: row.7.unwrap_or(0.0),
        avg_tokens_saved: row.5.unwrap_or(0.0),
        avg_tools_removed: row.6.unwrap_or(0.0),
        avg_inject_chars: row.8.unwrap_or(0.0),
        avg_adaptive_chars: row.9.unwrap_or(0.0),
        correction_rate_pct,
        coach_fire_rate_pct,
    }
}

fn ab_cost_delta_pct(treatment: &AbCohortMetrics, control: &AbCohortMetrics) -> Option<f64> {
    if control.count == 0 || control.avg_cost_usd <= 0.0 {
        return None;
    }
    Some(((treatment.avg_cost_usd - control.avg_cost_usd) / control.avg_cost_usd) * 100.0)
}

async fn api_ab_report(Query(q): Query<SinceQuery>) -> Json<Vec<AbFeatureReport>> {
    let Some(conn) = open_ctx_db() else {
        return Json(vec![]);
    };
    let wm = watermark_ts(&conn, &q);
    let features = [
        ("profile", "%P:T%", "%P:C%"),
        ("inject", "%I:T%", "%I:C%"),
        ("adaptive", "%A:T%", "%A:C%"),
        ("coaching", "%C:T%", "%C:C%"),
    ];
    let mut out = Vec::new();
    for (name, t_pat, c_pat) in features {
        let treatment = ab_cohort_metrics(&conn, t_pat, wm.as_deref());
        let control = ab_cohort_metrics(&conn, c_pat, wm.as_deref());
        let cost_delta_pct = ab_cost_delta_pct(&treatment, &control);
        out.push(AbFeatureReport {
            feature: name.to_string(),
            treatment,
            control,
            cost_delta_pct,
        });
    }
    Json(out)
}

async fn api_ab_daily(Query(q): Query<SinceQuery>) -> Json<Vec<AbDailyRow>> {
    let Some(conn) = open_ctx_db() else {
        return Json(vec![]);
    };
    let wm = watermark_ts(&conn, &q);
    let base = r#"
        SELECT substr(ts, 1, 10) AS day,
               feature,
               grp,
               COUNT(*),
               AVG(cost_usd),
               AVG(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0))
        FROM (
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'profile' AS feature, 'treatment' AS grp
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%P:T%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'profile', 'control'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%P:C%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'inject', 'treatment'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%I:T%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'inject', 'control'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%I:C%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'adaptive', 'treatment'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%A:T%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'adaptive', 'control'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%A:C%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'coaching', 'treatment'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%C:T%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'coaching', 'control'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%C:C%'
        )
        WHERE 1=1
    "#;
    let sql = if wm.is_some() {
        format!("{base} AND ts >= ?1 GROUP BY day, feature, grp ORDER BY day DESC")
    } else {
        format!("{base} GROUP BY day, feature, grp ORDER BY day DESC")
    };
    let map_row = |r: &rusqlite::Row<'_>| {
        Ok(AbDailyRow {
            date: r.get(0)?,
            feature: r.get(1)?,
            group: r.get(2)?,
            count: r.get(3)?,
            avg_cost: r.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
            avg_tokens: r.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
        })
    };
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare(&sql) {
        if let Some(since) = wm.as_deref() {
            if let Ok(rows) = stmt.query_map([since], map_row) {
                for row in rows.flatten() {
                    out.push(row);
                }
            }
        } else if let Ok(rows) = stmt.query_map([], map_row) {
            for row in rows.flatten() {
                out.push(row);
            }
        }
    }
    Json(out)
}

// ---------------------------------------------------------------------------
// Per-project breakdown (working directory from analytics)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ProjectRow {
    working_directory: String,
    requests: usize,
    tokens_saved: usize,
    cost_saved: f64,
}

async fn api_projects(Query(q): Query<SinceQuery>) -> Json<Vec<ProjectRow>> {
    let records = load_records();
    let conn = open_ctx_db();
    let wm = conn.as_ref().and_then(|c| watermark_ts(c, &q));
    let wm_ref = wm.as_deref();
    if !records.is_empty() {
        let mut m: HashMap<String, (usize, usize)> = HashMap::new();
        for r in records
            .iter()
            .filter(|r| r.tools_removed > 0)
            .filter(|r| record_ts_after_watermark(&r.ts, wm_ref))
        {
            let wd = if r.working_directory.is_empty() {
                "(unknown)".to_string()
            } else {
                r.working_directory.clone()
            };
            let e = m.entry(wd).or_default();
            e.0 += 1;
            e.1 += r.tokens_saved;
        }
        let mut rows: Vec<ProjectRow> = m
            .into_iter()
            .map(|(working_directory, (requests, tokens))| ProjectRow {
                cost_saved: (tokens as f64 / 1_000_000.0) * crate::analytics::CACHE_READ_RATE_PER_MTOK,
                working_directory,
                requests,
                tokens_saved: tokens,
            })
            .collect();
        rows.sort_by(|a, b| b.tokens_saved.cmp(&a.tokens_saved));
        rows.truncate(40);
        return Json(rows);
    }
    if let Some(ref c) = conn {
        let mut rows = projects_from_sessions(c, wm_ref);
        if !rows.is_empty() {
            rows.sort_by(|a, b| b.cost_saved.partial_cmp(&a.cost_saved).unwrap_or(std::cmp::Ordering::Equal));
            rows.truncate(40);
            return Json(rows);
        }
    }
    Json(vec![])
}

// ---------------------------------------------------------------------------
// MCP tool sent vs invoked (approximate)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ServerHeat {
    server: String,
    tools_sent: usize,
    tools_invoked: usize,
}

async fn api_tool_usage(Query(q): Query<SinceQuery>) -> Json<Vec<ServerHeat>> {
    let records = load_records();
    let conn = open_ctx_db();
    let wm = conn.as_ref().and_then(|c| watermark_ts(c, &q));
    let wm_ref = wm.as_deref();
    if !records.is_empty() {
        let mut sent: HashMap<String, usize> = HashMap::new();
        let mut inv: HashMap<String, usize> = HashMap::new();
        for r in records
            .iter()
            .filter(|r| record_ts_after_watermark(&r.ts, wm_ref))
        {
            for (srv, n) in &r.tools_sent_by_server {
                *sent.entry(srv.clone()).or_default() += n;
            }
            for name in &r.mcp_tools_invoked {
                if let Some(s) = crate::filter::server_display_from_tool(name) {
                    *inv.entry(s).or_default() += 1;
                }
            }
        }
        let keys: HashSet<String> = sent.keys().chain(inv.keys()).cloned().collect();
        let mut rows: Vec<ServerHeat> = keys
            .into_iter()
            .map(|server| ServerHeat {
                tools_sent: *sent.get(&server).unwrap_or(&0),
                tools_invoked: *inv.get(&server).unwrap_or(&0),
                server,
            })
            .collect();
        rows.sort_by(|a, b| (b.tools_invoked + b.tools_sent).cmp(&(a.tools_invoked + a.tools_sent)));
        rows.truncate(40);
        return Json(rows);
    }
    if let Some(ref c) = conn {
        let rows = tool_usage_from_invocations(c, wm_ref);
        if !rows.is_empty() {
            return Json(rows);
        }
    }
    Json(vec![])
}

// ---------------------------------------------------------------------------
// Gates pipeline — status and activity for all ctx interception layers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct GateStat {
    id: String,
    name: String,
    enabled: bool,
    detail: String,
    today_count: usize,
    today_tokens: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    impact_kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    impact_primary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    impact_secondary: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    verdict: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    verdict_detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ab_feature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chars_added: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quality_delta: Option<f32>,
}

fn ab_feature_for_gate(id: &str) -> Option<String> {
    match id {
        "filter" => Some("profile".into()),
        "inject" => Some("inject".into()),
        "adaptive" => Some("adaptive".into()),
        "coach" => Some("coaching".into()),
        _ => None,
    }
}

fn make_gate_stat(
    id: &str,
    name: &str,
    enabled: bool,
    detail: impl Into<String>,
    today_count: usize,
    today_tokens: usize,
) -> GateStat {
    GateStat {
        id: id.into(),
        name: name.into(),
        enabled,
        detail: detail.into(),
        today_count,
        today_tokens,
        impact_kind: String::new(),
        impact_primary: String::new(),
        impact_secondary: None,
        verdict: String::new(),
        verdict_detail: String::new(),
        ab_feature: ab_feature_for_gate(id),
        chars_added: None,
        quality_delta: None,
    }
}

fn auto_switch_savings(
    conn: &rusqlite::Connection,
    today: &str,
    wm: Option<&str>,
) -> (usize, usize) {
    let row: (i64, i64) = if let Some(since) = wm {
        conn.query_row(
            "SELECT COALESCE(SUM(tokens_saved), 0), COALESCE(SUM(tools_removed), 0)
             FROM hook_traces
             WHERE substr(ts, 1, 10) = ?1 AND auto_selected = 1 AND ts >= ?2",
            params![today, since],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    } else {
        conn.query_row(
            "SELECT COALESCE(SUM(tokens_saved), 0), COALESCE(SUM(tools_removed), 0)
             FROM hook_traces
             WHERE substr(ts, 1, 10) = ?1 AND auto_selected = 1",
            params![today],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }
    .unwrap_or((0, 0));
    (row.0.max(0) as usize, row.1.max(0) as usize)
}

#[derive(Serialize)]
struct GateEvent {
    id: String,
    label: String,
}

#[derive(Serialize)]
struct GateActivity {
    ts: String,
    gates: Vec<GateEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    working_directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_trigger: Option<String>,
}

#[derive(Serialize)]
struct GatesResponse {
    gates: Vec<GateStat>,
    activity: Vec<GateActivity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sessions_fallback_note: Option<String>,
    #[serde(default)]
    hook_only: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    active_profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    correction_rate_7d: Option<f64>,
    #[serde(default)]
    prompts_today: usize,
    verdict_min_prompts: usize,
}

async fn api_gates(Query(q): Query<SinceQuery>) -> Json<GatesResponse> {
    let records = load_records();
    let config = crate::config::Config::load();

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let conn = open_ctx_db();
    let wm = conn.as_ref().and_then(|c| watermark_ts(c, &q));
    let wm_ref = wm.as_deref();

    if records.is_empty() {
        if let Some(ref c) = conn {
            return Json(gates_when_no_requests(c, &today, &config, wm_ref));
        }
        return Json(GatesResponse {
            gates: vec![],
            activity: vec![],
            sessions_fallback_note: None,
            hook_only: false,
            active_profile: String::new(),
            correction_rate_7d: None,
            prompts_today: 0,
            verdict_min_prompts: VERDICT_MIN_PROMPTS,
        });
    }

    let today_recs: Vec<_> = records
        .iter()
        .filter(|r| r.ts.starts_with(&today))
        .filter(|r| record_ts_after_watermark(&r.ts, wm_ref))
        .collect();

    let filter_count = today_recs.iter().filter(|r| r.tools_removed > 0).count();
    let filter_tokens: usize = today_recs.iter().map(|r| r.tokens_saved).sum();
    let auto_count = today_recs.iter().filter(|r| r.auto_selected).count();
    let inject_count = today_recs.iter().filter(|r| r.inject_fired).count();
    let coach_count = today_recs.iter().filter(|r| r.coach_kind.is_some()).count();
    let behavior_count = today_recs.iter().filter(|r| r.behavior_kind.is_some()).count();
    let budget_count = today_recs.iter().filter(|r| r.budget_fired).count();
    let compress_count = today_recs.iter().filter(|r| r.compress_chars_saved > 0).count();
    let compress_chars: usize = today_recs.iter().map(|r| r.compress_chars_saved).sum();

    let adaptive_today = conn
        .as_ref()
        .map(|c| {
            if let Some(s) = wm_ref {
                c.query_row(
                    "SELECT COUNT(*) FROM hook_traces WHERE substr(ts,1,10)=?1 AND adaptive_fired=1 AND ts >= ?2",
                    params![today, s],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0) as usize
            } else {
                c.query_row(
                    "SELECT COUNT(*) FROM hook_traces WHERE substr(ts,1,10)=?1 AND adaptive_fired=1",
                    params![today],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0) as usize
            }
        })
        .unwrap_or(0);

    let active_profile = config.active_profile.as_deref().unwrap_or("all").to_string();
    let inject_on = config.inject_enabled && crate::config::system_prefix_path().exists();
    let adaptive_on = config.adaptive_prefix_enabled;
    let auto_on = config.auto_profile_enabled;

    let budget_threshold = crate::budget_guard::session_threshold_usd();
    let personal_ready = crate::profiles::personal_ready(&crate::profiles::usage_stats());
    let corr_rate = conn
        .as_ref()
        .and_then(|c| correction_rate_7d(c, wm_ref));
    let coach_rate = conn
        .as_ref()
        .and_then(|c| correction_rate_with_coaching(c, wm_ref));
    let (auto_tokens_saved, auto_tools_removed) = conn
        .as_ref()
        .map(|c| auto_switch_savings(c, &today, wm_ref))
        .unwrap_or((0, 0));

    let hook_totals = conn
        .as_ref()
        .map(|c| hook_trace_gate_totals(c, &today, wm_ref))
        .unwrap_or_default();
    let mut combined_totals = hook_totals;
    merge_request_prefix_totals(&mut combined_totals, &today_recs);
    let inject_count = inject_count.max(combined_totals.inject_count);
    let coach_count = coach_count.max(combined_totals.coach_count);
    let auto_count = auto_count.max(combined_totals.auto_count);
    let budget_count = budget_count.max(combined_totals.budget_count);
    let filter_count = filter_count.max(combined_totals.filter_count);
    let filter_tokens = filter_tokens.max(combined_totals.filter_tokens);
    let adaptive_today = adaptive_today.max(combined_totals.adaptive_count);

    let mut gates = vec![
        make_gate_stat(
            "filter",
            "Profile Filter",
            true,
            format!("{active_profile} profile"),
            filter_count,
            filter_tokens,
        ),
        make_gate_stat(
            "auto",
            "Auto-Profile",
            auto_on,
            if auto_count > 0 {
                format!("switched {auto_count}× today")
            } else {
                "watching cwd".into()
            },
            auto_count,
            0,
        ),
        make_gate_stat(
            "inject",
            "Inject",
            inject_on,
            if inject_on {
                "system_prefix.md"
            } else {
                "no prefix file"
            },
            inject_count,
            0,
        ),
        make_gate_stat(
            "adaptive",
            "Adaptive prefix",
            adaptive_on,
            "Learned from ctx.db session index",
            adaptive_today,
            0,
        ),
        make_gate_stat(
            "coach",
            "Coaching",
            true,
            if coach_count > 0 {
                format!("{coach_count} signals today")
            } else {
                "no signals".to_string()
            },
            coach_count,
            0,
        ),
        make_gate_stat(
            "behavior",
            "Behavior Guard",
            true,
            if behavior_count > 0 {
                format!("{behavior_count} hints fired")
            } else {
                "monitoring history".to_string()
            },
            behavior_count,
            0,
        ),
        make_gate_stat(
            "budget",
            "Budget Guard",
            true,
            if budget_count > 0 {
                "threshold crossed".into()
            } else {
                format!("~${budget_threshold:.0} session threshold (from monthly budget)")
            },
            budget_count,
            0,
        ),
        make_gate_stat(
            "compress",
            "Bash Compress",
            false,
            if compress_chars > 0 {
                fmt_tok(compress_chars / 4) + " tok saved"
            } else {
                "coming soon".into()
            },
            compress_count,
            compress_chars / 4,
        ),
    ];

    enrich_gate_stats(
        &mut gates,
        &active_profile,
        false,
        personal_ready,
        corr_rate,
        coach_rate,
        budget_threshold,
        auto_tokens_saved,
        auto_tools_removed,
        &combined_totals,
    );

    let mut activity: Vec<GateActivity> = records
        .iter()
        .rev()
        .filter(|r| record_ts_after_watermark(&r.ts, wm_ref))
        .filter_map(|r| {
            let mut events: Vec<GateEvent> = Vec::new();
            if r.tools_removed > 0 {
                events.push(GateEvent {
                    id: "filter".into(),
                    label: format!("-{} tools  -{}", r.tools_removed, fmt_tok(r.tokens_saved)),
                });
            }
            if r.auto_selected {
                let trig = r.auto_trigger.as_deref().unwrap_or("matched");
                events.push(GateEvent {
                    id: "auto".into(),
                    label: format!("switched to {}  ({})", r.profile, trig),
                });
            }
            if r.inject_fired {
                events.push(GateEvent {
                    id: "inject".into(),
                    label: "prefix applied".into(),
                });
            }
            if let Some(kind) = &r.coach_kind {
                events.push(GateEvent {
                    id: "coach".into(),
                    label: kind.clone(),
                });
            }
            if let Some(kind) = &r.behavior_kind {
                events.push(GateEvent {
                    id: "behavior".into(),
                    label: kind.clone(),
                });
            }
            if r.budget_fired {
                events.push(GateEvent {
                    id: "budget".into(),
                    label: "cost alert fired".into(),
                });
            }
            if r.compress_chars_saved > 0 {
                events.push(GateEvent {
                    id: "compress".into(),
                    label: format!("-{} chars compressed", r.compress_chars_saved),
                });
            }
            if events.is_empty() {
                return None;
            }
            Some(GateActivity {
                ts: r.ts.clone(),
                gates: events,
                session_id: None,
                working_directory: if r.working_directory.is_empty() {
                    None
                } else {
                    Some(r.working_directory.clone())
                },
                profile: if r.profile.is_empty() {
                    None
                } else {
                    Some(r.profile.clone())
                },
                auto_trigger: r.auto_trigger.clone(),
            })
        })
        .take(40)
        .collect();

    if let Some(ref c) = conn {
        if let Ok(rows) = crate::db::load_hook_traces(c, 50, 0, wm_ref) {
            for h in rows {
                if let Some(a) = gate_activity_from_hook_trace(&h) {
                    activity.push(a);
                }
            }
        }
    }
    activity.sort_by(|a, b| b.ts.cmp(&a.ts));
    activity.truncate(45);

    Json(GatesResponse {
        gates,
        activity,
        sessions_fallback_note: None,
        hook_only: false,
        active_profile,
        correction_rate_7d: corr_rate,
        prompts_today: combined_totals.trace_count.max(today_recs.len()),
        verdict_min_prompts: VERDICT_MIN_PROMPTS,
    })
}

#[derive(Deserialize, Default)]
struct SimilarSessionsQuery {
    /// `sessions.external_key` from the dashboard (same as `session_id` when loaded from DB).
    session_id: String,
    #[serde(default = "default_top_k")]
    top: usize,
}

fn default_top_k() -> usize {
    5
}

#[derive(Serialize)]
struct SimilarSessionOut {
    session_db_id: i64,
    similarity: f32,
    project: String,
    total_usd: f64,
    correction_rate: f64,
    turn_count: usize,
}

async fn api_similar_sessions(Query(q): Query<SimilarSessionsQuery>) -> Json<Vec<SimilarSessionOut>> {
    let mut out = Vec::new();
    let Ok(conn) = crate::db::open_db() else {
        return Json(out);
    };
    let _ = crate::db::ensure_schema(&conn);
    let Some(pk) = crate::embedder::session_pk_for_external(&conn, &q.session_id).unwrap_or(None) else {
        return Json(out);
    };
    let sims = crate::embedder::similar_sessions(&conn, pk, q.top.max(1).min(20)).unwrap_or_default();
    for (sid, sim) in sims {
        let row = conn.query_row(
            "SELECT project, total_usd, correction_turns, turn_count FROM sessions WHERE id = ?1",
            rusqlite::params![sid],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        );
        if let Ok((project, total_usd, ct, tc)) = row {
            let cr = if tc > 0 {
                ct as f64 / tc as f64
            } else {
                0.0
            };
            out.push(SimilarSessionOut {
                session_db_id: sid,
                similarity: sim,
                project,
                total_usd,
                correction_rate: cr,
                turn_count: tc as usize,
            });
        }
    }
    Json(out)
}

// ---------------------------------------------------------------------------
// Profile suggestion via session similarity
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct ProfileSuggestBody {
    dir: String,
    text: String,
}

#[derive(serde::Serialize, Clone)]
struct ProfileSuggestion {
    profile: String,
    confidence: f32,
    based_on: usize,
}

/// Embed the caller's working directory + message snippet, find the top-5 similar past sessions,
/// aggregate which profile they used (weighted by token savings), and persist the result to
/// ~/.ctx/profile-suggestion.json for filter.js to read on the next request.
async fn api_profile_suggest(Json(body): Json<ProfileSuggestBody>) -> Json<ProfileSuggestion> {
    let fallback = ProfileSuggestion { profile: String::new(), confidence: 0.0, based_on: 0 };
    let cfg = crate::config::Config::load();
    let active = cfg.active_profile.as_deref().unwrap_or("all");

    if let Some(m) = crate::profiles::select_by_similarity(&body.dir, &body.text, active) {
        let suggestion = ProfileSuggestion {
            profile: m.slug,
            confidence: m.confidence,
            based_on: m.based_on,
        };
        return Json(suggestion);
    }

    Json(fallback)
}

async fn api_pattern_alerts() -> Json<Vec<crate::conversations::PatternAlert>> {
    let Ok(conn) = crate::db::open_db() else {
        return Json(vec![]);
    };
    let _ = crate::db::ensure_schema(&conn);
    Json(crate::conversations::detect_patterns(&conn))
}

async fn api_quality_alerts() -> Json<Vec<crate::quality_guard::QualityAlert>> {
    Json(crate::quality_guard::quality_alerts().unwrap_or_default())
}

async fn api_profiles_auto() -> Json<serde_json::Value> {
    let res = crate::profiles::auto_generate(true);
    Json(serde_json::json!({
        "ok": res.is_ok(),
        "error": res.err().map(|e| e.to_string()),
    }))
}

async fn api_profiles_generate() -> Json<serde_json::Value> {
    let res = crate::profiles::generate_from_config(true);
    Json(serde_json::json!({
        "ok": res.is_ok(),
        "error": res.err().map(|e| e.to_string()),
    }))
}

async fn api_profiles_readiness() -> Json<serde_json::Value> {
    Json(crate::profiles::personal_readiness_json())
}

#[derive(Serialize)]
struct ProjectHealthRow {
    working_directory: String,
    week: String,
    spend_usd: f64,
    correction_rate: f64,
}

async fn api_project_health(Query(q): Query<SinceQuery>) -> Json<Vec<ProjectHealthRow>> {
    let mut out = Vec::new();
    let Ok(conn) = crate::db::open_db() else {
        return Json(out);
    };
    let _ = crate::db::ensure_schema(&conn);
    let wm = watermark_ts(&conn, &q);

    fn map_health_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectHealthRow> {
        Ok(ProjectHealthRow {
            working_directory: r.get(0)?,
            week: r.get(1)?,
            spend_usd: r.get(2)?,
            correction_rate: r.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
        })
    }

    if let Some(since) = wm.as_deref() {
        let batch: Vec<ProjectHealthRow> = {
            let Ok(mut stmt) = conn.prepare(
                r#"SELECT working_directory,
               strftime('%Y-W%W', started_at) AS wk,
               SUM(total_usd) AS spend,
               AVG(CAST(correction_turns AS REAL) / MAX(turn_count, 1)) AS corr
        FROM sessions
        WHERE started_at != '' AND started_at >= ?1
        GROUP BY working_directory, wk
        ORDER BY wk DESC
        LIMIT 120"#,
            ) else {
                return Json(out);
            };
            let x = match stmt.query_map(params![since], map_health_row) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            };
            x
        };
        out.extend(batch);
    } else {
        let batch: Vec<ProjectHealthRow> = {
            let Ok(mut stmt) = conn.prepare(
                r#"SELECT working_directory,
               strftime('%Y-W%W', started_at) AS wk,
               SUM(total_usd) AS spend,
               AVG(CAST(correction_turns AS REAL) / MAX(turn_count, 1)) AS corr
        FROM sessions
        WHERE started_at != ''
        GROUP BY working_directory, wk
        ORDER BY wk DESC
        LIMIT 120"#,
            ) else {
                return Json(out);
            };
            let x = match stmt.query_map([], map_health_row) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            };
            x
        };
        out.extend(batch);
    }
    Json(out)
}

async fn api_prompt_clusters() -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}
