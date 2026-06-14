//! Pre-switch safety tiers and post-switch quality alerts from SQLite history.

use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskTier {
    Safe,
    Low,
    Active,
    Critical,
}

#[derive(Debug, Serialize)]
pub struct ServerSafetyRow {
    pub server: String,
    pub invocations_30d: i64,
    pub sessions_30d: i64,
    pub session_pct: f64,
    pub risk: RiskTier,
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct ToolSafetyRow {
    pub tool_name: String,
    pub invocations_30d: i64,
    pub sessions_30d: i64,
    pub session_pct: f64,
    pub risk: RiskTier,
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct SafetyReport {
    pub rows: Vec<ServerSafetyRow>,
    pub tool_rows: Vec<ToolSafetyRow>,
    pub critical_blockers: Vec<String>,
}

fn kept_by_profile(prefix: &str, proposed_keep: &[String]) -> bool {
    if proposed_keep.is_empty() {
        return true;
    }
    proposed_keep
        .iter()
        .any(|k| prefix.starts_with(k.as_str()) || k.starts_with(prefix))
}

fn risk_from_usage(inv: i64, sess: i64, total_sessions: i64) -> RiskTier {
    if inv == 0 {
        return RiskTier::Safe;
    }
    let pct = sess as f64 / total_sessions as f64;
    if pct > 0.30 {
        RiskTier::Critical
    } else if pct >= 0.05 {
        RiskTier::Active
    } else {
        RiskTier::Low
    }
}

fn action_for_risk(risk: RiskTier) -> String {
    match risk {
        RiskTier::Critical => "Will not strip without --force on CLI".into(),
        RiskTier::Active => "Warn before stripping".into(),
        RiskTier::Low => "Low usage".into(),
        RiskTier::Safe => "Safe".into(),
    }
}

pub fn safety_report(proposed_keep: &[String]) -> SafetyReport {
    safety_report_for_profile(&crate::profiles::Profile {
        display: String::new(),
        description: String::new(),
        keep: proposed_keep.to_vec(),
        ..Default::default()
    })
}

pub fn safety_report_for_profile(profile: &crate::profiles::Profile) -> SafetyReport {
    let mut rows = Vec::new();
    let mut tool_rows = Vec::new();
    let mut critical_blockers = Vec::new();

    let Ok(conn) = crate::db::open_db() else {
        return SafetyReport {
            rows,
            tool_rows,
            critical_blockers,
        };
    };
    let _ = crate::db::ensure_schema(&conn);

    let cutoff = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let total_sessions: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE started_at >= ?1",
            rusqlite::params![cutoff],
            |r| r.get(0),
        )
        .unwrap_or(1)
        .max(1);

    if profile.uses_tool_level() {
        let mut stmt = conn
            .prepare(
                "SELECT tool_name, COUNT(*) AS inv, COUNT(DISTINCT session_id) AS sess \
                 FROM tool_invocations WHERE ts >= ?1 GROUP BY tool_name ORDER BY inv DESC",
            )
            .ok();
        if let Some(ref mut stmt) = stmt {
            if let Ok(iter) = stmt.query_map(rusqlite::params![cutoff], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            }) {
                for row in iter.flatten() {
                    let (tool_name, inv, sess) = row;
                    if profile.keeps_tool(&tool_name) {
                        continue;
                    }
                    let risk = risk_from_usage(inv, sess, total_sessions);
                    let pct = sess as f64 / total_sessions as f64;
                    let short = tool_name
                        .rsplit("__")
                        .next()
                        .unwrap_or(&tool_name)
                        .replace('_', " ");
                    if risk == RiskTier::Critical && inv > 0 {
                        critical_blockers.push(format!(
                            "{short}: used in {sess} sessions in the last 30 days ({pct:.0}% of sessions)"
                        ));
                    }
                    tool_rows.push(ToolSafetyRow {
                        tool_name: tool_name.clone(),
                        invocations_30d: inv,
                        sessions_30d: sess,
                        session_pct: pct,
                        risk,
                        action: action_for_risk(risk),
                    });
                }
            }
        }
        tool_rows.sort_by(|a, b| b.invocations_30d.cmp(&a.invocations_30d));
        return SafetyReport {
            rows,
            tool_rows,
            critical_blockers,
        };
    }

