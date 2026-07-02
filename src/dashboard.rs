use axum::{
    body::Body,
    extract::Query,
    http::header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    http::StatusCode,
    response::IntoResponse,
    response::Response,
    routing::{get, post},
    Json, Router,
};
use chrono::Datelike;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
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
            let _ = crate::conversations::ingest_claude_jsonl(false);
        })
        .await;
        ingest_running().store(false, Ordering::Release);
        crate::dashboard_push::notify(crate::dashboard_push::DashboardEvent {
            kind: "ingest_complete".into(),
            hook_type,
        });
    });
}

const HTML: &str = include_str!("dashboard.html");

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

fn open_ctx_db() -> Option<rusqlite::Connection> {
    let c = crate::db::open_db().ok()?;
    crate::db::ensure_schema(&c).ok()?;
    Some(c)
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
            let _ = crate::conversations::ingest_claude_jsonl(false);
        })
        .await;
    });

    let app = Router::new()
        .route("/", get(serve_html))
        // Runtime ingest: the filter.js NODE_OPTIONS shim, Claude Code hooks, and the statusline post here.
        .route("/api/ingest-request", post(api_ingest_request))
        .route("/api/hook/event", post(api_hook_event))
        .route("/api/trigger-ingest", post(api_trigger_ingest))
        .route("/api/allowance/snapshot", post(api_allowance_snapshot))
        .route("/api/allowance/current", get(api_allowance_current))
        .route("/api/allowance/burn-rate", get(api_allowance_burn_rate))
        .route(
            "/api/events/stream",
            get(crate::dashboard_push::api_events_stream),
        )
        .route(
            "/api/dashboard/push",
            post(crate::dashboard_push::api_dashboard_push),
        )
        .route("/api/profile-suggest", post(api_profile_suggest))
        // Dashboard Home + Tools.
        .route("/api/context", get(api_context))
        .route("/api/context/preset", post(api_context_preset))
        .route("/api/context/proof", get(api_context_proof))
        .route("/api/context/bill", get(api_context_bill))
        .route("/api/context/tool-bill", get(api_context_tool_bill))
        .route("/api/context/rewind", post(api_context_rewind))
        .route(
            "/api/context/model-progress",
            get(api_context_model_progress),
        )
        .route("/api/context/trial", post(api_context_trial))
        // Dashboard Settings.
        .route(
            "/api/settings",
            get(api_settings_get).post(api_settings_post),
        )
        .route(
            "/api/settings/purge-prompts",
            post(api_settings_purge_prompts),
        )
        .route("/api/settings/delete-data", post(api_settings_delete_data))
        .route("/api/settings/export", get(api_settings_export))
        .route("/api/profiles", get(api_profiles))
        .route("/api/profiles/switch", post(api_profiles_switch))
        // A/B experiment report (experiment runtime + ab_api test).
        .route("/api/ab-report", get(api_ab_report))
        .route("/api/ab-daily", get(api_ab_daily));

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
        let _ = crate::semantic_tools::process_stop_hook_recovery(&payload);
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

// ---------------------------------------------------------------------------
// Context home (Learning / Earning / Improving) — the self-learning controller
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ContextToolView {
    tool: String,
    decisions: i64,
    joined: i64,
    clean_runs: i64,
    corrections: i64,
    rereads: i64,
    active: bool,
    earned: bool,
    need: i64,
}

#[derive(Serialize)]
struct ContextModelView {
    version: u32,
    trained_at: String,
    /// What the model predicts ("needed_whole" as of increment 3), so the UI never mislabels the
    /// AUC as a correction model.
    target: String,
    holdout_auc: f64,
    /// Holdout AUC of the kind-only twin, and whether file-awareness beat it. Lets the UI show the
    /// model only earns the right to steer once knowing the file actually helps.
    kind_only_auc: f64,
    file_aware_wins: bool,
    holdout_accuracy: f64,
    base_correction_rate: f64,
    base_need_rate: f64,
    /// Repos where the model has enough of their own labels to propose, newest math straight from
    /// the trainer so the dashboard and `ctx context learn` agree.
    repos_ready: usize,
    repos_seen: usize,
    history: Vec<serde_json::Value>,
}

