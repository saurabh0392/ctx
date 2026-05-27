use anyhow::{Context, Result};
use colored::Colorize;
use std::io::{stdin, stdout, IsTerminal, Write};

const PLIST_LABEL: &str = "com.ctx.proxy";
const DASHBOARD_PLIST_LABEL: &str = "com.ctx.dashboard";
const INGEST_PLIST_LABEL: &str = "com.ctx.ingest";
const DASHBOARD_PORT: u16 = 8789;

const CURSOR_RULE_MDC: &str = r#"---
description: ctx cost optimization hints for Claude Code sessions.
alwaysApply: true
---

- When a session exceeds 15 turns, suggest starting a fresh session.
- Prefer Sonnet over Opus for standard tasks.
- Break multi-part asks into sequential focused messages.
"#;

fn is_cursor_ide() -> bool {
    std::env::var("CURSOR_TRACE_ID").is_ok()
        || std::env::var("VSCODE_PID").is_ok()
        || dirs::home_dir()
            .map(|h| h.join(".cursor").join("extensions").is_dir())
            .unwrap_or(false)
}

fn ingest_plist_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{INGEST_PLIST_LABEL}.plist"))
}

fn claude_projects_has_jsonl() -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    let projects_dir = home.join(".claude").join("projects");
    let Ok(entries) = std::fs::read_dir(&projects_dir) else {
        return false;
    };
    for proj in entries.flatten() {
        let p = proj.path();
        if !p.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&p) else {
            continue;
        };
        for f in files.flatten() {
            let path = f.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".jsonl") && !name.contains("compact") {
                return true;
            }
        }
    }
    false
}

fn dashboard_plist_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{DASHBOARD_PLIST_LABEL}.plist"))
}

fn plist_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{PLIST_LABEL}.plist"))
}

fn ctx_bin() -> String {
    dirs::home_dir()
        .map(|h| h.join(".cargo").join("bin").join("ctx"))
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "ctx".to_string())
}

#[cfg(target_os = "macos")]
fn claude_desktop_installed() -> bool {
    dirs::home_dir()
        .map(|h| h.join("Library/Application Support/Claude").is_dir())
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn claude_desktop_installed() -> bool {
    false
}

fn ide_kind_label() -> &'static str {
    if is_cursor_ide() {
        "Cursor IDE"
    } else if std::env::var("TERM_PROGRAM").ok().as_deref() == Some("vscode") {
        "VS Code (or compatible shell)"
    } else {
        "Claude Code CLI or other"
    }
}

fn setup_preview_lines(port: u16, upstream: &str) -> Vec<String> {
    let cursor = is_cursor_ide();
    let ingest_tail = if cursor {
        ", periodic ingest (Cursor, every 5 min)"
    } else {
        ""
    };
    vec![
        format!(
            "Create {} and write filter.js, CA cert, and config",
            crate::config::ctx_dir().display()
        ),
        format!(
            "Install launchd agents: proxy (:{port} → {upstream}), dashboard (:{DASHBOARD_PORT}){ingest_tail}"
        ),
        "Merge NODE_OPTIONS into ~/.claude/settings.json (unless --no-install)".to_string(),
        "Index ~/.claude/projects/**/*.jsonl into ~/.ctx/ctx.db when JSONL files exist".to_string(),
        "Choose default MCP profile (personal from history, or carrier)".to_string(),
    ]
}

fn write_cursor_rule_to_home() -> Result<()> {
    let Some(home) = dirs::home_dir() else {
        anyhow::bail!("no home directory");
    };
    let path = home.join(".cursor/rules/ctx.mdc");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, CURSOR_RULE_MDC)?;
    println!("  {} Wrote {}", "✓".green(), path.display());
    Ok(())
}

fn maybe_offer_cursor_rule() -> Result<()> {
    print!("Write Cursor rules at ~/.cursor/rules/ctx.mdc? [y/N] ");
    stdout().flush()?;
    let mut line = String::new();
    stdin().read_line(&mut line)?;
    let t = line.trim().to_lowercase();
    if t == "y" || t == "yes" {
        write_cursor_rule_to_home()?;
    }
    Ok(())
}

