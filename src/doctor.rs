//! Read-only installation diagnostics.

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

    let StaleScan {
        supervised,
        session,
    } = stale_processes();
    checks.push(if supervised.is_empty() {
        let detail = if session.is_empty() {
            "every ctx process is running its current binary".to_string()
        } else {
            // Not a fault: these belong to editor sessions ctx cannot restart without pulling the
            // tools out from under a live agent. Worth saying, not worth failing over.
            format!(
                "services are current; {} editor-owned `ctx mcp` process(es) still on an older binary, which update when those sessions restart: {}",
                session.len(),
                session.join(", ")
            )
        };
        check("running version", true, detail)
    } else {
        check(
            "running version",
            false,
            format!(
                "{} supervised service(s) running a binary older than the file on disk: {}. These restart themselves within about a minute of an upgrade, so if it persists the supervisor is not restarting them.",
                supervised.len(),
                supervised.join(", ")
            ),
        )
    });

    DoctorReport {
        schema_version: 3,
        healthy: checks.iter().all(|c| c.status == "ok"),
        ctx_version: env!("CARGO_PKG_VERSION"),
        checks,
    }
}

/// ctx processes still executing a binary older than the file they were launched from.
///
/// Each process is judged against *its own* argv[0], not one global path: ctx can be installed in
/// several places at once (Homebrew, ~/.local/bin, cargo), and comparing everything to whichever
/// binary happens to be running `doctor` marks unrelated installs stale.
///
/// Split by who can fix it. A supervised service that is stale is a fault, because it should have
/// restarted itself. A `ctx mcp` server is owned by an editor session, cannot be restarted without
/// pulling tools out from under a live agent, and is expected to lag until that session restarts.
///
/// Uses process start time against binary mtime, which needs only `ps` and reads the same on macOS
/// and Linux; reading another process's executable would mean `lsof` or `/proc` and a different
/// answer per platform. Diagnostics only, and biased toward over-reporting: a false positive costs
/// a restart of a few seconds.
#[derive(Default)]
struct StaleScan {
    supervised: Vec<String>,
    session: Vec<String>,
}

fn stale_processes() -> StaleScan {
    let Ok(output) = std::process::Command::new("ps")
        .args(["-Ao", "pid=,lstart=,args="])
        .output()
    else {
        return StaleScan::default();
    };
    let me = std::process::id();
    let mut scan = StaleScan::default();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        let Some((pid, rest)) = line.split_once(' ') else {
            continue;
        };
        let Ok(pid) = pid.trim().parse::<u32>() else {
            continue;
        };
        if pid == me {
            continue;
        }
        // `ps` prints lstart as a fixed 24-character ctime string, then the command.
        let rest = rest.trim_start();
        if rest.len() < 25 {
            continue;
        }
        let (started, args) = rest.split_at(24);
        let args = args.trim();
        if !is_ctx_command(args) {
            continue;
        }
        let Some(started) = parse_ctime(started.trim()) else {
            continue;
        };
        let Some(exe) = args.split_whitespace().next() else {
            continue;
        };
        // Launched through PATH rather than an absolute path, so there is nothing to stat.
        if !exe.starts_with('/') {
            continue;
        }
        let Ok(built) = std::fs::metadata(exe).and_then(|m| m.modified()) else {
            continue;
        };
        if started >= built {
            continue;
        }
        let subcommand = args.split_whitespace().nth(1).unwrap_or("");
        let entry = format!("pid {pid} ({exe} {subcommand})");
        if subcommand == "mcp" {
            scan.session.push(entry);
        } else {
            scan.supervised.push(entry);
        }
    }
    scan
}

/// Whether a `ps` command line is one of ctx's own processes. Matches on the executable's file name
/// so a path like /opt/homebrew/bin/ctx counts, while an unrelated command that merely mentions ctx
/// in an argument does not.
fn is_ctx_command(args: &str) -> bool {
    args.split_whitespace()
        .next()
        .and_then(|p| p.rsplit('/').next())
        .is_some_and(|name| name == "ctx" || name == "ctx.exe")
}

/// Parse the ctime format `ps` emits for lstart, e.g. "Fri Aug 21 18:24:03 2026".
fn parse_ctime(value: &str) -> Option<std::time::SystemTime> {
    // ps space-pads single-digit days ("Aug  2"), which no single chrono day specifier matches
    // across both widths. Collapse runs of whitespace first and parse one canonical shape.
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let parsed = chrono::NaiveDateTime::parse_from_str(&normalized, "%a %b %d %H:%M:%S %Y").ok()?;
    let local = parsed.and_local_timezone(chrono::Local).single()?;
    Some(std::time::SystemTime::from(
        local.with_timezone(&chrono::Utc),
    ))
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
    fn ctx_commands_are_matched_by_executable_name_not_by_mention() {
        assert!(is_ctx_command(
            "/opt/homebrew/bin/ctx dashboard --port 8789"
        ));
        assert!(is_ctx_command("ctx mcp"));
        // An unrelated process that merely names ctx must not be reported as a stale ctx service.
        assert!(!is_ctx_command(
            "/usr/bin/tail -f /Users/me/.ctx/dashboard.log"
        ));
        assert!(!is_ctx_command("grep ctx src/lib.rs"));
        assert!(!is_ctx_command(""));
    }

    #[test]
    fn ctime_parses_the_format_ps_emits() {
        assert!(parse_ctime("Fri Aug 21 18:24:03 2026").is_some());
        // ps space-pads single-digit days, so the double space has to survive normalization.
        assert!(parse_ctime("Sun Aug  2 09:05:00 2026").is_some());
        assert!(parse_ctime("not a date").is_none());
        // chrono validates the weekday against the date, so a line ps could not have produced is
        // rejected rather than silently parsed into the wrong instant.
        assert!(parse_ctime("Sat Aug  2 09:05:00 2026").is_none());
    }

    #[test]
    fn stale_scan_never_reports_the_calling_process() {
        // Shells out to `ps`, which is slow enough to widen the window in which a neighbouring test
        // has CTX_HOME pointed at its own temp dir. The suite runs with --test-threads=1 in CI, and
        // holding the same lock keeps a local parallel run honest too.
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // inspect() runs inside a ctx process older than nothing; whatever it finds, it must not
        // accuse itself, or `ctx doctor` would always fail.
        let me = format!("pid {} ", std::process::id());
        let scan = stale_processes();
        assert!(!scan.supervised.iter().any(|p| p.starts_with(&me)));
        assert!(!scan.session.iter().any(|p| p.starts_with(&me)));
    }

    #[test]
    fn report_has_stable_machine_readable_shape() {
        let value = serde_json::to_value(inspect()).unwrap();
        assert_eq!(value["schema_version"], 3);
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
