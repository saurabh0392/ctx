//! Token-gated beta state and privacy-bounded evidence snapshots.
//!
//! CTX has no background telemetry. This module stores the beta capability locally, builds an
//! allowlisted aggregate snapshot for preview, and sends only after an explicit dashboard action.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration as StdDuration;

pub const DIST_ENDPOINT: &str =
    "https://lkj2hle2qarv4liyggqpqtarr40fkhtw.lambda-url.us-east-1.on.aws/";
pub const FEEDBACK_ENDPOINT: &str =
    "https://yds5zrqx7pbhf7jcigvsjirepm0twbga.lambda-url.us-east-1.on.aws/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BetaState {
    pub schema_version: u32,
    pub release_channel: String,
    pub participant_id: String,
    pub installed_at: String,
    pub credential: String,
    pub dist_endpoint: String,
    pub feedback_endpoint: String,
    #[serde(default)]
    pub checkin_dismissed_until: Option<String>,
}

pub fn state_path() -> PathBuf {
    crate::config::ctx_dir().join("beta.json")
}

pub fn load_state() -> Option<BetaState> {
    let text = std::fs::read_to_string(state_path()).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn capability_details(credential: &str) -> Option<(String, DateTime<Utc>)> {
    let parts: Vec<_> = credential.split('.').collect();
    if parts.len() != 5
        || parts[0] != "v1"
        || parts[1].len() != 16
        || !parts[1].chars().all(|c| c.is_ascii_hexdigit())
        || !parts[3].split('-').any(|scope| scope == "download")
        || !parts[3].split('-').any(|scope| scope == "feedback")
        || parts[4].len() != 64
        || !parts[4].chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    let expiry = DateTime::from_timestamp(parts[2].parse().ok()?, 0)?;
    Some((parts[1].to_string(), expiry))
}

fn validate_capability(credential: &str) -> Result<(String, DateTime<Utc>)> {
    let details = capability_details(credential).context("malformed beta capability")?;
    if details.1 <= Utc::now() {
        bail!("beta capability expired; reinstall with a current invite");
    }
    Ok(details)
}

fn write_state(state: &BetaState) -> Result<()> {
    crate::config::ensure_dir()?;
    let path = state_path();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(tmp, path)?;
    Ok(())
}

pub fn refresh_credential(credential: &str) -> Result<()> {
    let (participant_id, _) = validate_capability(credential)?;
    let mut state = load_state().context("this install is not enrolled in the beta")?;
    state.credential = credential.to_string();
    state.participant_id = participant_id;
    write_state(&state)
}

/// Persist the scoped capability passed by the binary installer. The one-time invite token is
/// intentionally never read or stored here.
pub fn activate_from_environment() -> Result<BetaState> {
    let prior = load_state();
    let credential = std::env::var("CTX_BETA_CREDENTIAL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| prior.as_ref().map(|s| s.credential.clone()))
        .unwrap_or_default();
    let (participant_id, _) = validate_capability(&credential)
        .context("ctx setup --beta must be run by the token-gated beta installer")?;
    if let Some(supplied_id) = std::env::var("CTX_PARTICIPANT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        if supplied_id != participant_id {
            bail!("beta participant id does not match the scoped capability");
        }
    }
    let state = BetaState {
        schema_version: 1,
        release_channel: "beta".into(),
        participant_id,
        installed_at: prior
            .as_ref()
            .map(|s| s.installed_at.clone())
            .unwrap_or_else(|| Utc::now().to_rfc3339()),
        credential,
        dist_endpoint: std::env::var("CTX_DIST_ENDPOINT")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| prior.as_ref().map(|s| s.dist_endpoint.clone()))
            .unwrap_or_else(|| DIST_ENDPOINT.into()),
        feedback_endpoint: std::env::var("CTX_FEEDBACK_ENDPOINT")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| prior.as_ref().map(|s| s.feedback_endpoint.clone()))
            .unwrap_or_else(|| FEEDBACK_ENDPOINT.into()),
        checkin_dismissed_until: prior.and_then(|s| s.checkin_dismissed_until),
    };
    write_state(&state)?;
    Ok(state)
}

