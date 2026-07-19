//! Read-only installation diagnostics for beta support.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DoctorCheck {
    pub name: &'static str,
    pub status: &'static str,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub healthy: bool,
    pub ctx_version: &'static str,
    pub checks: Vec<DoctorCheck>,
}

fn check(name: &'static str, ok: bool, detail: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        name,
        status: if ok { "ok" } else { "error" },
        detail: detail.into(),
        state: None,
    }
}

fn settings_has(needle: &str) -> bool {
    std::fs::read_to_string(crate::config::claude_settings_path())
        .map(|s| s.contains(needle))
        .unwrap_or(false)
}

pub fn inspect() -> DoctorReport {
    let mut checks = Vec::new();
    let cfg_path = crate::config::ctx_dir().join("config.toml");
    checks.push(check(
        "config",
        cfg_path.is_file(),
        cfg_path.display().to_string(),
    ));

    match crate::db::inspect_schema_version() {
        Ok(version) if version == crate::db::SCHEMA_VERSION => checks.push(check(
            "database",
            true,
            format!("{} (schema {version})", crate::config::db_path().display()),
        )),
        Ok(version) => checks.push(check(
            "database",
            false,
            format!(
                "{} uses schema {version}; expected {} (run ctx setup)",
                crate::config::db_path().display(),
                crate::db::SCHEMA_VERSION
            ),
        )),
        Err(e) => checks.push(check("database", false, e.to_string())),
    }

    let current = std::env::current_exe();
    checks.push(match current {
        Ok(path) => check("binary", path.is_file(), path.display().to_string()),
        Err(e) => check("binary", false, e.to_string()),
    });

    let claude = std::process::Command::new("claude")
        .arg("--version")
        .output();
    checks.push(match claude {
        Ok(out) if out.status.success() => check(
            "claude_code",
            true,
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        ),
        Ok(out) => check(
            "claude_code",
            false,
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ),
        Err(e) => check("claude_code", false, e.to_string()),
    });

    checks.push(check(
        "claude_hooks",
        settings_has("UserPromptSubmit") && settings_has("PostToolUse"),
        crate::config::claude_settings_path().display().to_string(),
    ));
    checks.push(check(
        "ctx_mcp",
        settings_has("mcpServers") && settings_has("ctx"),
        "ctx recovery server registered in Claude settings",
    ));

    match crate::mcp_gateway::registry::GatewayRegistry::load() {
        Ok(registry) => {
            let mut item = check(
                "mcp_gateway",
                true,
                format!(
                    "{} approved destination(s); {} Codex server(s) routed",
                    registry.servers.len(),
                    registry.codex_backups.len()
                ),
            );
            item.state = Some(
                if registry.servers.is_empty() {
                    "not_configured"
                } else {
                    "configured"
                }
                .into(),
            );
            checks.push(item);
        }
        Err(error) => checks.push(check("mcp_gateway", false, error.to_string())),
    }

    let codex = std::process::Command::new("codex")
        .arg("--version")
        .output();
    match codex {
        Err(_) => {
            let mut item = check("codex_plugin", true, "Codex is not installed");
            item.state = Some("not_installed".into());
            checks.push(item);
        }
        Ok(version) if version.status.success() => {
            let version_text = String::from_utf8_lossy(&version.stdout).trim().to_string();
            let list = std::process::Command::new("codex")
                .args(["plugin", "list", "--json"])
                .output();
            let installed = list
                .as_ref()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
                .and_then(|v| v.get("installed").and_then(|v| v.as_array()).cloned())
                .map(|rows| {
                    rows.iter().any(|row| {
                        row.get("name").and_then(|v| v.as_str()) == Some("ctx")
                            || row.to_string().contains("ctx@ctx")
                    })
                })
                .unwrap_or(false);
            let (heartbeat, acted) = inspect_codex_activity();
            let state = if !installed {
                "not_installed"
            } else if !heartbeat {
                "awaiting_hook_trust"
            } else if acted > 0 || crate::mcp_gateway::registry::codex_gateway_server_count() > 0 {
                "partially_active"
            } else {
                "observing"
            };
            let ok = installed && heartbeat;
            let mut item = check(
                "codex_plugin",
                ok,
                format!(
                    "{version_text}; {state}; {} MCP server(s) routed through CTX",
                    crate::mcp_gateway::registry::codex_gateway_server_count()
                ),
            );
            item.state = Some(state.into());
            checks.push(item);
        }
        Ok(version) => {
            let mut item = check(
                "codex_plugin",
                false,
                String::from_utf8_lossy(&version.stderr).trim().to_string(),
            );
            item.state = Some("incompatible".into());
            checks.push(item);
        }
    }

    let cfg = crate::config::Config::load();
    let port = cfg.dashboard_port.unwrap_or(8789);
    let dashboard_up = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
    checks.push(check(
        "dashboard",
        dashboard_up,
        format!("http://127.0.0.1:{port}"),
    ));

    let beta = crate::beta::load_state();
    checks.push(match beta {
        Some(state) => match crate::beta::capability_details(&state.credential) {
            Some((participant, expiry)) if expiry > chrono::Utc::now() => check(
                "beta_capability",
                participant == state.participant_id,
                format!(
                    "participant {} ({}; expires {})",
                    state.participant_id,
                    state.release_channel,
                    expiry.format("%Y-%m-%d")
                ),
            ),
            Some((_, expiry)) => check(
                "beta_capability",
                false,
                format!(
                    "expired {}; reinstall with a current invite",
                    expiry.format("%Y-%m-%d")
                ),
            ),
            None => check(
                "beta_capability",
                false,
                "scoped capability is missing or malformed; reinstall with a current invite",
            ),
        },
        None => check("beta_capability", true, "not enrolled (standard install)"),
    });

    DoctorReport {
        schema_version: 2,
        healthy: checks.iter().all(|c| c.status == "ok"),
        ctx_version: env!("CARGO_PKG_VERSION"),
        checks,
    }
}