/// One tool's place on the watching -> learning -> earned path, for the loop-health view
/// (CTX-26 / ADR 0017). Flattens the raw causal counts and the shared `tool_stage` verdict so the
/// view never re-derives "earned" on its own.
#[derive(Serialize)]
struct LoopHealthToolView {
    #[serde(flatten)]
    outcome: crate::db::CausalToolOutcome,
    #[serde(flatten)]
    stage: crate::compress::activation::ToolStage,
}

/// Honest accrual picture for the loop-health view (CTX-26 / ADR 0017): how much signal exists,
/// how much of it joined to an outcome, the per-day arrival, and where each tool stands. No gate
/// math here; it reads the same thresholds the live gate uses.
#[derive(Serialize)]
struct LoopHealthView {
    total: i64,
    joined: i64,
    /// `joined / total`, or `None` when there are no decisions yet (so the UI says "not yet"
    /// instead of showing 0%).
    joined_pct: Option<f64>,
    today: i64,
    /// Whether autopilot burn-in is on. When off, tools shown as "learning" are eligible to trim
    /// but will not start on their own, so the view can say so plainly.
    autopilot: bool,
    min_baseline: i64,
    min_trimmed: i64,
    by_day: Vec<crate::db::DecisionsByDay>,
    tools: Vec<LoopHealthToolView>,
}

#[derive(Serialize)]
struct ContextView {
    preset: String,
    shadow_enabled: bool,
    activation_min: i64,
    stats: crate::db::CompressDecisionStats,
    tools: Vec<ContextToolView>,
    feed: Vec<crate::db::CompressDecisionFeedRow>,
    model: Option<ContextModelView>,
    /// Per-surface count of corrections that followed a native compaction within a window
    /// (ADR 0016 / CTX-25). Always present, with `unknown` confidence for surfaces ctx
    /// cannot yet see. No causal claim.
    compaction: Vec<crate::db::CompactionFollowups>,
    /// Accrual and per-tool stage for the loop-health view (ADR 0017 / CTX-26).
    loop_health: LoopHealthView,
    /// Per-agent activity for the cross-surface view (ADR 0018 / CTX-34). Always Claude Code and
    /// Cursor, each with a `seen` flag so the UI can show an honest empty state.
    surfaces: Vec<crate::db::SurfaceSummary>,
    /// Per-tool aggregate "suspected trim cost" (CTX-54). Replaces the always-zero gate-corrections
    /// headline: how often an applied trim coincided with the agent needing the dropped content
    /// back. Aggregate suspicion, never single-case proof.
    attribution: Vec<crate::db::ToolAttribution>,
    /// Per-week net-ahead scoreboard (CTX-63): the north-star metric made real. Each week's reclaimed
    /// vs eligible and harm-vs-baseline, with a fail-closed net-ahead verdict.
    wnad: Vec<crate::db::WeekNetAhead>,
    /// Insight-actions (CTX-63 / L4): behavior changes ctx can see, recoveries and MCP prunes. The
    /// education-engagement KPI, counted only from locally logged actions.
    insight_actions: crate::db::InsightActions,
}

/// POST /api/context/rewind: return the verbatim original ctx trimmed, by rewind id (CTX-57).
async fn api_context_rewind(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    let id = body.get("id").and_then(|v| v.as_str()).unwrap_or("");
    match open_ctx_db().and_then(|c| crate::db::get_rewind(&c, id)) {
        Some(e) => Json(serde_json::json!({
            "id": e.id,
            "source": e.command_or_path,
            "chars": e.chars,
            "original": e.original,
            "trimmed": e.trimmed,
        })),
        None => Json(serde_json::json!({ "error": "not found" })),
    }
}

/// GET /api/context/bill: where your context goes, per tool. The education front door, ranked
/// output sinks with reclaimable and reclaimed room. Populates from the first session, no labels.
async fn api_context_bill() -> Json<crate::db::ContextBill> {
    match open_ctx_db() {
        Some(conn) => Json(crate::db::context_bill(&conn)),
        None => Json(crate::db::ContextBill::default()),
    }
}

