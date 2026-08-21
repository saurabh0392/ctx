//! Local product snapshot, onboarding stage, and product-event recording.
//!
//! Everything here is computed from the local database and stays on this machine. CTX has no
//! background telemetry and no remote intake; issue reports go through GitHub.

use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProductSnapshot {
    pub schema_version: u32,
    pub ctx_version: String,
    pub os: String,
    pub arch: String,
    pub active_days_total: i64,
    pub active_days_last7: i64,
    pub sessions_total: i64,
    pub sessions_last7: i64,
    pub decisions_total: i64,
    pub decisions_joined: i64,
    pub bill_ready: bool,
    pub sink_tokens: i64,
    pub reclaimable_tokens: i64,
    pub reclaimed_tokens: i64,
    pub applied_trims: i64,
    pub reexpansions: i64,
    pub suspected_recovery_events: i64,
    pub tools_watching: i64,
    pub tools_trialing: i64,
    pub tools_earned: i64,
    pub pruned_server_count: i64,
    pub insight_action_count: i64,
    pub latest_net_ahead_state: String,
    pub product_event_counts: BTreeMap<String, i64>,
}

fn scalar(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0)
}

fn active_days(conn: &Connection, last_seven_days: bool) -> i64 {
    conn.query_row(
        "SELECT COUNT(DISTINCT day) FROM (\
         SELECT date(ts) AS day FROM compress_decisions \
         UNION ALL SELECT date(started_at) FROM sessions \
         UNION ALL SELECT date(ts) FROM product_events) \
         WHERE day IS NOT NULL \
           AND (?1 = 0 OR day >= date('now','-6 days'))",
        rusqlite::params![i64::from(last_seven_days)],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

pub fn build_snapshot(conn: &Connection) -> ProductSnapshot {
    let _ = crate::db::ensure_schema(conn);
    let stats = crate::db::compress_decision_stats(conn);
    let bill = crate::db::context_bill(conn);
    let counts = crate::db::product_event_counts(conn);
    let active_days_total = active_days(conn, false);
    let active_days_last7 = active_days(conn, true);
    let sessions_total = scalar(conn, "SELECT COUNT(*) FROM sessions");
    let sessions_last7 = scalar(
        conn,
        "SELECT COUNT(*) FROM sessions WHERE datetime(started_at) >= datetime('now','-7 days')",
    );
    let applied_trims = scalar(
        conn,
        "SELECT COUNT(*) FROM compress_decisions WHERE applied=1 AND lines_drop>0",
    );
    let reexpansions = scalar(
        conn,
        "SELECT COUNT(*) FROM rewind_store WHERE expanded_at IS NOT NULL",
    );
    let suspected_recovery_events = crate::db::tool_attribution(conn)
        .iter()
        .map(|t| t.suspect)
        .sum();

    let th = crate::compress::activation::CausalThresholds::default();
    let mut tools_watching = 0;
    let mut tools_trialing = 0;
    let mut tools_earned = 0;
    for outcome in crate::db::causal_tool_outcomes(conn, None) {
        use crate::compress::activation::ToolStage;
        match crate::compress::activation::tool_stage(&outcome, &th) {
            ToolStage::Learning { .. } => tools_trialing += 1,
            ToolStage::Earned => tools_earned += 1,
            ToolStage::Watching { .. } | ToolStage::Held | ToolStage::Blocked => {
                tools_watching += 1
            }
        }
    }
    let cfg = crate::config::Config::load();
    for tool in &cfg.compress_trial_tools {
        if !crate::db::causal_tool_outcomes(conn, Some(tool))
            .iter()
            .any(|o| {
                matches!(
                    crate::compress::activation::tool_stage(o, &th),
                    crate::compress::activation::ToolStage::Learning { .. }
                )
            })
        {
            tools_trialing += 1;
        }
    }
    let local_actions = crate::db::insight_actions(conn).total;
    // Rewinds are already represented in the local insight-action total. Server pruning is not,
    // so add only those product events and avoid inflating the count when a rewind records both.
    let event_actions = counts.get("server_pruned").copied().unwrap_or(0)
        + counts.get("server_unpruned").copied().unwrap_or(0);
    let latest_net_ahead_state = crate::db::weekly_net_ahead(conn)
        .first()
        .map(|w| {
            if w.net_ahead {
                "net_ahead"
            } else if w.harm_unconfirmed {
                "unconfirmed"
            } else {
                "behind"
            }
        })
        .unwrap_or("no_data")
        .to_string();

    ProductSnapshot {
        schema_version: 2,
        ctx_version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        active_days_total,
        active_days_last7,
        sessions_total,
        sessions_last7,
        decisions_total: stats.total,
        decisions_joined: stats.joined,
        bill_ready: bill.decisions > 0,
        sink_tokens: bill.total_sink_chars / 4,
        reclaimable_tokens: bill.total_reclaimable_chars / 4,
        reclaimed_tokens: bill.total_reclaimed_chars / 4,
        applied_trims,
        reexpansions,
        suspected_recovery_events,
        tools_watching,
        tools_trialing,
        tools_earned,
        pruned_server_count: cfg.pruned_servers.len() as i64,
        insight_action_count: local_actions + event_actions,
        latest_net_ahead_state,
        product_event_counts: counts,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OnboardingView {
    pub stage: String,
    pub autopilot_enabled: bool,
    pub bill_ready: bool,
    pub reclaimed_tokens: i64,
}

pub fn onboarding(conn: &Connection) -> OnboardingView {
    let snapshot = build_snapshot(conn);
    let cfg = crate::config::Config::load();
    let stage = if snapshot.reclaimed_tokens > 0 {
        "reclaiming"
    } else if snapshot.tools_trialing > 0 {
        "trialing"
    } else if snapshot.bill_ready {
        "bill_ready"
    } else if snapshot.sessions_total > 0 || snapshot.decisions_total > 0 {
        "observing"
    } else {
        "installed"
    };
    OnboardingView {
        stage: stage.into(),
        autopilot_enabled: cfg.compress_preset != crate::config::CompressPreset::Off,
        bill_ready: snapshot.bill_ready,
        reclaimed_tokens: snapshot.reclaimed_tokens,
    }
}

pub fn record_event(event: &str, source: &str, value: Option<&str>) {
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let _ = crate::db::record_product_event(&conn, event, source, value);
    }
}

/// Path of the legacy beta enrollment file; kept only so setup/uninstall can clean it up.
pub fn legacy_beta_state_path() -> std::path::PathBuf {
    crate::config::ctx_dir().join("beta.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_schema_is_content_free() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temporary = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", temporary.path());
        let conn = Connection::open_in_memory().unwrap();
        crate::db::ensure_schema(&conn).unwrap();
        let value = serde_json::to_value(build_snapshot(&conn)).unwrap();
        let text = value.to_string();
        for forbidden in ["prompt", "command", "path", "repo", "tool_name", "output"] {
            assert!(!text.contains(forbidden), "snapshot leaked key {forbidden}");
        }
    }
}