/// Read-only proof that a trusted Codex hook has actually run. Plugin installation by itself does
/// not count as observing.
fn inspect_codex_activity() -> (bool, i64) {
    use rusqlite::{Connection, OpenFlags};
    let path = crate::config::db_path();
    let Ok(conn) = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return (false, 0);
    };
    let heartbeat = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM surface_hook_events WHERE surface='codex')",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        == 1;
    let acted = conn
        .query_row(
            "SELECT COUNT(*) FROM compress_decisions WHERE surface='codex' AND applied=1",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);
    (heartbeat, acted)
}

pub fn run(json: bool) -> anyhow::Result<()> {
    let report = inspect();
    let healthy = report.healthy;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "ctx doctor {}\n",
            if healthy {
                "found a healthy install"
            } else {
                "found issues"
            }
        );
        for item in report.checks {
            let mark = if item.status == "ok" { "✓" } else { "!" };
            println!("  {mark} {:<18} {}", item.name, item.detail);
        }
    }
    if !healthy {
        anyhow::bail!("one or more installation checks failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_has_stable_machine_readable_shape() {
        let value = serde_json::to_value(inspect()).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert!(value["healthy"].is_boolean());
        assert!(value["checks"].is_array());
    }

    #[test]
    fn inspect_does_not_create_a_database_for_a_fresh_install() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temporary = tempfile::tempdir().unwrap();
        let previous = std::env::var("CTX_HOME").ok();
        std::env::set_var("CTX_HOME", temporary.path());

        let report = inspect();

        assert!(!crate::config::db_path().exists());
        assert_eq!(
            report
                .checks
                .iter()
                .find(|item| item.name == "database")
                .map(|item| item.status),
            Some("error")
        );
        match previous {
            Some(value) => std::env::set_var("CTX_HOME", value),
            None => std::env::remove_var("CTX_HOME"),
        }
    }
}
