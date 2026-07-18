//! Claude Code statusLine allowance snapshots and burn-rate reconciliation.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

pub const ALLOWANCE_MIN_SNAPSHOTS: usize = 10;
pub const ALLOWANCE_MIN_PAIRS: usize = 5;
pub const PRIMARY_WINDOW: &str = "seven_day";

#[derive(Debug, Clone, Serialize)]
pub struct AllowanceWindowCurrent {
    pub used_pct: f64,
    pub remaining_pct: f64,
    pub resets_at: Option<i64>,
    pub resets_in_secs: Option<i64>,
    pub updated_at: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllowanceCurrentResponse {
    pub configured: bool,
    /// ctx statusLine entry present in ~/.claude/settings.json
    pub statusline_wired: bool,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_statusline_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_hint: Option<String>,
    pub windows: std::collections::HashMap<String, AllowanceWindowCurrent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllowanceBurnRateResponse {
    pub metrics_ready: bool,
    pub window: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx_active_since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_pct_per_hour: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recent_pct_per_hour: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub fn ingest_statusline_payload(conn: &Connection, payload: &Value) -> Result<usize> {
    let ts = Utc::now().to_rfc3339();
    let session_id = payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let model = payload
        .get("model")
        .and_then(|m| m.get("display_name").or_else(|| m.get("id")))
        .and_then(|v| v.as_str())
        .map(String::from);
    let session_cost = payload
        .get("cost")
        .and_then(|c| c.get("total_cost_usd"))
        .and_then(|v| v.as_f64());

    let rate_limits = payload.get("rate_limits");
    let has_limits = rate_limits
        .and_then(|r| r.as_object())
        .map(|o| !o.is_empty())
        .unwrap_or(false);
    if !has_limits {
        let _ = conn.execute(
            "INSERT OR REPLACE INTO meta (k, v) VALUES ('last_statusline_at', ?1)",
            rusqlite::params![ts],
        );
        return Ok(0);
    }

    let rate_limits = rate_limits.unwrap();

    let mut inserted = 0usize;
    for (window, key) in [("five_hour", "five_hour"), ("seven_day", "seven_day")] {
        let Some(win) = rate_limits.get(key) else {
            continue;
        };
        let Some(used) = win.get("used_percentage").and_then(|v| v.as_f64()) else {
            continue;
        };
        let remaining = (100.0 - used).max(0.0);
        let resets_at = win.get("resets_at").and_then(parse_resets_at);
        if crate::db::insert_allowance_snapshot(
            conn,
            &ts,
            session_id.as_deref(),
            model.as_deref(),
            window,
            used,
            Some(remaining),
            resets_at,
            session_cost,
        )? {
            inserted += 1;
        }
    }
    Ok(inserted)
}

/// Parse Claude statusLine `resets_at` (epoch seconds, epoch ms, or RFC3339).
fn parse_resets_at(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        if n > 1_000_000_000_000 {
            return Some(n / 1000);
        }
        if n > 1_000_000_000 {
            return Some(n);
        }
    }
    if let Some(s) = v.as_str() {
        if let Ok(dt) = s.parse::<DateTime<Utc>>() {
            return Some(dt.timestamp());
        }
    }
    None
}

pub fn current_allowance(conn: &Connection) -> AllowanceCurrentResponse {
    let mut windows = std::collections::HashMap::new();
    let now = Utc::now();

    for window in ["seven_day", "five_hour"] {
        let Some(row) = crate::db::latest_allowance_snapshot(conn, window) else {
            continue;
        };
        if !snapshot_window_valid(&row, now) {
            continue;
        }
        let remaining = row.remaining_pct.unwrap_or((100.0 - row.used_pct).max(0.0));
        let resets_in_secs = row.resets_at.map(|reset| (reset - now.timestamp()).max(0));
        windows.insert(
            window.to_string(),
            AllowanceWindowCurrent {
                used_pct: row.used_pct,
                remaining_pct: remaining,
                resets_at: row.resets_at,
                resets_in_secs,
                updated_at: row.ts,
                model: row.model,
            },
        );
    }

    let configured = !windows.is_empty();
    let last_statusline_at = crate::db::get_meta(conn, "last_statusline_at");
    let stale = if configured {
        windows.values().any(|w| {
            w.updated_at
                .parse::<DateTime<Utc>>()
                .map(|dt| now.signed_duration_since(dt) > Duration::hours(6))
                .unwrap_or(true)
        })
    } else {
        true
    };

    let statusline_wired = crate::claude_settings::ctx_statusline_wired_in_settings();

    AllowanceCurrentResponse {
        configured,
        statusline_wired,
        stale,
        last_statusline_at,
        setup_hint: if configured && !stale {
            None
        } else if statusline_wired {
            Some(
                "statusLine is wired. Reload Claude Code (Cmd+Shift+P → Reload Window), send one prompt, then wait for the first API response. Allowance % needs Claude Pro/Max.".into(),
            )
        } else {
            Some(
                "Run `ctx setup` to install the statusLine bridge, then reload Claude Code.".into(),
            )
        },
        windows,
    }
}

fn snapshot_window_valid(row: &crate::db::AllowanceSnapshotRow, now: DateTime<Utc>) -> bool {
    let Ok(dt) = row.ts.parse::<DateTime<Utc>>() else {
        return false;
    };
    now.signed_duration_since(dt) <= Duration::hours(6)
}

pub fn burn_rate(conn: &Connection) -> AllowanceBurnRateResponse {
    let ctx_active_since = crate::db::get_ctx_active_since(conn);
    let Some(active_since) = ctx_active_since.as_deref() else {
        return AllowanceBurnRateResponse {
            metrics_ready: false,
            window: PRIMARY_WINDOW.into(),
            ctx_active_since,
            baseline_pct_per_hour: None,
            recent_pct_per_hour: None,
            delta_pct: None,
            direction: None,
            message: Some("ctx install watermark missing — burn rate unavailable.".into()),
        };
    };

    let Ok(active_dt) = active_since.parse::<DateTime<Utc>>() else {
        return AllowanceBurnRateResponse {
            metrics_ready: false,
            window: PRIMARY_WINDOW.into(),
            ctx_active_since,
            baseline_pct_per_hour: None,
            recent_pct_per_hour: None,
            delta_pct: None,
            direction: None,
            message: Some("Invalid ctx_active_since timestamp.".into()),
        };
    };

    let now = Utc::now();
    let baseline_end = (active_dt + Duration::days(7)).min(now);
    let recent_start = now - Duration::days(7);

    let baseline_rows = crate::db::load_allowance_snapshots(
        conn,
        PRIMARY_WINDOW,
        Some(active_since),
        Some(&baseline_end.to_rfc3339()),
    );
    let recent_rows = crate::db::load_allowance_snapshots(
        conn,
        PRIMARY_WINDOW,
        Some(&recent_start.to_rfc3339()),
        None,
    );

    let baseline_rate = period_burn_rate(&baseline_rows);
    let recent_rate = period_burn_rate(&recent_rows);

    match (baseline_rate, recent_rate) {
        (Some(base), Some(recent)) if base > 0.001 => {
            let delta = ((base - recent) / base) * 100.0;
            let direction = if delta.abs() < 3.0 {
                "flat".into()
            } else if delta > 0.0 {
                "slower".into()
            } else {
                "faster".into()
            };
            let message = if direction == "flat" {
                "Allowance burn is about the same as your first week with ctx.".into()
            } else if direction == "slower" {
                format!(
                    "Allowance burn is {:.0}% slower than your first week with ctx.",
                    delta.abs()
                )
            } else {
                format!(
                    "Allowance burn is {:.0}% faster than your first week with ctx.",
                    delta.abs()
                )
            };
            AllowanceBurnRateResponse {
                metrics_ready: true,
                window: PRIMARY_WINDOW.into(),
                ctx_active_since,
                baseline_pct_per_hour: Some(base),
                recent_pct_per_hour: Some(recent),
                delta_pct: Some(delta),
                direction: Some(direction),
                message: Some(message),
            }
        }
        _ => AllowanceBurnRateResponse {
            metrics_ready: false,
            window: PRIMARY_WINDOW.into(),
            ctx_active_since,
            baseline_pct_per_hour: baseline_rate,
            recent_pct_per_hour: recent_rate,
            delta_pct: None,
            direction: None,
            message: Some(format!(
                "Collecting allowance baseline — need at least {} snapshots in your first week and last 7 days.",
                ALLOWANCE_MIN_SNAPSHOTS
            )),
        },
    }
}

fn period_burn_rate(rows: &[crate::db::AllowanceSnapshotRow]) -> Option<f64> {
    if rows.len() < ALLOWANCE_MIN_SNAPSHOTS {
        return None;
    }

    let mut total = 0.0;
    let mut pairs = 0usize;
    for w in rows.windows(2) {
        let Ok(t0) = w[0].ts.parse::<DateTime<Utc>>() else {
            continue;
        };
        let Ok(t1) = w[1].ts.parse::<DateTime<Utc>>() else {
            continue;
        };
        let hours = (t1 - t0).num_seconds() as f64 / 3600.0;
        if hours <= 0.0 {
            continue;
        }
        let delta = w[1].used_pct - w[0].used_pct;
        if delta >= 0.0 {
            total += delta / hours;
            pairs += 1;
        }
    }

    if pairs < ALLOWANCE_MIN_PAIRS {
        return None;
    }
    Some(total / pairs as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn row(ts: &str, used: f64) -> crate::db::AllowanceSnapshotRow {
        crate::db::AllowanceSnapshotRow {
            id: 0,
            ts: ts.into(),
            session_id: None,
            model: None,
            window: PRIMARY_WINDOW.into(),
            used_pct: used,
            remaining_pct: Some(100.0 - used),
            resets_at: None,
            session_cost_usd: None,
        }
    }

    #[test]
    fn period_burn_rate_needs_minimum_snapshots() {
        assert!(period_burn_rate(&[]).is_none());
    }

    #[test]
    fn period_burn_rate_computes_positive_slope() {
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let rows: Vec<_> = (0..12)
            .map(|i| {
                row(
                    &(start + Duration::minutes(30 * i)).to_rfc3339(),
                    i as f64 * 2.0,
                )
            })
            .collect();
        let rate = period_burn_rate(&rows).expect("rate");
        assert!(rate > 0.0);
    }

    #[test]
    fn snapshot_window_accepts_fresh_ts_even_when_resets_at_passed() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 12, 0, 0).unwrap();
        let mut r = row("2026-05-28T11:00:00Z", 34.0);
        r.resets_at = Some(now.timestamp() - 600);
        assert!(snapshot_window_valid(&r, now));
    }

    #[test]
    fn snapshot_window_rejects_stale_ts() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 12, 0, 0).unwrap();
        let mut r = row("2026-05-27T00:00:00Z", 10.0);
        r.resets_at = Some(now.timestamp() + 3600);
        assert!(!snapshot_window_valid(&r, now));
    }
}