pub fn run(
    port: u16,
    upstream: &str,
    no_install: bool,
    _no_zshrc_prompt: bool,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    println!("{} Detected: {}", "i".cyan(), ide_kind_label());
    if claude_desktop_installed() {
        println!(
            "{} Claude Desktop app data found. ctx targets Claude Code in Cursor or a terminal. Desktop is not wired yet.",
            "i".yellow()
        );
    }
    if std::env::var("TERM_PROGRAM").ok().as_deref() == Some("vscode") && !is_cursor_ide() {
        println!(
            "{} VS Code shell. filter.js runs when Claude Code starts Node; VS Code extension support is limited.",
            "i".yellow()
        );
    }
    println!();
    println!("{} ctx will:", "i".cyan());
    for (i, line) in setup_preview_lines(port, upstream).into_iter().enumerate() {
        println!("  {}. {}", i + 1, line);
    }
    println!();
    if dry_run {
        println!("{} Dry run: no changes made.", "i".yellow());
        return Ok(());
    }
    if !yes {
        if !stdin().is_terminal() || !stdout().is_terminal() {
            anyhow::bail!(
                "Non-interactive terminal. Re-run with --yes to confirm without a prompt."
            );
        }
        print!("Proceed? [Y/n] ");
        stdout().flush()?;
        let mut buf = String::new();
        stdin().read_line(&mut buf)?;
        let t = buf.trim().to_lowercase();
        if t == "n" || t == "no" {
            anyhow::bail!("Setup aborted.");
        }
    }

    crate::ensure_tls_crypto_provider();
    crate::config::ensure_dir()?;
    crate::filter_hook::write_filter_js()?;
    crate::ca::ensure_ca()?;

    // Step 1: write launchd plist
    println!("{} Step 1/5: Installing launchd agent (auto-start on login)...", "->".cyan());
    write_plist(port, upstream)?;

    // Step 2: load the agent
    println!("{} Step 2/5: Starting ctx proxy via launchctl...", "->".cyan());
    load_plist()?;
    wait_for_proxy(port)?;

    // Step 3: create default system_prefix.md if missing
    println!("{} Step 3/5: Creating default system_prefix.md...", "->".cyan());
    let prefix_path = crate::config::system_prefix_path();
    if !prefix_path.exists() {
        std::fs::write(&prefix_path, DEFAULT_PREFIX)?;
        println!("  Created {}", prefix_path.display());
    } else {
        println!("  {} (already exists, skipping)", prefix_path.display());
    }

    // Step 3b: download MiniLM model files (onnx feature only, non-blocking)
    #[cfg(feature = "onnx")]
    {
        if !crate::config::minilm_onnx_path().exists() || !crate::config::minilm_tokenizer_path().exists() {
            println!("{} Downloading MiniLM embedding model (~30 MB)...", "->".cyan());
            match download_minilm_model() {
                Ok(()) => println!("  {} Model files saved to {}", "✓".green(), crate::config::models_dir().display()),
                Err(e) => println!("  {} Model download failed (similarity will use hash fallback): {e}", "!".yellow()),
            }
        } else {
            println!("{} MiniLM model already present at {}", "✓".green(), crate::config::models_dir().display());
        }
    }

    // Step 4: default focus profile (personal from history when possible)
    println!("{} Step 4/5: Setting default focus profile...", "->".cyan());
    let _ = crate::db::open_db().and_then(|c| {
        crate::db::ensure_schema(&c)?;
        crate::db::maybe_backfill_requests_from_jsonl(&c)?;
        Ok::<(), anyhow::Error>(())
    });
    if claude_projects_has_jsonl() {
        let _ = crate::conversations::ingest_claude_jsonl();
        if crate::profiles::auto_generate(false).is_ok() {
            let _ = crate::profiles::switch("personal", true);
        } else {
            let _ = crate::profiles::switch("carrier", false);
        }
    } else {
        let _ = crate::profiles::switch("carrier", false);
    }
    crate::filter_hook::sync_filter_config_from_active_config()?;
    let _ = crate::behavior_guard::write_behavior_hints_file();

    // Step 5: start dashboard as background service
    println!("{} Step 5/5: Starting ctx dashboard as background service...", "->".cyan());
    write_dashboard_plist(DASHBOARD_PORT)?;
    load_dashboard_plist()?;

    // Cursor IDE: install periodic ingest since filter.js doesn't run inside Cursor
    let cursor_detected = is_cursor_ide();
    if cursor_detected {
        println!("{} Cursor IDE detected. Installing periodic ingest (every 5 min)...", "->".cyan());
        match write_ingest_plist() {
            Ok(()) => {
                load_ingest_plist()?;
                println!("  {} Ingest agent installed ({INGEST_PLIST_LABEL})", "✓".green());
            }
            Err(e) => println!("  {} Ingest agent failed: {e}. Run `ctx ingest` manually.", "!".yellow()),
        }
    }

    println!();
    println!("{} ctx proxy running via launchd on :{port}", "✓".green().bold());
    println!("{} ctx dashboard running at http://127.0.0.1:{DASHBOARD_PORT}", "✓".green().bold());
    if cursor_detected {
        println!("{} ctx ingest running every 5 min (Cursor mode)", "✓".green().bold());
    }
    println!();

    if no_install {
        println!("{} Skipped writing ~/.claude/settings.json (--no-install)", "i".yellow());
        println!();
        println!("Close Cursor/Claude Code, then run:");
        println!("  ctx proxy install");
        println!("Then reopen Cursor.");
    } else if cursor_detected {
        println!("{} Wiring Claude Code settings (NODE_OPTIONS in-process filter)...", "->".cyan());
        crate::proxy::install(port, upstream)?;
        println!();
        println!("  Cursor mode:  filter.js runs inside Claude Code CLI only.");
        println!("                Cursor sessions are indexed via periodic `ctx ingest`.");
        println!("  Dashboard:    http://127.0.0.1:{DASHBOARD_PORT}");
        println!("  Autorun:      {PLIST_LABEL}, {DASHBOARD_PLIST_LABEL}, {INGEST_PLIST_LABEL}");
        println!("  Inject:       ~/.ctx/system_prefix.md (Claude Code CLI only)");
        println!();
        println!("Dashboard: open http://127.0.0.1:{DASHBOARD_PORT} to see spend, insights, and session similarity.");
    } else {
        println!("{} Wiring Claude Code settings (NODE_OPTIONS in-process filter)...", "->".cyan());
        crate::proxy::install(port, upstream)?;
        println!();
        println!("  Filter:    NODE_OPTIONS loads ~/.ctx/filter.js (strips MCP tools before TLS)");
        println!("  CA:        {} (proxy process only)", crate::ca::ca_cert_path().display());
        println!("  Dashboard: http://127.0.0.1:{DASHBOARD_PORT}");
        println!("  Autorun:   launchd agents {PLIST_LABEL}, {DASHBOARD_PLIST_LABEL}");
        println!("  Inject:    ~/.ctx/system_prefix.md");
        println!();
        println!("Next: restart Claude Code, then run {} to activate filtering.", "`ctx use carrier`".bold());
        println!("Dashboard: open http://127.0.0.1:{DASHBOARD_PORT} to see savings and prompt stats.");
    }

    wire_mcp_server()?;

    let dashboard_url = format!("http://127.0.0.1:{DASHBOARD_PORT}");
    println!();
    print!("{} Setup complete. Waiting for dashboard...", "✓".green().bold());
    let _ = stdout().flush();
    let mut dashboard_ready = false;
    for _ in 0..20 {
        if std::net::TcpStream::connect(format!("127.0.0.1:{DASHBOARD_PORT}")).is_ok() {
            dashboard_ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    if dashboard_ready {
        println!(" Opening {}", dashboard_url);
        let _ = open::that(&dashboard_url);
    } else {
        println!(" Dashboard not ready yet. Open {} manually.", dashboard_url);
    }

    if is_cursor_ide() && stdin().is_terminal() {
        let _ = maybe_offer_cursor_rule();
    }

    Ok(())
}

pub fn uninstall() -> Result<()> {
    // Restore settings.json first so Claude Code is never left pointing at a dead proxy
    crate::proxy::uninstall()?;

    let domain = format!("gui/{}", uid());

    // Remove proxy agent
    let p = plist_path();
    if p.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &domain, p.to_str().unwrap_or("")])
            .status();
        std::fs::remove_file(&p)?;
        println!("{} Removed proxy launchd agent", "✓".green());
    }

    // Remove dashboard agent
    let dp = dashboard_plist_path();
    if dp.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &domain, dp.to_str().unwrap_or("")])
            .status();
        std::fs::remove_file(&dp)?;
        println!("{} Removed dashboard launchd agent", "✓".green());
    }

    // Remove ingest agent
    let ip = ingest_plist_path();
    if ip.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &domain, ip.to_str().unwrap_or("")])
            .status();
        std::fs::remove_file(&ip)?;
        println!("{} Removed ingest launchd agent", "✓".green());
    }

    unwire_mcp_server();

    println!("{} ctx uninstalled. Restart Claude Code to apply.", "✓".green());
    Ok(())
}

