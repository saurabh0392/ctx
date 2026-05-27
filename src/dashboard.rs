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
    if h.budget_fired {
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

    let adaptive_today: i64 = if let Some(since) = wm {
        conn.query_row(
            "SELECT COUNT(*) FROM hook_traces WHERE substr(ts,1,10)=?1 AND adaptive_fired=1 AND ts >= ?2",
            params![today, since],
            |r| r.get(0),
        )
        .unwrap_or(0)
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM hook_traces WHERE substr(ts,1,10)=?1 AND adaptive_fired=1",
            params![today],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };

    let corr_n = corr_sum.max(0) as usize;
    let compact_n = compact_sum.max(0) as usize;

    let sessions_note = if wm.is_some() {
        format!(
            "No per-request filter events in ctx.db. Post-ctx session data: {sess_today} sessions today, {turn_sum} turns, {corr_n} correction turns, {compact_n} context compacts (from ingest, filtered to after ctx activation)."
        )
    } else {
        "No per-request filter events in ctx.db. Showing all historical sessions (watermark off).".into()
    };

    let gates = vec![
        GateStat {
            id: "filter".into(),
            name: "Profile Filter".into(),
            enabled: true,
            detail: "No MCP strip rows for today (see note below).".into(),
            today_count: 0,
            today_tokens: 0,
        },
        GateStat {
            id: "auto".into(),
            name: "Auto-Profile".into(),
            enabled: auto_on,
            detail: "watching cwd".into(),
            today_count: 0,
            today_tokens: 0,
        },
        GateStat {
            id: "inject".into(),
            name: "Inject".into(),
            enabled: inject_on,
            detail: if inject_on {
                "system_prefix.md"
            } else {
                "no prefix file"
            }
            .into(),
            today_count: 0,
            today_tokens: 0,
        },
        GateStat {
            id: "adaptive".into(),
            name: "Adaptive prefix".into(),
            enabled: adaptive_on,
            detail: "Learned from ctx.db session index".into(),
            today_count: adaptive_today as usize,
            today_tokens: 0,
        },
        GateStat {
            id: "coach".into(),
            name: "Coaching".into(),
            enabled: true,
            detail: if corr_n > 0 {
                format!("{corr_n} correction turns today (session quality)")
            } else {
                "no correction-heavy turns today".into()
            },
            today_count: corr_n,
            today_tokens: 0,
        },
        GateStat {
            id: "behavior".into(),
            name: "Behavior Guard".into(),
            enabled: true,
            detail: "session-derived signals only without request log".into(),
            today_count: 0,
            today_tokens: 0,
        },
        GateStat {
            id: "budget".into(),
            name: "Budget Guard".into(),
            enabled: true,
            detail: format!("~${budget_threshold:.0} session threshold (from monthly budget)"),
            today_count: 0,
            today_tokens: 0,
        },
        GateStat {
            id: "compress".into(),
            name: "Bash Compress".into(),
            enabled: true,
            detail: if compact_n > 0 {
                format!("{compact_n} sessions hit context compact today")
            } else {
                "no compacts today".into()
            },
            today_count: compact_n,
            today_tokens: 0,
        },
    ];

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
        .route("/api/trigger-ingest", post(api_trigger_ingest))
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
        .route("/api/settings/purge-prompts", post(api_settings_purge_prompts))
        .route("/api/settings/delete-data", post(api_settings_delete_data))
        .route("/api/settings/export", get(api_settings_export))
        // Tab 3: profiles
        .route("/api/profiles", get(api_profiles))
        .route("/api/profiles/switch", post(api_profiles_switch))
        .route("/api/profiles/create", post(api_profiles_create))
        .route("/api/profiles/analytics", get(api_profiles_analytics))
        // Request trace
        .route("/api/requests", get(api_requests))
        .route("/api/hook-events", get(api_hook_events))
        .route("/api/hook-traces", get(api_hook_traces))
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
        .route("/api/project-health", get(api_project_health))
        .route("/api/prompt-clusters", get(api_prompt_clusters));

    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    let url = format!("http://{addr}");
    println!("ctx dashboard running at {url}");
    if !no_open {
        let _ = open::that(&url);
    }

    axum::serve(listener, app).await?;
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

    if matches!(hook_type, "Stop" | "SessionEnd") {
        if ingest_running()
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(|| {
                    let _ = crate::conversations::ingest_claude_jsonl();
                })
                .await;
                ingest_running().store(false, Ordering::Release);
            });
        }
    }

    StatusCode::OK
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
    if ingest_running()
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        // Already running — the in-flight ingest will cover the latest turn.
        return StatusCode::ACCEPTED;
    }
    tokio::spawn(async move {
        let _ = tokio::task::spawn_blocking(|| {
            let _ = crate::conversations::ingest_claude_jsonl();
        })
        .await;
        ingest_running().store(false, Ordering::Release);
    });
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
    let total_tokens: usize = filter_recs.iter().map(|r| r.tokens_saved).sum();
    let total_tools: usize = filter_recs.iter().map(|r| r.tools_removed).sum();
    let total_kept: usize = filter_recs.iter().map(|r| r.tools_sent_count).sum();
    let all_tokens = total_tokens;
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
    let month_spend: f64 = spend_sessions
        .iter()
        .filter(|s| s.started_at.starts_with(&current_month))
        .filter(|s| session_after_ctx(&s.started_at, spend_ctx.as_deref()))
        .map(|s| s.total_usd)
        .sum();
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

    let effective_session_count = if sessions.is_empty() {
        spend_sessions
            .iter()
            .filter(|s| s.started_at.starts_with(&current_month))
            .filter(|s| session_after_ctx(&s.started_at, spend_ctx.as_deref()))
            .count()
    } else {
        sessions.len()
    };

    let sessions_fallback = records.is_empty();

    let ctx_active_since = conn.as_ref().and_then(|c| crate::db::get_ctx_active_since(c));
    let dashboard_watermark_filtering =
        use_ctx_watermark(&q) && ctx_active_since.is_some();

    Json(Stats {
        total_tokens_saved: all_tokens,
        total_tools_removed: total_tools,
        total_tools_kept: total_kept,
        cost_saved: (all_tokens as f64 / 1_000_000.0) * crate::analytics::CACHE_READ_RATE_PER_MTOK,
        cost_saved_worst_case: (total_tokens as f64 / 1_000_000.0)
            * crate::analytics::WORST_CASE_INPUT_RATE_PER_MTOK,
        session_count: effective_session_count,
        request_count: filter_recs.len(),
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
    proxy_install_mode: Option<String>,
    auto_profile_enabled: bool,
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
        proxy_install_mode: cfg.proxy_install_mode.clone(),
        auto_profile_enabled: cfg.auto_profile_enabled,
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
    system_prefix: Option<String>,
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
    tokens_per_turn: usize,
    savings_pct: f32,
    active: bool,
    servers_included: Vec<String>,
    servers_excluded: Vec<String>,
}

async fn api_profiles() -> Json<Vec<ProfileInfo>> {
    let config = crate::config::Config::load();
    let active = config.active_profile.as_deref().unwrap_or("all");
    let profiles = crate::profiles::load_all();

    // All known server display names
    let all_servers: Vec<&str> = crate::profiles::SERVER_COUNTS
        .iter()
        .map(|(k, _)| *k)
        .collect();

    let mut result: Vec<ProfileInfo> = profiles.into_iter().map(|(slug, p)| {
        let servers_included: Vec<String> = if p.keep.is_empty() {
            all_servers.iter().map(|s| server_display(s)).collect()
        } else {
            p.keep.iter().map(|k| server_display(k)).collect()
        };

        let servers_excluded: Vec<String> = if p.keep.is_empty() {
            vec![]
        } else {
            all_servers.iter()
                .filter(|s| !p.keep.iter().any(|k| s.starts_with(k.as_str()) || k.starts_with(*s)))
                .map(|s| server_display(s))
                .collect()
        };

        ProfileInfo {
            active: slug == active,
            tool_count: p.tool_count(),
            tokens_per_turn: p.token_cost(),
            savings_pct: p.savings_pct(),
            servers_included,
            servers_excluded,
            slug,
            display: p.display,
            description: p.description,
        }
    }).collect();

    // Sort: built-in first in a fixed order, then custom alphabetically
    let order = ["carrier", "data", "design", "minimal", "all"];
    result.sort_by(|a, b| {
        let ai = order.iter().position(|&s| s == a.slug).unwrap_or(99);
        let bi = order.iter().position(|&s| s == b.slug).unwrap_or(99);
        ai.cmp(&bi).then(a.slug.cmp(&b.slug))
    });

    Json(result)
}

fn server_display(prefix: &str) -> String {
    prefix
        .trim_start_matches("mcp__claude_ai_")
        .trim_end_matches("__")
        .replace("_", " ")
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
}

#[derive(Serialize)]
struct CreateResponse {
    ok: bool,
}

async fn api_profiles_create(Json(body): Json<CreateProfileBody>) -> Json<CreateResponse> {
    let ok = crate::profiles::add(&body.name, body.servers).is_ok();
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
    let total = filter_recs.len();

    let mut by_profile: HashMap<String, (usize, usize, usize)> = HashMap::new(); // (requests, tokens, auto_count)
    for rec in &filter_recs {
        let slug = if rec.profile.is_empty() { "all".to_string() } else { rec.profile.clone() };
        let e = by_profile.entry(slug).or_default();
        e.0 += 1;
        e.1 += rec.tokens_saved;
        if rec.auto_selected { e.2 += 1; }
    }

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

    let gates = vec![
        GateStat {
            id: "filter".into(),
            name: "Profile Filter".into(),
            enabled: true,
            detail: format!("{active_profile} profile"),
            today_count: filter_count,
            today_tokens: filter_tokens,
        },
        GateStat {
            id: "auto".into(),
            name: "Auto-Profile".into(),
            enabled: auto_on,
            detail: if auto_count > 0 {
                format!("switched {auto_count}x today")
            } else {
                "watching cwd".into()
            },
            today_count: auto_count,
            today_tokens: 0,
        },
        GateStat {
            id: "inject".into(),
            name: "Inject".into(),
            enabled: inject_on,
            detail: if inject_on {
                "system_prefix.md"
            } else {
                "no prefix file"
            }
            .into(),
            today_count: inject_count,
            today_tokens: 0,
        },
        GateStat {
            id: "adaptive".into(),
            name: "Adaptive prefix".into(),
            enabled: adaptive_on,
            detail: "Learned from ctx.db session index".into(),
            today_count: adaptive_today,
            today_tokens: 0,
        },
        GateStat {
            id: "coach".into(),
            name: "Coaching".into(),
            enabled: true,
            detail: if coach_count > 0 {
                format!("{coach_count} signals today")
            } else {
                "no signals".to_string()
            }
            .into(),
            today_count: coach_count,
            today_tokens: 0,
        },
        GateStat {
            id: "behavior".into(),
            name: "Behavior Guard".into(),
            enabled: true,
            detail: if behavior_count > 0 {
                format!("{behavior_count} hints fired")
            } else {
                "monitoring history".to_string()
            }
            .into(),
            today_count: behavior_count,
            today_tokens: 0,
        },
        GateStat {
            id: "budget".into(),
            name: "Budget Guard".into(),
            enabled: true,
            detail: if budget_count > 0 {
                "threshold crossed".into()
            } else {
                format!("~${budget_threshold:.0} session threshold (from monthly budget)")
            },
            today_count: budget_count,
            today_tokens: 0,
        },
        GateStat {
            id: "compress".into(),
            name: "Bash Compress".into(),
            enabled: true,
            detail: if compress_chars > 0 {
                fmt_tok(compress_chars / 4) + " tok saved"
            } else {
                "hook ready".into()
            },
            today_count: compress_count,
            today_tokens: compress_chars / 4,
        },
    ];

    let activity: Vec<GateActivity> = records
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

    Json(GatesResponse {
        gates,
        activity,
        sessions_fallback_note: None,
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
    let Ok(conn) = crate::db::open_db() else { return Json(fallback) };
    let _ = crate::db::ensure_schema(&conn);

    let query_text = format!("[dir: {}] {}", body.dir.trim(), body.text.trim());
    let Ok(embedding) = crate::embedder::embed_text(&query_text) else { return Json(fallback) };

    let sims = crate::embedder::similar_sessions_by_query(&conn, &embedding, 5, None)
        .unwrap_or_default();
    if sims.is_empty() { return Json(fallback); }

    // Aggregate: for each similar session, weight its profile vote by similarity * tokens_saved.
    // Skip sessions on "all" — they give no filtering signal.
    let mut scores: std::collections::HashMap<String, (f32, usize)> = std::collections::HashMap::new();
    let mut total_weight = 0f32;
    for (sid, sim) in &sims {
        if let Ok((profile, tokens_saved)) = conn.query_row(
            "SELECT COALESCE(profile,''), tokens_saved FROM sessions WHERE id = ?1",
            rusqlite::params![sid],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        ) {
            if profile.is_empty() || profile == "all" { continue; }
            let w = sim * (tokens_saved.max(0) as f32 + 1.0); // +1 so zero-saving sessions still vote
            let entry = scores.entry(profile).or_insert((0.0, 0));
            entry.0 += w;
            entry.1 += 1;
            total_weight += w;
        }
    }

    let Some((best_profile, (best_score, based_on))) = scores
        .into_iter()
        .max_by(|a, b| a.1.0.partial_cmp(&b.1.0).unwrap_or(std::cmp::Ordering::Equal))
    else { return Json(fallback) };

    // Require at least 2 agreeing sessions to avoid single-session noise.
    if based_on < 2 { return Json(fallback); }

    let confidence = if total_weight > 0.0 { (best_score / total_weight).min(1.0) } else { 0.0 };
    let suggestion = ProfileSuggestion { profile: best_profile, confidence, based_on };

    // Persist for filter.js
    let p = crate::config::ctx_dir().join("profile-suggestion.json");
    let _ = std::fs::write(&p, serde_json::to_string(&suggestion).unwrap_or_default());

    Json(suggestion)
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