/// GET /api/context/tool-bill: the input-side Context Bill (CTX-63 / M-A). Per connected MCP server,
/// the full catalog carried on every request versus what was actually invoked, ranked by dead
/// weight. The fixed-cost, input-tax mirror of `/api/context/bill`.
async fn api_context_tool_bill() -> Json<crate::db::ToolMenuBill> {
    let lookback = crate::config::Config::load()
        .profile_thresholds
        .lookback_days;
    match open_ctx_db() {
        Some(conn) => Json(crate::db::tool_menu_bill(&conn, lookback)),
        None => Json(crate::db::ToolMenuBill::default()),
    }
}

/// GET /api/context — everything the Context home needs, drawn only from real data.
async fn api_context() -> Json<ContextView> {
    use crate::compress::activation::{causal_clears_bar, tool_stage, CausalThresholds};
    let cfg = crate::config::Config::load();
    let th = CausalThresholds::default();
    let (stats, tools, feed, compaction, loop_health, surfaces, attribution, wnad, insight_actions) = match open_ctx_db() {
        Some(conn) => {
            let stats = crate::db::compress_decision_stats(&conn);
            let progress = crate::db::compress_tool_progress(&conn);
            let causal = crate::db::causal_tool_outcomes(&conn, None);
            let tools = progress
                .into_iter()
                .map(|p| {
                    // Earned means causal: trimming is not measurably worse than leaving the
                    // tool alone. Fails closed until a trial collects the trimmed arm, so the
                    // badge never claims "ready" on baseline volume alone.
                    let earned = causal
                        .iter()
                        .find(|o| o.tool_name == p.tool_name)
                        .map(|o| causal_clears_bar(o, &th))
                        .unwrap_or(false);
                    ContextToolView {
                        tool: p.tool_name,
                        decisions: p.decisions,
                        joined: p.joined,
                        clean_runs: p.clean_runs,
                        corrections: p.corrections,
                        rereads: p.rereads,
                        active: p.active,
                        earned,
                        need: th.min_baseline,
                    }
                })
                .collect();
            let feed = crate::db::compress_decision_feed(&conn, 12);
            let compaction = crate::db::compaction_followups(&conn);
            let by_day = crate::db::decisions_by_day(&conn, 14);
            // Only place tools that have actually started accruing trim-eligible evidence on the
            // path. A tool with zero baseline and zero trimmed runs has nothing to show yet, and a
            // wall of identical "0 of 30" rows buries the tools that are really moving. They count
            // toward the totals above; they just aren't on the path until there is something to
            // measure.
            let lh_tools = causal
                .iter()
                .filter(|o| o.baseline_n > 0 || o.trimmed_n > 0)
                .map(|o| LoopHealthToolView {
                    stage: tool_stage(o, &th),
                    outcome: o.clone(),
                })
                .collect();
            let joined_pct = if stats.total > 0 {
                Some(stats.joined as f64 / stats.total as f64)
            } else {
                None
            };
            let loop_health = LoopHealthView {
                total: stats.total,
                joined: stats.joined,
                joined_pct,
                today: stats.today,
                autopilot: cfg.compress_auto_trial,
                min_baseline: th.min_baseline,
                min_trimmed: th.min_trimmed,
                by_day,
                tools: lh_tools,
            };
            let home = dirs::home_dir().unwrap_or_default();
            let surfaces = crate::db::surface_summary_full(&conn, &home);
            let attribution = crate::db::tool_attribution(&conn);
            let wnad = crate::db::weekly_net_ahead(&conn);
            let insight_actions = crate::db::insight_actions(&conn);
            (stats, tools, feed, compaction, loop_health, surfaces, attribution, wnad, insight_actions)
        }
        None => (
            Default::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            LoopHealthView {
                total: 0,
                joined: 0,
                joined_pct: None,
                today: 0,
                autopilot: cfg.compress_auto_trial,
                min_baseline: th.min_baseline,
                min_trimmed: th.min_trimmed,
                by_day: Vec::new(),
                tools: Vec::new(),
            },
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Default::default(),
        ),
    };

    let model = crate::learn::load_model().map(|m| {
        let history = read_model_history(40);
        ContextModelView {
            version: m.version,
            trained_at: m.trained_at,
            target: m.target,
            holdout_auc: m.holdout_auc,
            kind_only_auc: m.kind_only_auc,
            file_aware_wins: m.file_aware_wins,
            holdout_accuracy: m.holdout_accuracy,
            base_correction_rate: m.base_correction_rate,
            base_need_rate: m.base_need_rate,
            repos_ready: m.per_repo.iter().filter(|r| r.ready).count(),
            repos_seen: m.per_repo.len(),
            history,
        }
    });

    Json(ContextView {
        preset: cfg.compress_preset.as_str().to_string(),
        shadow_enabled: cfg.compress_shadow_enabled,
        activation_min: th.min_baseline,
        stats,
        tools,
        feed,
        model,
        compaction,
        loop_health,
        surfaces,
        attribution,
        wnad,
        insight_actions,
    })
}