fn unwire_mcp_server() {
    let settings_path = crate::config::claude_settings_path();
    if settings_path.exists() {
        if let Ok(text) = std::fs::read_to_string(&settings_path) {
            if let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(servers) = doc.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                    if servers.remove("ctx").is_some() {
                        let _ = crate::config::write_json_atomic(&settings_path, &doc);
                        println!("{} Removed ctx MCP server from {}", "✓".green(), settings_path.display());
                    }
                }
            }
        }
    }
    let cursor_mcp = dirs::home_dir()
        .unwrap_or_default()
        .join(".cursor")
        .join("mcp.json");
    if cursor_mcp.exists() {
        if let Ok(text) = std::fs::read_to_string(&cursor_mcp) {
            if let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(servers) = doc.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                    if servers.remove("ctx").is_some() {
                        let _ = crate::config::write_json_atomic(&cursor_mcp, &doc);
                        println!("{} Removed ctx MCP server from {}", "✓".green(), cursor_mcp.display());
                    }
                }
            }
        }
    }
}

fn wire_mcp_server() -> Result<()> {
    let bin = ctx_bin();

    let settings_path = crate::config::claude_settings_path();
    if settings_path.exists() {
        let text = std::fs::read_to_string(&settings_path).unwrap_or_default();
        let mut doc: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
        let servers = doc.as_object_mut()
            .unwrap()
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(obj) = servers.as_object_mut() {
            obj.insert("ctx".to_string(), serde_json::json!({
                "command": bin,
                "args": ["mcp"],
            }));
        }
        crate::config::write_json_atomic(&settings_path, &doc)?;
        println!("{} Registered ctx MCP server in {}", "✓".green(), settings_path.display());
    }

    if is_cursor_ide() {
        let cursor_mcp = dirs::home_dir()
            .unwrap_or_default()
            .join(".cursor")
            .join("mcp.json");
        let mut doc: serde_json::Value = if cursor_mcp.exists() {
            let text = std::fs::read_to_string(&cursor_mcp).unwrap_or_default();
            serde_json::from_str(&text).unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({})
        };
        let servers = doc.as_object_mut()
            .unwrap()
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(obj) = servers.as_object_mut() {
            obj.insert("ctx".to_string(), serde_json::json!({
                "command": bin,
                "args": ["mcp"],
            }));
        }
        if let Some(parent) = cursor_mcp.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        crate::config::write_json_atomic(&cursor_mcp, &doc)?;
        println!("{} Registered ctx MCP server in {}", "✓".green(), cursor_mcp.display());
    }

    Ok(())
}

