use anyhow::Result;
use colored::Colorize;
use std::io::{stdin, stdout, IsTerminal, Write};

const DASHBOARD_PORT: u16 = 8789;

const CURSOR_RULE_MDC: &str = r#"---
description: ctx cost optimization hints for Claude Code sessions.
alwaysApply: true
---

- When a session exceeds 15 turns, suggest starting a fresh session.
- Prefer Sonnet over Opus for standard tasks.
- Break multi-part asks into sequential focused messages.
"#;

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

fn ctx_bin() -> String {
    dirs::home_dir()
        .map(|h| h.join(".cargo").join("bin").join("ctx"))
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "ctx".to_string())
}

fn autorun_summary(periodic_ingest: bool) -> String {
    #[cfg(target_os = "macos")]
    {
        if periodic_ingest {
            format!(
                "{}, {}, {}",
                crate::daemon::PROXY_LABEL,
                crate::daemon::DASHBOARD_LABEL,
                crate::daemon::INGEST_LABEL
            )
        } else {
            format!(
                "{}, {}",
                crate::daemon::PROXY_LABEL,
                crate::daemon::DASHBOARD_LABEL
            )
        }
    }
    #[cfg(target_os = "linux")]
    {
        if periodic_ingest {
            "ctx-proxy.service, ctx-dashboard.service, ctx-ingest.timer".to_string()
        } else {
            "ctx-proxy.service, ctx-dashboard.service".to_string()
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = periodic_ingest;
        "ctx proxy + ctx dashboard (background processes; see ~/.ctx/*.log)".to_string()
    }
}

fn setup_preview_lines(port: u16, upstream: &str, periodic_ingest: bool) -> Vec<String> {
    vec![
        format!(
            "Create {} and write filter.js, CA cert, and config",
            crate::config::ctx_dir().display()
        ),
        crate::daemon::background_services_summary(port, upstream, DASHBOARD_PORT, periodic_ingest),
        "Merge NODE_OPTIONS into ~/.claude/settings.json (unless --no-install)".to_string(),
        "Index ~/.claude/projects/**/*.jsonl and Claude Desktop session logs into ~/.ctx/ctx.db when present"
            .to_string(),
        "Choose default MCP profile (personal from history, or carrier)".to_string(),
    ]
}

fn write_editor_rule(path: &std::path::Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body)?;
    println!("  {} Wrote {}", "✓".green(), path.display());
    Ok(())
}