/// Read the last `limit` model-version history lines (Improving view), newest first.
fn read_model_history(limit: usize) -> Vec<serde_json::Value> {
    let path = crate::config::retention_model_history_path();
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut rows: Vec<serde_json::Value> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    rows.reverse();
    rows.truncate(limit);
    rows
}

/// Phase 2 readiness for the per-decision model (ADR 0007/0008, CTX-17). Combines two honest
/// signals: the data accruing (how many live decisions the model has scored and how many are judged)
/// and whether the model can yet do anything different from, and no worse than, the heuristic on the
/// user's own label history. The frontend renders these as a "road to Phase 2" gate checklist; the
/// numbers here are reused straight from the benchmark so the dashboard and `ctx bench run` agree.
#[derive(serde::Serialize)]
struct ModelProgressView {
    trained: bool,
    model_version: u32,
    scored_total: i64,
    scored_joined: i64,
    distinct_repos: i64,
    /// How many judged live decisions we want before a live per-decision proof can mean anything.
    min_judged: i64,
    heuristic_n: usize,
    heuristic_correction: Option<f64>,
    heuristic_reread: Option<f64>,
    learned_n: usize,
    learned_correction: Option<f64>,
    learned_reread: Option<f64>,
    /// The model would act on a different set than the rules (it declines or adds some). Today this
    /// is false: it mirrors the heuristic, which is the honest current blocker.
    differs_from_rules: bool,
    /// Where it differs, its correction and re-read rates are no worse than the heuristic's.
    no_worse_where_differs: bool,
    // --- Phase 2 randomized exploration (ADR 0009) ---
    /// Fraction of trim-eligible decisions deliberately kept as control samples.
    explore_rate: f64,
    explore_active: bool,
    /// Every explored row (control + treatment), judged or not.
    explore_collected_total: i64,
    /// Explored rows that have an outcome attached.
    explore_judged_total: i64,
    /// Per-arm samples per arm before a tool's leaning is shown.
    explore_min_arm: i64,
    explore_tools: Vec<ExploreToolView>,
}

/// One tool's randomized control-vs-treatment outcome, as the Phase 2 view renders it.
#[derive(serde::Serialize)]
struct ExploreToolView {
    tool: String,
    control_collected: i64,
    treatment_collected: i64,
    control_n: i64,
    treatment_n: i64,
    control_correction: Option<f64>,
    treatment_correction: Option<f64>,
    control_reread: Option<f64>,
    treatment_reread: Option<f64>,
    /// treatment - control, the effect of trimming. Negative or zero means trimming did not cost
    /// more re-reads / corrections. Only set once both arms have enough judged samples.
    correction_delta: Option<f64>,
    reread_delta: Option<f64>,
    /// "collecting" until both arms clear `explore_min_arm`, then "safe" or "costly" as a leaning.
    verdict: String,
}