    for (prefix, _) in crate::profiles::SERVER_COUNTS {
        if kept_by_profile(prefix, &profile.keep) {
            continue;
        }
        let inv: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tool_invocations WHERE server_prefix = ?1 AND ts >= ?2",
                rusqlite::params![prefix, cutoff],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let sess: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT session_id) FROM tool_invocations WHERE server_prefix = ?1 AND ts >= ?2",
                rusqlite::params![prefix, cutoff],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let pct = sess as f64 / total_sessions as f64;
        let risk = risk_from_usage(inv, sess, total_sessions);
        let server = prefix
            .trim_start_matches("mcp__claude_ai_")
            .trim_end_matches("__")
            .replace('_', " ");
        if risk == RiskTier::Critical && inv > 0 {
            critical_blockers.push(format!(
                "{server}: used in {sess} sessions in the last 30 days ({pct:.0}% of sessions)"
            ));
        }
        rows.push(ServerSafetyRow {
            server,
            invocations_30d: inv,
            sessions_30d: sess,
            session_pct: pct,
            risk,
            action: action_for_risk(risk),
        });
    }

    rows.sort_by(|a, b| b.invocations_30d.cmp(&a.invocations_30d));
    SafetyReport {
        rows,
        tool_rows,
        critical_blockers,
    }
}

#[derive(Debug, Serialize)]
pub struct QualityAlert {
    #[serde(rename = "type")]
    pub alert_type: String,
    pub profile_change_id: i64,
    pub from_profile: String,
    pub to_profile: String,
    pub correction_rate_before: f64,
    pub correction_rate_after: f64,
    pub recommendation: String,
}

pub fn quality_alerts() -> Result<Vec<QualityAlert>> {
    let mut out = Vec::new();
    // These alerts are about profile filtering removing a tool the agent then missed. With filtering
    // off (the default since ADR 0027), no tools are removed, so the advice to re-enable a server is
    // dead and misleading. Only surface it for users who have opted back into filtering.
    if crate::config::Config::load().filter_mode == crate::config::FilterMode::Off {
        return Ok(out);
    }
    let Ok(conn) = crate::db::open_db() else {
        return Ok(out);
    };
    crate::db::ensure_schema(&conn)?;

    let mut stmt = conn.prepare(
        "SELECT id, ts, from_profile, to_profile, servers_removed FROM profile_changes ORDER BY id DESC LIMIT 12",
    )?;
    let changes: Vec<(i64, String, String, String, String)> = stmt
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<Result<_, _>>()?;

    for (id, _ts, from_p, to_p, removed_json) in changes {
        let anchor: String = conn.query_row(
            "SELECT ts FROM profile_changes WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )?;
        let before = avg_correction_rate(&conn, &anchor, true, 10)?;
        let after = avg_correction_rate(&conn, &anchor, false, 10)?;
        if after - before > 0.05 {
            let removed: Vec<String> = serde_json::from_str(&removed_json).unwrap_or_default();
            let hint = removed.first().cloned().unwrap_or_default();
            out.push(QualityAlert {
                alert_type: "quality_degradation".into(),
                profile_change_id: id,
                from_profile: from_p.clone(),
                to_profile: to_p.clone(),
                correction_rate_before: before,
                correction_rate_after: after,
                recommendation: format!(
                    "Correction rate moved from {:.0}% to {:.0}% after switching from {} to {}. Servers removed included {}. Consider re-enabling a removed MCP server if quality dropped.",
                    before * 100.0,
                    after * 100.0,
                    from_p,
                    to_p,
                    hint
                ),
            });
        }
    }

    Ok(out)
}

fn avg_correction_rate(
    conn: &rusqlite::Connection,
    anchor_ts: &str,
    before: bool,
    n: i64,
) -> Result<f64> {
    let cmp = if before { "<" } else { ">=" };
    let sql = format!(
        "SELECT AVG(cr) FROM (
            SELECT CAST(correction_turns AS REAL) / MAX(turn_count, 1) AS cr
            FROM sessions
            WHERE started_at {cmp} ?1
            ORDER BY started_at DESC
            LIMIT {n}
        )",
        cmp = cmp,
        n = n
    );
    let avg: Option<f64> = conn.query_row(&sql, rusqlite::params![anchor_ts], |r| r.get(0))?;
    Ok(avg.unwrap_or(0.0).clamp(0.0, 1.0))
}