fn write_plist(port: u16, upstream: &str) -> Result<()> {
    let bin = ctx_bin();
    let log_dir = crate::config::ctx_dir();
    let stdout_log = log_dir.join("proxy.stdout.log");
    let stderr_log = log_dir.join("proxy.stderr.log");
    let ca_cert = crate::ca::canonical_ca_cert_path_string()?
        .replace('&', "&amp;")
        .replace('<', "&lt;");

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{PLIST_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>proxy</string>
        <string>start</string>
        <string>--port</string>
        <string>{port}</string>
        <string>--upstream</string>
        <string>{upstream}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{home}</string>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:{cargo_bin}</string>
        <key>NODE_EXTRA_CA_CERTS</key>
        <string>{ca_cert}</string>
    </dict>
</dict>
</plist>"#,
        stdout = stdout_log.display(),
        stderr = stderr_log.display(),
        home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default(),
        cargo_bin = dirs::home_dir()
            .map(|h| h.join(".cargo").join("bin").display().to_string())
            .unwrap_or_default(),
    );

    let plist_dir = plist_path().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&plist_dir)?;
    std::fs::write(plist_path(), &plist)?;
    println!("  Written {}", plist_path().display());
    Ok(())
}

fn wait_for_proxy(port: u16) -> Result<()> {
    for _ in 0..20 {
        if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            println!("  Proxy is up on :{port}");
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    anyhow::bail!("Proxy did not start within 10s on port {port}. Check ~/.ctx/proxy.stderr.log")
}

fn uid() -> String {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "501".to_string())
}

fn load_plist() -> Result<()> {
    let p = plist_path();
    let domain = format!("gui/{}", uid());

    // Bootout first (idempotent - ignore errors)
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &domain, p.to_str().unwrap_or("")])
        .status();

    let status = std::process::Command::new("launchctl")
        .args(["bootstrap", &domain, p.to_str().unwrap_or("")])
        .status()
        .context("launchctl bootstrap failed")?;

    if status.success() {
        println!("  launchd agent bootstrapped ({PLIST_LABEL})");
    } else {
        anyhow::bail!(
            "launchctl bootstrap failed. Try manually:\n  launchctl bootstrap {} {}",
            domain,
            p.display()
        );
    }
    Ok(())
}