pub fn remove_state() -> Result<()> {
    let path = state_path();
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AlphaSnapshotV1 {
    pub schema_version: u32,
    pub participant_id: String,
    pub ctx_version: String,
    pub release_channel: String,
    pub os: String,
    pub arch: String,
    pub installed_at: String,
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

fn active_days_since(conn: &Connection, installed_at: &str, last_seven_days: bool) -> i64 {
    conn.query_row(
        "SELECT COUNT(DISTINCT day) FROM (\
         SELECT date(ts) AS day, ts AS occurred_at FROM compress_decisions \
         UNION ALL SELECT date(started_at), started_at FROM sessions \
         UNION ALL SELECT date(ts), ts FROM product_events) \
         WHERE day IS NOT NULL \
           AND datetime(occurred_at) >= datetime(?1) \
           AND (?2 = 0 OR day >= date('now','-6 days'))",
        rusqlite::params![installed_at, i64::from(last_seven_days)],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

pub fn build_snapshot(conn: &Connection) -> AlphaSnapshotV1 {
    let _ = crate::db::ensure_schema(conn);
    let state = load_state();
    let stats = crate::db::compress_decision_stats(conn);
    let bill = crate::db::context_bill(conn);
    let counts = crate::db::product_event_counts(conn);
    // Setup deliberately ingests pre-CTX Claude history. Beta retention/check-in timing must start at
    // enrollment, otherwise a fresh install can appear to have seven active beta days immediately.
    let installed_since = state
        .as_ref()
        .map(|value| value.installed_at.as_str())
        .unwrap_or("1970-01-01T00:00:00Z");
    let active_days_total = active_days_since(conn, installed_since, false);
    let active_days_last7 = active_days_since(conn, installed_since, true);
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

    AlphaSnapshotV1 {
        schema_version: 1,
        participant_id: state
            .as_ref()
            .map(|s| s.participant_id.clone())
            .unwrap_or_else(|| "not-enrolled".into()),
        ctx_version: env!("CARGO_PKG_VERSION").into(),
        release_channel: state
            .as_ref()
            .map(|s| s.release_channel.clone())
            .unwrap_or_else(|| "local".into()),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        installed_at: state
            .as_ref()
            .map(|s| s.installed_at.clone())
            .unwrap_or_default(),
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
    pub enrolled: bool,
    pub stage: String,
    pub autopilot_enabled: bool,
    pub bill_ready: bool,
    pub reclaimed_tokens: i64,
    pub checkin_due: bool,
    pub checkin_target_day: Option<i64>,
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
    let sent = snapshot
        .product_event_counts
        .get("beta_checkin_sent")
        .copied()
        .unwrap_or(0);
    let target = match sent {
        0 => Some(7),
        1 => Some(21),
        _ => None,
    };
    let dismissed = load_state()
        .and_then(|s| s.checkin_dismissed_until)
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|t| t.with_timezone(&Utc) > Utc::now())
        .unwrap_or(false);
    OnboardingView {
        enrolled: load_state().is_some(),
        stage: stage.into(),
        autopilot_enabled: cfg.compress_preset != crate::config::CompressPreset::Off,
        bill_ready: snapshot.bill_ready,
        reclaimed_tokens: snapshot.reclaimed_tokens,
        checkin_due: target
            .map(|day| snapshot.active_days_total >= day && !dismissed)
            .unwrap_or(false),
        checkin_target_day: target,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckinAnswers {
    pub learned_something: String,
    pub changed_behavior: String,
    pub keep_using: String,
    pub price_interest_25_per_developer: String,
}

impl CheckinAnswers {
    fn bounded(mut self) -> Self {
        fn chars(value: String, limit: usize) -> String {
            value.chars().take(limit).collect()
        }
        self.learned_something = chars(self.learned_something, 500);
        self.changed_behavior = chars(self.changed_behavior, 500);
        self.keep_using = chars(self.keep_using, 100);
        self.price_interest_25_per_developer = chars(self.price_interest_25_per_developer, 100);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckinEnvelope {
    pub schema: String,
    pub snapshot: AlphaSnapshotV1,
    pub answers: CheckinAnswers,
}

fn pending_checkin_path() -> PathBuf {
    crate::config::ctx_dir().join("pending-checkin.json")
}

fn write_pending_checkin(envelope: &CheckinEnvelope) -> Result<()> {
    let pending = pending_checkin_path();
    let temporary = pending.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(envelope)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(temporary, pending)?;
    Ok(())
}

pub fn preview_checkin(conn: &Connection, answers: CheckinAnswers) -> Result<CheckinEnvelope> {
    let _ = crate::db::record_product_event(conn, "beta_checkin_previewed", "dashboard", None);
    let envelope = CheckinEnvelope {
        schema: "ctx.beta-checkin.v1".into(),
        snapshot: build_snapshot(conn),
        answers: answers.bounded(),
    };
    write_pending_checkin(&envelope)?;
    Ok(envelope)
}

pub fn dismiss_checkin() -> Result<()> {
    let mut state = load_state().context("this install is not enrolled in the beta")?;
    state.checkin_dismissed_until = Some((Utc::now() + Duration::days(7)).to_rfc3339());
    write_state(&state)?;
    let pending = pending_checkin_path();
    if pending.exists() {
        std::fs::remove_file(pending)?;
    }
    Ok(())
}

pub async fn send_checkin() -> Result<serde_json::Value> {
    let state = load_state().context("this install is not enrolled in the beta")?;
    if state.credential.is_empty() {
        bail!("beta capability is missing; reinstall with your current invite");
    }
    let pending = pending_checkin_path();
    let envelope: CheckinEnvelope = serde_json::from_slice(
        &std::fs::read(&pending)
            .context("no reviewed check-in is pending; preview the JSON before sending")?,
    )
    .context("the pending check-in is invalid; preview it again")?;
    if envelope.schema != "ctx.beta-checkin.v1"
        || envelope.snapshot.participant_id != state.participant_id
    {
        bail!("the pending check-in does not belong to this beta participant; preview it again");
    }
    let response = reqwest::Client::builder()
        .timeout(StdDuration::from_secs(30))
        .build()?
        .post(&state.feedback_endpoint)
        .json(&serde_json::json!({
            "action": "checkin",
            "credential": state.credential,
            "checkin": envelope,
        }))
        .send()
        .await
        .context("send beta check-in")?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .unwrap_or_else(|_| serde_json::json!({"error":"invalid intake response"}));
    if !status.is_success() {
        bail!("check-in intake returned {status}: {body}");
    }
    let _ = std::fs::remove_file(pending);
    record_event("beta_checkin_sent", "dashboard", None);
    Ok(body)
}

pub struct ProxyResponse {
    pub status: reqwest::StatusCode,
    pub body: serde_json::Value,
}

/// Forward a report action to the private intake without revealing the capability to browser JS.
pub async fn proxy_feedback(mut body: serde_json::Value) -> Result<ProxyResponse> {
    let state = load_state().context("this install is not enrolled in the beta")?;
    if state.credential.is_empty() {
        bail!("beta capability is missing; reinstall with your current invite");
    }
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
    if !matches!(action, "presign" | "submit") {
        bail!("unsupported feedback action");
    }
    body.as_object_mut()
        .context("feedback body must be an object")?
        .insert("credential".into(), state.credential.into());
    let response = reqwest::Client::builder()
        .timeout(StdDuration::from_secs(30))
        .build()?
        .post(&state.feedback_endpoint)
        .json(&body)
        .send()
        .await
        .context("send feedback")?;
    let status = response.status();
    let body = response
        .json()
        .await
        .unwrap_or_else(|_| serde_json::json!({"error":"invalid intake response"}));
    Ok(ProxyResponse { status, body })
}

pub fn record_event(event: &str, source: &str, value: Option<&str>) {
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let _ = crate::db::record_product_event(&conn, event, source, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_exposes_only_participant_id() {
        assert_eq!(
            capability_details("v1.0123456789abcdef.123.download-feedback.sig"),
            None
        );
        let future = (Utc::now() + Duration::days(1)).timestamp();
        let credential = format!(
            "v1.0123456789abcdef.{future}.download-feedback.{}",
            "a".repeat(64)
        );
        assert_eq!(
            capability_details(&credential).map(|(id, _)| id),
            Some("0123456789abcdef".into())
        );
        assert_eq!(capability_details("invite-token"), None);
    }

    #[test]
    fn beta_active_days_exclude_history_ingested_before_enrollment() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::ensure_schema(&conn).unwrap();
        let now = Utc::now();
        conn.execute(
            "INSERT INTO product_events (ts,event_name,source) VALUES (?1,'dashboard_opened','test')",
            rusqlite::params![(now - Duration::days(30)).to_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO product_events (ts,event_name,source) VALUES (?1,'dashboard_opened','test')",
            rusqlite::params![now.to_rfc3339()],
        )
        .unwrap();

        let installed_at = (now - Duration::hours(1)).to_rfc3339();
        assert_eq!(active_days_since(&conn, &installed_at, false), 1);
        assert_eq!(active_days_since(&conn, &installed_at, true), 1);
    }

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

    #[test]
    fn preview_is_the_same_bounded_payload_the_server_accepts() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temporary = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", temporary.path());
        let conn = Connection::open_in_memory().unwrap();
        crate::db::ensure_schema(&conn).unwrap();
        let preview = preview_checkin(
            &conn,
            CheckinAnswers {
                learned_something: "é".repeat(600),
                changed_behavior: "x".repeat(700),
                keep_using: "yes".repeat(100),
                price_interest_25_per_developer: "maybe".repeat(100),
            },
        )
        .unwrap();
        assert_eq!(preview.answers.learned_something.chars().count(), 500);
        assert_eq!(preview.answers.changed_behavior.chars().count(), 500);
        assert_eq!(preview.answers.keep_using.chars().count(), 100);
        assert_eq!(
            preview
                .answers
                .price_interest_25_per_developer
                .chars()
                .count(),
            100
        );
        let pending: CheckinEnvelope =
            serde_json::from_slice(&std::fs::read(pending_checkin_path()).unwrap()).unwrap();
        assert_eq!(pending.snapshot, preview.snapshot);
        assert_eq!(
            pending.answers.learned_something,
            preview.answers.learned_something
        );
    }
}