async fn api_context_model_progress() -> Json<ModelProgressView> {
    let report = crate::bench::run_report();
    let arm = |name: &str| report.arms.iter().find(|a| a.arm == name);
    let h = arm("ctx-heuristic");
    let l = arm("ctx-learned");

    let cfg = crate::config::Config::load();
    let explore_rate = cfg.compress_explore_rate;
    let explore_min_arm: i64 = 20;

    let (scored_total, scored_joined, distinct_repos, explore_tools) = match open_ctx_db() {
        Some(conn) => {
            let p = crate::db::model_shadow_progress(&conn);
            let rate = |num: i64, den: i64| (den > 0).then(|| num as f64 / den as f64);
            let tools = crate::db::explore_tool_outcomes(&conn, None)
                .into_iter()
                .map(|e| {
                    let cc = rate(e.control_corrections, e.control_n);
                    let tc = rate(e.treatment_corrections, e.treatment_n);
                    let cr = rate(e.control_rereads, e.control_n);
                    let tr = rate(e.treatment_rereads, e.treatment_n);
                    let enough = e.control_n >= explore_min_arm && e.treatment_n >= explore_min_arm;
                    let correction_delta = match (tc, cc) {
                        (Some(t), Some(c)) if enough => Some(t - c),
                        _ => None,
                    };
                    let reread_delta = match (tr, cr) {
                        (Some(t), Some(c)) if enough => Some(t - c),
                        _ => None,
                    };
                    let verdict = match (correction_delta, reread_delta) {
                        (Some(cd), Some(rd)) if cd <= 1e-9 && rd <= 1e-9 => "safe",
                        (Some(_), Some(_)) => "costly",
                        _ => "collecting",
                    }
                    .to_string();
                    ExploreToolView {
                        tool: e.tool_name,
                        control_collected: e.control_collected,
                        treatment_collected: e.treatment_collected,
                        control_n: e.control_n,
                        treatment_n: e.treatment_n,
                        control_correction: cc,
                        treatment_correction: tc,
                        control_reread: cr,
                        treatment_reread: tr,
                        correction_delta,
                        reread_delta,
                        verdict,
                    }
                })
                .collect::<Vec<_>>();
            (p.scored_total, p.scored_joined, p.distinct_repos, tools)
        }
        None => (0, 0, 0, Vec::new()),
    };

    let explore_collected_total: i64 = explore_tools
        .iter()
        .map(|t| t.control_collected + t.treatment_collected)
        .sum();
    let explore_judged_total: i64 = explore_tools
        .iter()
        .map(|t| t.control_n + t.treatment_n)
        .sum();

    let model_version = crate::learn::load_model().map(|m| m.version).unwrap_or(0);
    let trained = model_version > 0;

    let heuristic_n = h.map(|a| a.n_acted).unwrap_or(0);
    let learned_n = l.map(|a| a.n_acted).unwrap_or(0);
    let heuristic_correction = h.and_then(|a| a.correction_rate);
    let heuristic_reread = h.and_then(|a| a.reread_rate);
    let learned_correction = l.and_then(|a| a.correction_rate);
    let learned_reread = l.and_then(|a| a.reread_rate);

    // The model "differs" only when it acts on a different set than the rules. Equal n with the
    // heuristic means it trims the same decisions, so there is nothing new to prove yet.
    let differs_from_rules = trained && learned_n != heuristic_n && learned_n > 0;
    let no_worse_where_differs = differs_from_rules
        && match (
            learned_correction,
            heuristic_correction,
            learned_reread,
            heuristic_reread,
        ) {
            (Some(lc), Some(hc), Some(lr), Some(hr)) => lc <= hc + 1e-9 && lr <= hr + 1e-9,
            _ => false,
        };

    Json(ModelProgressView {
        trained,
        model_version,
        scored_total,
        scored_joined,
        distinct_repos,
        min_judged: 60,
        heuristic_n,
        heuristic_correction,
        heuristic_reread,
        learned_n,
        learned_correction,
        learned_reread,
        differs_from_rules,
        no_worse_where_differs,
        explore_rate,
        explore_active: explore_rate > 0.0 || cfg.compress_explore_read_rate > 0.0,
        explore_collected_total,
        explore_judged_total,
        explore_min_arm,
        explore_tools,
    })
}

#[derive(serde::Deserialize)]
struct ContextPresetBody {
    preset: String,
}