fn write_dashboard_plist(port: u16) -> Result<()> {
    let bin = ctx_bin();
    let log_dir = crate::config::ctx_dir();
    let stdout_log = log_dir.join("dashboard.stdout.log");
    let stderr_log = log_dir.join("dashboard.stderr.log");

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{DASHBOARD_PLIST_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>dashboard</string>
        <string>--port</string>
        <string>{port}</string>
        <string>--no-open</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{home}</string>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:{cargo_bin}</string>
    </dict>
</dict>
</plist>"#,
        stdout = stdout_log.display(),
        stderr = stderr_log.display(),
        home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default(),
        cargo_bin = dirs::home_dir()
            .map(|h| h.join(".cargo").join("bin").display().to_string())
            .unwrap_or_default(),
    );

    let plist_dir = dashboard_plist_path().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&plist_dir)?;
    std::fs::write(dashboard_plist_path(), &plist)?;
    println!("  Written {}", dashboard_plist_path().display());
    Ok(())
}

fn load_dashboard_plist() -> Result<()> {
    let p = dashboard_plist_path();
    let domain = format!("gui/{}", uid());

    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &domain, p.to_str().unwrap_or("")])
        .status();

    let status = std::process::Command::new("launchctl")
        .args(["bootstrap", &domain, p.to_str().unwrap_or("")])
        .status()
        .context("launchctl bootstrap failed for dashboard")?;

    if status.success() {
        println!("  launchd dashboard bootstrapped ({DASHBOARD_PLIST_LABEL})");
    } else {
        // Non-fatal: proxy is more important; dashboard failure shouldn't block install
        eprintln!("  Warning: dashboard launchd failed to start. Run `ctx dashboard` manually.");
    }
    Ok(())
}

fn write_ingest_plist() -> Result<()> {
    let bin = ctx_bin();
    let log_dir = crate::config::ctx_dir();
    let stdout_log = log_dir.join("ingest.stdout.log");
    let stderr_log = log_dir.join("ingest.stderr.log");

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{INGEST_PLIST_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>ingest</string>
    </array>
    <key>StartInterval</key>
    <integer>300</integer>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{home}</string>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:{cargo_bin}</string>
    </dict>
</dict>
</plist>"#,
        stdout = stdout_log.display(),
        stderr = stderr_log.display(),
        home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default(),
        cargo_bin = dirs::home_dir()
            .map(|h| h.join(".cargo").join("bin").display().to_string())
            .unwrap_or_default(),
    );

    let plist_dir = ingest_plist_path().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&plist_dir)?;
    std::fs::write(ingest_plist_path(), &plist)?;
    println!("  Written {}", ingest_plist_path().display());
    Ok(())
}

fn load_ingest_plist() -> Result<()> {
    let p = ingest_plist_path();
    let domain = format!("gui/{}", uid());

    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &domain, p.to_str().unwrap_or("")])
        .status();

    let status = std::process::Command::new("launchctl")
        .args(["bootstrap", &domain, p.to_str().unwrap_or("")])
        .status()
        .context("launchctl bootstrap failed for ingest")?;

    if !status.success() {
        eprintln!("  Warning: ingest launchd failed to start. Run `ctx ingest` manually.");
    }
    Ok(())
}

#[cfg(feature = "onnx")]
fn download_minilm_model() -> Result<()> {
    let models_dir = crate::config::models_dir();
    std::fs::create_dir_all(&models_dir)?;

    let onnx_path = crate::config::minilm_onnx_path();
    let tok_path = crate::config::minilm_tokenizer_path();

    let onnx_url = "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx";
    let tok_url = "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json";

    if !onnx_path.exists() {
        download_file(onnx_url, &onnx_path)?;
    }
    if !tok_path.exists() {
        download_file(tok_url, &tok_path)?;
    }
    Ok(())
}

#[cfg(feature = "onnx")]
fn download_file(url: &str, dest: &std::path::Path) -> Result<()> {
    let tmp = dest.with_extension("tmp");
    let output = std::process::Command::new("curl")
        .args(["-fSL", "--max-time", "120", "-o"])
        .arg(&tmp)
        .arg(url)
        .output()
        .context("curl not found")?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&tmp);
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("download failed: {stderr}");
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

const DEFAULT_PREFIX: &str = r#"# Workspace Standards

## Code style
- No em dashes in any output
- Concise responses; avoid restating what was just done
- No trailing summaries after completing a task

## Commits
- Conventional commits: feat/fix/refactor/chore
- Co-authored-by footer when using AI assistance

## Reviews
- Flag security issues (injection, auth, secrets in code) before anything else
- Prefer editing existing files over creating new ones
"#;