fn maybe_offer_editor_rule(host: &dyn crate::host::HostAdapter) -> Result<()> {
    if !host.offer_editor_rules() {
        return Ok(());
    }
    let Some(path) = host.editor_rules_path() else {
        return Ok(());
    };
    print!("Write ctx rules at {}? [y/N] ", path.display());
    stdout().flush()?;
    let mut line = String::new();
    stdin().read_line(&mut line)?;
    let t = line.trim().to_lowercase();
    if t == "y" || t == "yes" {
        write_editor_rule(&path, CURSOR_RULE_MDC)?;
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
    let host = crate::host::detect_primary_host();
    println!("{} Detected: {}", "i".cyan(), host.label());
    if crate::config::claude_desktop_installed() {
        println!(
            "{} Claude Desktop detected. ctx registers an MCP server in claude_desktop_config.json. Per-request tracing and hook-driven tool filtering apply to Claude Code (CLI or IDE), not to standalone Desktop chat. Use `ctx ingest` for session-level Desktop data when local-agent logs exist. See README.",
            "i".yellow()
        );
    }
    if host.ide_kind() == Some(crate::host::IdeKind::VsCode) {
        println!(
            "{} VS Code shell. filter.js runs when Claude Code starts Node; VS Code extension support is limited.",
            "i".yellow()
        );
    }
    println!();
    println!("{} ctx will:", "i".cyan());
    for (i, line) in setup_preview_lines(port, upstream, host.needs_periodic_ingest())
        .into_iter()
        .enumerate()
    {
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

    // Step 1: background proxy service (launchd / systemd / spawn)
    println!("{} {}", "->".cyan(), crate::daemon::step1_banner());
    crate::daemon::install_proxy(port, upstream)?;

    // Step 2: start proxy
    println!("{} {}", "->".cyan(), crate::daemon::step2_banner());
    crate::daemon::bootstrap_proxy(port, upstream)?;
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

    // Step 4: generate MCP profiles, then set a sensible default
    println!("{} Step 4/5: Generating MCP profiles and setting default...", "->".cyan());

    // Seed the DB first so generate_from_config sees the user's actual server history.
    let _ = crate::db::open_db().and_then(|c| {
        crate::db::ensure_schema(&c)?;
        crate::db::maybe_backfill_requests_from_jsonl(&c)?;
        Ok::<(), anyhow::Error>(())
    });
    if claude_projects_has_jsonl() {
        let _ = crate::conversations::ingest_claude_jsonl();
    }

    // Try to derive profiles from the user's actual MCP stack (no history required).
    // generate_from_config bails when no servers are discoverable, so we can safely
    // fall back to the history-based path in that case.
    let generated = crate::profiles::generate_from_config().is_ok();
    if generated {
        // Only switch away from "all" when the DB has real request history, meaning the
        // generated profiles are based on what the user actually uses rather than the
        // full SERVER_COUNTS fallback list. For fresh installs, stay on "all" and let
        // the first real session inform the choice.
        let has_history = crate::db::open_db()
            .and_then(|c| crate::db::request_count(&c))
            .unwrap_or(0) > 0;

        if has_history {
            let preferred = ["data", "design", "work", "finance", "files", "infra", "shippo", "comms", "other"];
            let custom_path = crate::config::ctx_dir().join("profiles.toml");
            if let Ok(content) = std::fs::read_to_string(&custom_path) {
                if let Ok(profiles) = toml::from_str::<std::collections::HashMap<String, crate::profiles::Profile>>(&content) {
                    for slug in &preferred {
                        if profiles.contains_key(*slug) {
                            let _ = crate::profiles::switch(slug, true);
                            println!("  {} Active profile: {}  (switch any time with `ctx use <profile>`)", "✓".green(), slug);
                            break;
                        }
                    }
                }
            }
        } else {
            // No history yet: profiles were written but stay on "all" until the user
            // has run a real session. ctx profile list shows what's available.
            let _ = crate::profiles::switch("all", false);
            println!("  {} Profiles generated. Staying on 'all' for now — run `ctx profile list` after", "✓".green());
            println!("      your first session, then `ctx use <profile>` to activate the right one.");
        }
    } else {
        // Fallback: history-based personal profile, or carrier as last resort.
        if claude_projects_has_jsonl() {
            if crate::profiles::auto_generate(false).is_ok() {
                let _ = crate::profiles::switch("personal", true);
            } else {
                let _ = crate::profiles::switch("carrier", false);
            }
        } else {
            let _ = crate::profiles::switch("carrier", false);
        }
    }
    crate::filter_hook::sync_filter_config_from_active_config()?;
    let _ = crate::behavior_guard::write_behavior_hints_file();

    // Step 5: dashboard background service
    println!("{} {}", "->".cyan(), crate::daemon::step5_banner());
    crate::daemon::install_dashboard(DASHBOARD_PORT)?;
    crate::daemon::bootstrap_dashboard(DASHBOARD_PORT)?;

    // Periodic ingest for IDEs where filter.js does not cover every path, plus Desktop session logs.
    let periodic_ingest = host.needs_periodic_ingest();
    if periodic_ingest {
        println!(
            "{} Periodic ingest (every 5 min): indexing Claude Code JSONL and Desktop sessions…",
            "->".cyan()
        );
        match crate::daemon::install_periodic_ingest() {
            Ok(()) => {
                let _ = crate::daemon::bootstrap_ingest();
                println!("  {} Periodic ingest installed", "✓".green());
            }
            Err(e) => println!(
                "  {} Ingest scheduler failed: {e}. Run `ctx ingest` manually.",
                "!".yellow()
            ),
        }
    }

    println!();
    #[cfg(target_os = "macos")]
    println!("{} ctx proxy running via launchd on :{port}", "✓".green().bold());
    #[cfg(target_os = "linux")]
    println!(
        "{} ctx proxy running via systemd user service on :{port}",
        "✓".green().bold()
    );
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    println!("{} ctx proxy running on :{port}", "✓".green().bold());
    println!("{} ctx dashboard running at http://127.0.0.1:{DASHBOARD_PORT}", "✓".green().bold());
    if periodic_ingest {
        println!("{} ctx ingest running every 5 min", "✓".green().bold());
    }
    println!();

    if no_install {
        println!("{} Skipped writing ~/.claude/settings.json (--no-install)", "i".yellow());
        println!();
        println!("Run:");
        println!("  ctx proxy install");
        println!("Then reload the window: Cmd+Shift+P (macOS) or Ctrl+Shift+P (Windows/Linux), type Reload Window, Enter.");
        println!("This picks up NODE_OPTIONS and MCP server config without quitting.");
    } else if host.ide_kind() == Some(crate::host::IdeKind::Cursor) {
        println!("{} Wiring Claude Code settings (NODE_OPTIONS in-process filter)...", "->".cyan());
        crate::proxy::install(port, upstream)?;
        println!();
        println!("  Cursor mode:  filter.js runs inside Claude Code CLI only.");
        println!("                Sessions are indexed via periodic `ctx ingest` (Claude Code + Desktop).");
        println!("  Dashboard:    http://127.0.0.1:{DASHBOARD_PORT}");
        println!("  Autorun:      {}", autorun_summary(periodic_ingest));
        println!("  Inject:       ~/.ctx/system_prefix.md (Claude Code CLI only)");
        println!();
        println!("Dashboard: open http://127.0.0.1:{DASHBOARD_PORT} to see spend, insights, and session similarity.");
    } else if host.ide_kind().is_some() {
        println!("{} Wiring Claude Code settings (NODE_OPTIONS in-process filter)...", "->".cyan());
        crate::proxy::install(port, upstream)?;
        println!();
        println!("  IDE mode:     filter.js runs when Claude Code starts Node.");
        println!("                Sessions are indexed via periodic `ctx ingest` when enabled.");
        println!("  Dashboard:    http://127.0.0.1:{DASHBOARD_PORT}");
        println!("  Autorun:      {}", autorun_summary(periodic_ingest));
        println!("  Inject:       ~/.ctx/system_prefix.md (Claude Code CLI only)");
        println!();
        println!("Dashboard: open http://127.0.0.1:{DASHBOARD_PORT} to see spend, insights, and session similarity.");
    } else if !host.supports_node_options() {
        println!("{} Claude Desktop wiring (no NODE_OPTIONS)...", "->".cyan());
        println!();
        println!("  MCP tools:  registered in claude_desktop_config.json (quit + reopen to activate)");
        println!("  Dashboard:  http://127.0.0.1:{DASHBOARD_PORT}");
        println!("  Autorun:    {}", autorun_summary(periodic_ingest));
        println!();
        println!("  Tool filtering (NODE_OPTIONS / filter.js) is not supported for Claude Desktop alone.");
        println!("  Request tracing, savings tracking from the hook, and per-request analytics need Claude Code (CLI or IDE).");
        println!("  Desktop still gets MCP tools and session data via `ctx ingest` when local-agent logs exist.");
        println!("  Install the Claude Code CLI for full token savings and Request Trace coverage.");
    } else {
        println!("{} Wiring Claude Code settings (NODE_OPTIONS in-process filter)...", "->".cyan());
        crate::proxy::install(port, upstream)?;
        println!();
        println!("  Filter:    NODE_OPTIONS loads ~/.ctx/filter.js (strips MCP tools before TLS)");
        println!("  CA:        {} (proxy process only)", crate::ca::ca_cert_path().display());
        println!("  Dashboard: http://127.0.0.1:{DASHBOARD_PORT}");
        println!("  Autorun:      {}", autorun_summary(periodic_ingest));
        println!("  Inject:    ~/.ctx/system_prefix.md");
        println!();
        println!("Dashboard: open http://127.0.0.1:{DASHBOARD_PORT} to see savings and prompt stats.");
    }

    wire_mcp_server(&*host)?;

    if !no_install {
        println!();
        println!("Next: {}", host.reload_instruction());
        if host.supports_node_options() {
            println!("This re-reads NODE_OPTIONS and MCP server config without quitting.");
            if host.ide_kind().is_none() {
                println!("If you only use Claude Code in a plain terminal, start a new session once so NODE_OPTIONS applies.");
                println!("Then run {} to activate filtering.", "`ctx use carrier`".bold());
            }
            if crate::config::claude_desktop_installed() {
                println!(
                    "Claude Desktop: quit the app completely and reopen it so MCP changes in claude_desktop_config.json apply."
                );
            }
        }
    }

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

    if host.offer_editor_rules() && stdin().is_terminal() {
        let _ = maybe_offer_editor_rule(&*host);
    }

    Ok(())
}

pub fn uninstall() -> Result<()> {
    // Restore settings.json first so Claude Code is never left pointing at a dead proxy
    crate::proxy::uninstall()?;

    crate::daemon::uninstall_all()?;

    unwire_mcp_server();

    let host = crate::host::detect_primary_host();
    println!(
        "{} ctx uninstalled. {}",
        "✓".green(),
        host.reload_instruction()
    );
    if host.supports_node_options() {
        println!("Apply the reload so removed NODE_OPTIONS and MCP server config take effect.");
    }
    Ok(())
}

fn unwire_mcp_server() {
    let settings_path = crate::config::claude_settings_path();
    if settings_path.exists() {
        if let Ok(text) = std::fs::read_to_string(&settings_path) {
            if let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(&text) {
                if crate::config::remove_ctx_from_mcp_servers(&mut doc) {
                    let _ = crate::config::write_json_atomic(&settings_path, &doc);
                    println!("{} Removed ctx MCP server from {}", "✓".green(), settings_path.display());
                }
            }
        }
    }
    let cursor_mcp = dirs::home_dir()
        .unwrap_or_default()
        .join(".cursor")
        .join("mcp.json");
    let windsurf_mcp = dirs::home_dir()
        .unwrap_or_default()
        .join(".codeium")
        .join("windsurf")
        .join("mcp_config.json");
    for mcp_path in [cursor_mcp, windsurf_mcp] {
        if mcp_path.exists() {
            if let Ok(text) = std::fs::read_to_string(&mcp_path) {
                if let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(&text) {
                    if crate::config::remove_ctx_from_mcp_servers(&mut doc) {
                        let _ = crate::config::write_json_atomic(&mcp_path, &doc);
                        println!("{} Removed ctx MCP server from {}", "✓".green(), mcp_path.display());
                    }
                }
            }
        }
    }
    if let Some(desktop_cfg) = crate::config::claude_desktop_config_path() {
        if desktop_cfg.exists() {
            if let Ok(text) = std::fs::read_to_string(&desktop_cfg) {
                if let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(&text) {
                    if crate::config::remove_ctx_from_mcp_servers(&mut doc) {
                        let _ = crate::config::write_json_atomic(&desktop_cfg, &doc);
                        println!("{} Removed ctx MCP server from {}", "✓".green(), desktop_cfg.display());
                    }
                }
            }
        }
    }
}

fn wire_mcp_server(host: &dyn crate::host::HostAdapter) -> Result<()> {
    let bin = ctx_bin();

    let settings_path = crate::config::claude_settings_path();
    if settings_path.exists() {
        let text = std::fs::read_to_string(&settings_path).unwrap_or_default();
        let mut doc: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
        crate::config::merge_ctx_into_mcp_servers(&mut doc, &bin)?;
        crate::config::write_json_atomic(&settings_path, &doc)?;
        println!("{} Registered ctx MCP server in {}", "✓".green(), settings_path.display());
    }

    for extra in host.mcp_extra_config_paths() {
        let mut doc: serde_json::Value = if extra.exists() {
            let text = std::fs::read_to_string(&extra).unwrap_or_default();
            serde_json::from_str(&text).unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({})
        };
        crate::config::merge_ctx_into_mcp_servers(&mut doc, &bin)?;
        if let Some(parent) = extra.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        crate::config::write_json_atomic(&extra, &doc)?;
        println!("{} Registered ctx MCP server in {}", "✓".green(), extra.display());
    }

    if crate::config::claude_desktop_installed() {
        if let Some(desktop_cfg) = crate::config::claude_desktop_config_path() {
            let mut doc: serde_json::Value = if desktop_cfg.exists() {
                let text = std::fs::read_to_string(&desktop_cfg).unwrap_or_default();
                serde_json::from_str(&text).unwrap_or(serde_json::json!({}))
            } else {
                serde_json::json!({})
            };
            crate::config::merge_ctx_into_mcp_servers(&mut doc, &bin)?;
            if let Some(parent) = desktop_cfg.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            crate::config::write_json_atomic(&desktop_cfg, &doc)?;
            println!("{} Registered ctx MCP server in {}", "✓".green(), desktop_cfg.display());
        }
    }

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