/// POST /api/context/preset — set off | safe | full from the dashboard.
async fn api_context_preset(Json(body): Json<ContextPresetBody>) -> impl IntoResponse {
    match crate::context_ctl::set_preset(&body.preset) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Proof — the causal before/after (SAU-150 / CTX-2), the USP made visible.
// All rates and intervals are computed here so the page, the CLI, and the live
// activation gate share one definition of "earned" (ADR 0002).
// ---------------------------------------------------------------------------

/// A proportion with its 95% Wilson interval. Rates are fractions in [0, 1].
#[derive(Serialize)]
struct ProofMetric {
    rate: f64,
    lo: f64,
    hi: f64,
}

/// A trimmed-minus-baseline difference with its 95% Newcombe interval and a per-metric verdict.
#[derive(Serialize)]
struct ProofDelta {
    diff: f64,
    lo: f64,
    hi: f64,
    verdict: String,
}

#[derive(Serialize)]
struct ProofToolView {
    tool: String,
    trialing: bool,
    baseline_n: i64,
    trimmed_n: i64,
    /// Present when baseline_n > 0.
    baseline_corrections: Option<ProofMetric>,
    baseline_rereads: Option<ProofMetric>,
    /// Present when trimmed_n > 0.
    trimmed_corrections: Option<ProofMetric>,
    trimmed_rereads: Option<ProofMetric>,
    correction_delta: Option<ProofDelta>,
    reread_delta: Option<ProofDelta>,
    /// not_tested | collecting | safe | harmful | unclear.
    verdict: String,
    /// Characters ctx actually removed from this tool's output (applied trims only).
    applied_chars_saved: i64,
    /// Applied trims collected so far, joined or not. `trimmed_n` counts only scored trims, so a
    /// fresh trial reads as "0 trimmed" while trims are visibly happening; this matches what the
    /// user just watched (CTX-62).
    trimmed_collected: i64,
    /// True when this tool is judged by re-edit rather than re-read, for honest UI labels.
    is_edit_tool: bool,
}

#[derive(Serialize)]
struct ProofView {
    preset: String,
    trial_tools: Vec<String>,
    min_baseline: i64,
    min_trimmed: i64,
    tools: Vec<ProofToolView>,
    /// Characters removed by tools that earned the causal gate (verdict "safe"). This is the only
    /// figure shown as real savings, because it is the trimming we have proof is safe.
    safe_chars_saved: i64,
    /// Characters removed by tools that have not earned yet (trials and unproven trims). Shown as
    /// "trimmed while testing", never as money, so we never bank savings we cannot vouch for.
    trial_chars_saved: i64,
    /// Aggregate trimmed-arm outcomes across all tools, for the honest home headline. Raw counts so
    /// the UI can state the real correction/re-read rate "on trimmed calls" with no rounding.
    trimmed_n_total: i64,
    trimmed_corrections_total: i64,
    trimmed_rereads_total: i64,
}

fn proof_metric(hits: i64, n: i64) -> Option<ProofMetric> {
    if n <= 0 {
        return None;
    }
    let (lo, hi) = crate::stats::wilson_interval(hits, n);
    Some(ProofMetric {
        rate: hits as f64 / n as f64,
        lo,
        hi,
    })
}

/// Plain-token verdict for a single delta interval, relative to zero. The tool-level verdict
/// uses the causal gate (with its noise slack); this is the readable per-metric description.
fn delta_verdict(lo: f64, hi: f64) -> &'static str {
    if hi <= 0.0 {
        "safe"
    } else if lo > 0.0 {
        "harmful"
    } else {
        "unclear"
    }
}

fn proof_delta(trim_hits: i64, trim_n: i64, base_hits: i64, base_n: i64) -> Option<ProofDelta> {
    if trim_n <= 0 || base_n <= 0 {
        return None;
    }
    let (diff, lo, hi) = crate::stats::newcombe_diff(trim_hits, trim_n, base_hits, base_n);
    Some(ProofDelta {
        diff,
        lo,
        hi,
        verdict: delta_verdict(lo, hi).to_string(),
    })
}

fn proof_tool_view(
    o: &crate::db::CausalToolOutcome,
    th: &crate::compress::activation::CausalThresholds,
    trialing: bool,
) -> ProofToolView {
    let correction_delta = proof_delta(
        o.trimmed_corrections,
        o.trimmed_n,
        o.baseline_corrections,
        o.baseline_n,
    );
    let reread_delta = proof_delta(
        o.trimmed_rereads,
        o.trimmed_n,
        o.baseline_rereads,
        o.baseline_n,
    );

    // Tool-level verdict. Fails closed: never "safe" until both arms clear the minimum and the
    // causal gate passes, so the page agrees with live activation.
    let verdict = if o.trimmed_n == 0 {
        "not_tested"
    } else if o.baseline_n < th.min_baseline || o.trimmed_n < th.min_trimmed {
        "collecting"
    } else if crate::compress::activation::causal_clears_bar(o, th) {
        "safe"
    } else if correction_delta.as_ref().is_some_and(|d| d.lo > 0.0)
        || reread_delta.as_ref().is_some_and(|d| d.lo > 0.0)
    {
        "harmful"
    } else {
        "unclear"
    };

    ProofToolView {
        tool: o.tool_name.clone(),
        trialing,
        baseline_n: o.baseline_n,
        trimmed_n: o.trimmed_n,
        baseline_corrections: proof_metric(o.baseline_corrections, o.baseline_n),
        baseline_rereads: proof_metric(o.baseline_rereads, o.baseline_n),
        trimmed_corrections: proof_metric(o.trimmed_corrections, o.trimmed_n),
        trimmed_rereads: proof_metric(o.trimmed_rereads, o.trimmed_n),
        correction_delta,
        reread_delta,
        verdict: verdict.to_string(),
        applied_chars_saved: 0,
        trimmed_collected: o.trimmed_collected,
        is_edit_tool: o.is_edit_tool,
    }
}

/// GET /api/context/proof — per-tool causal before/after with intervals and verdicts. Only tools
/// the heuristic has wanted to trim (a non-empty baseline or trimmed arm) appear, so the page
/// never shows a tool with nothing to prove.
async fn api_context_proof() -> Json<ProofView> {
    use crate::compress::activation::CausalThresholds;
    let cfg = crate::config::Config::load();
    let th = CausalThresholds::default();
    let mut trimmed_n_total = 0i64;
    let mut trimmed_corrections_total = 0i64;
    let mut trimmed_rereads_total = 0i64;
    let (tools, safe_chars_saved, trial_chars_saved) = match open_ctx_db() {
        Some(conn) => {
            // Applied chars saved per tool, then bucketed by verdict so only earned ("safe")
            // trimming is counted as real savings.
            let savings: std::collections::HashMap<String, i64> =
                crate::db::compress_savings_by_tool(&conn)
                    .into_iter()
                    .map(|s| (s.tool_name, s.chars_saved))
                    .collect();
            let outcomes: Vec<crate::db::CausalToolOutcome> =
                crate::db::causal_tool_outcomes(&conn, None)
                    .into_iter()
                    .filter(|o| o.baseline_n > 0 || o.trimmed_n > 0)
                    .collect();
            trimmed_n_total = outcomes.iter().map(|o| o.trimmed_n).sum();
            trimmed_corrections_total = outcomes.iter().map(|o| o.trimmed_corrections).sum();
            trimmed_rereads_total = outcomes.iter().map(|o| o.trimmed_rereads).sum();
            let tools: Vec<ProofToolView> = outcomes
                .into_iter()
                .map(|o| {
                    let trialing = cfg.compress_trial_tools.iter().any(|t| t == &o.tool_name);
                    let mut v = proof_tool_view(&o, &th, trialing);
                    v.applied_chars_saved = savings.get(&v.tool).copied().unwrap_or(0);
                    v
                })
                .collect();
            // Tools that have applied trims but never joined an outcome are absent from the proof
            // list; their savings are unproven, so fold them into the trial bucket.
            let proof_tools: std::collections::HashSet<&String> =
                tools.iter().map(|t| &t.tool).collect();
            let mut safe = 0i64;
            let mut trial = 0i64;
            for t in &tools {
                if t.verdict == "safe" {
                    safe += t.applied_chars_saved;
                } else {
                    trial += t.applied_chars_saved;
                }
            }
            for (name, chars) in &savings {
                if !proof_tools.contains(name) {
                    trial += *chars;
                }
            }
            (tools, safe, trial)
        }
        None => (Vec::new(), 0, 0),
    };
    Json(ProofView {
        preset: cfg.compress_preset.as_str().to_string(),
        trial_tools: cfg.compress_trial_tools.clone(),
        min_baseline: th.min_baseline,
        min_trimmed: th.min_trimmed,
        tools,
        safe_chars_saved,
        trial_chars_saved,
        trimmed_n_total,
        trimmed_corrections_total,
        trimmed_rereads_total,
    })
}

#[derive(serde::Deserialize)]
struct ContextTrialBody {
    tool: String,
    on: bool,
}

/// POST /api/context/trial — start or stop a single-tool trim trial from the dashboard.
async fn api_context_trial(Json(body): Json<ContextTrialBody>) -> impl IntoResponse {
    let tool = body.tool.trim();
    if tool.is_empty() {
        return (StatusCode::BAD_REQUEST, "name a tool to trial".to_string()).into_response();
    }
    let res = crate::context_ctl::trial(Some(tool), body.on, !body.on);
    match res {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
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

// ---------------------------------------------------------------------------
// Tab 2 — Prompt Stats
// ---------------------------------------------------------------------------

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
    dashboard_port: Option<u16>,
    auto_profile_enabled: bool,
    filter_mode: String,
    inject_enabled: bool,
    coaching_enabled: bool,
    adaptive_prefix_enabled: bool,
    compress_enabled: bool,
    compress_sgr_enabled: bool,
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
        .query_row("SELECT v FROM meta WHERE k = 'last_ingest_at'", [], |r| {
            r.get(0)
        })
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
        dashboard_port: cfg.dashboard_port,
        auto_profile_enabled: cfg.auto_profile_enabled,
        filter_mode: cfg.filter_mode.as_str().to_string(),
        inject_enabled: cfg.inject_enabled,
        coaching_enabled: cfg.coaching_enabled,
        adaptive_prefix_enabled: cfg.adaptive_prefix_enabled,
        compress_enabled: cfg.compress_enabled,
        compress_sgr_enabled: cfg.compress_sgr_enabled,
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
    compress_enabled: Option<bool>,
    compress_sgr_enabled: Option<bool>,
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

async fn api_settings_post(Json(body): Json<SettingsPostBody>) -> impl IntoResponse {
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
        }
    }

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
    if let Some(v) = body.compress_enabled {
        cfg.compress_enabled = v;
    }
    if let Some(v) = body.compress_sgr_enabled {
        cfg.compress_sgr_enabled = v;
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
    if let Err(e) = cfg.save() {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    let _ = crate::filter_hook::sync_filter_config_from_active_config();
    Json(serde_json::json!({ "ok": true })).into_response()
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
    Json(SwitchResponse {
        ok,
        active: body.slug,
    })
}

// ---------------------------------------------------------------------------
// Profile analytics — per-profile request / token breakdown
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Request trace — recent per-request records with server breakdown
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Hook events — companion to request trace for v2 hook architecture
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Hook traces — hybrid rows from hooks, enriched by JSONL ingest
// ---------------------------------------------------------------------------

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
    avg_compress_chars_saved: f64,
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
                AVG(adaptive_chars),
                AVG(compress_chars_saved)
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
                r.get(10)?,
            ))
        })
        .unwrap_or((0, None, None, None, 0, None, None, None, None, None, None))
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
                r.get(10)?,
            ))
        })
        .unwrap_or((0, None, None, None, 0, None, None, None, None, None, None))
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
        avg_compress_chars_saved: row.10.unwrap_or(0.0),
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
        ("compress", "%X:T%", "%X:C%"),
        ("compress_sgr", "%S:T%", "%S:C%"),
        ("tool_mix", "%M:T%", "%M:C%"),
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
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'compress', 'treatment'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%X:T%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'compress', 'control'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%X:C%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'compress_sgr', 'treatment'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%S:T%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'compress_sgr', 'control'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%S:C%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'tool_mix', 'treatment'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%M:T%'
            UNION ALL
            SELECT ts, cost_usd, input_tokens, output_tokens,
                   'tool_mix', 'control'
            FROM hook_traces
            WHERE ab_group IS NOT NULL AND enriched = 1 AND ab_group LIKE '%M:C%'
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

// ---------------------------------------------------------------------------
// MCP tool sent vs invoked (approximate)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Gates pipeline — status and activity for all ctx interception layers
// ---------------------------------------------------------------------------

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
    let fallback = ProfileSuggestion {
        profile: String::new(),
        confidence: 0.0,
        based_on: 0,
    };
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
