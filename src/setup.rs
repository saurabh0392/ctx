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
    crate::config::claude_projects_has_jsonl()
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
                "{}, {}",
                crate::daemon::DASHBOARD_LABEL,
                crate::daemon::INGEST_LABEL
            )
        } else {
            crate::daemon::DASHBOARD_LABEL.to_string()
        }
    }
    #[cfg(target_os = "linux")]
    {
        if periodic_ingest {
            "ctx-dashboard.service, ctx-ingest.timer".to_string()
        } else {
            "ctx-dashboard.service".to_string()
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = periodic_ingest;
        "ctx dashboard (background process; see ~/.ctx/*.log)".to_string()
    }
}

fn setup_preview_lines(periodic_ingest: bool) -> Vec<String> {
    vec![
        format!(
            "Create {} and write config (MCP filtering off by default)",
            crate::config::ctx_dir().display()
        ),
        crate::daemon::dashboard_ingest_summary(DASHBOARD_PORT, periodic_ingest),
        "Merge ctx hooks into ~/.claude/settings.json, with no MCP filter rules (unless --no-install)"
            .to_string(),
        "Index ~/.claude/projects/**/*.jsonl into ~/.ctx/ctx.db so the dashboard and learning have history"
            .to_string(),
        "Keep all tools available; profile filtering stays an opt-in (`ctx use <profile>`)".to_string(),
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

pub fn run(no_install: bool, _no_zshrc_prompt: bool, dry_run: bool, yes: bool) -> Result<()> {
    let host = crate::host::detect_primary_host();
    println!("{} Detected: {}", "i".cyan(), host.label());
    if crate::config::claude_desktop_installed() {
        println!(
            "{} Claude Desktop detected. ctx registers an MCP server in claude_desktop_config.json. Per-request tracing and filter.js-driven savings apply to Claude Code (CLI or IDE), not to standalone Desktop chat. Use `ctx ingest` for session-level Desktop data when local-agent logs exist. See README.",
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
    for (i, line) in setup_preview_lines(host.needs_periodic_ingest())
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

    crate::config::ensure_dir()?;
    if crate::experiment_plan::restore_experiment_state_if_missing()? {
        println!(
            "{} Restored experiment plan from persistent backup (survives rm -rf ~/.ctx)",
            "✓".green()
        );
        match crate::experiment_plan::ensure_pending_phase_applied() {
            Ok(true) => println!(
                "  Applied pending experiment phase from calendar (run `ctx experiment plan status`)"
            ),
            Ok(false) => {}
            Err(e) => println!(
                "  {} Could not apply experiment phase: {e}",
                "!".yellow()
            ),
        }
    }
    crate::filter_hook::write_filter_js()?;

    // Fresh installs ship with profile filtering off (CTX-43, ADR 0027). ctx earns its keep with
    // proof-gated output trimming and the cross-surface view, not by stripping MCP tools on a
    // heuristic. Filtering stays available as an opt-in; it is just not on by default.
    {
        let cfg_path = crate::config::ctx_dir().join("config.toml");
        if !cfg_path.exists() {
            let mut cfg = crate::config::Config::load();
            cfg.filter_mode = crate::config::FilterMode::Off;
            cfg.active_profile = Some("all".to_string());
            cfg.auto_profile_enabled = false;
            cfg.dashboard_port = Some(DASHBOARD_PORT);
            cfg.experiment_hooks_enabled = true;
            let _ = cfg.save();
        }
    }

    // ctx is hook-first: no proxy, no MITM. MCP filtering runs through Claude Code permission
    // rules (permissions.deny), and everything else happens in hooks.
    // Step 1: create default system_prefix.md if missing
    println!(
        "{} Step 1/4: Creating default system_prefix.md...",
        "->".cyan()
    );
    let prefix_path = crate::config::system_prefix_path();
    if !prefix_path.exists() {
        std::fs::write(&prefix_path, DEFAULT_PREFIX)?;
        println!("  Created {}", prefix_path.display());
    } else {
        println!("  {} (already exists, skipping)", prefix_path.display());
    }

    // Step 1b: download MiniLM model files (onnx feature only, non-blocking)
    #[cfg(feature = "onnx")]
    {
        if !crate::config::minilm_onnx_path().exists()
            || !crate::config::minilm_tokenizer_path().exists()
        {
            println!(
                "{} Downloading MiniLM embedding model (~30 MB)...",
                "->".cyan()
            );
            match download_minilm_model() {
                Ok(()) => println!(
                    "  {} Model files saved to {}",
                    "✓".green(),
                    crate::config::models_dir().display()
                ),
                Err(e) => println!(
                    "  {} Model download failed (similarity will use hash fallback): {e}",
                    "!".yellow()
                ),
            }
        } else {
            println!(
                "{} MiniLM model already present at {}",
                "✓".green(),
                crate::config::models_dir().display()
            );
        }
    }

    // Step 2: ingest session history, generate profiles from MCP usage, pick default
    println!(
        "{} Step 2/4: Indexing history and building MCP profiles...",
        "->".cyan()
    );

    let _ = crate::db::open_db().and_then(|c| {
        crate::db::ensure_schema(&c)?;
        crate::db::maybe_backfill_requests_from_jsonl(&c)?;
        Ok::<(), anyhow::Error>(())
    });
    if claude_projects_has_jsonl() {
        match crate::conversations::ingest_claude_jsonl() {
            Ok(n) if n > 0 => println!("  Ingested {n} session file(s)"),
            Ok(_) => {}
            Err(e) => println!("  {} Ingest skipped: {e}", "!".yellow()),
        }
    }

    if let Ok(conn) = crate::db::open_db() {
        if crate::db::maybe_reset_stale_install_watermark(&conn).unwrap_or(false) {
            println!(
                "{} Cleared stale install watermark (indexed sessions predate reinstall)",
                "✓".green()
            );
        }
    }

    let _ = crate::profiles::bootstrap_from_history(false)?;
    crate::filter_hook::sync_filter_config_from_active_config()?;
    let _ = crate::behavior_guard::write_behavior_hints_file();

    // Step 3: dashboard background service
    println!("{} Step 3/4: Installing dashboard...", "->".cyan());
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
    println!(
        "{} ctx dashboard running at http://127.0.0.1:{DASHBOARD_PORT}",
        "✓".green().bold()
    );
    if periodic_ingest {
        println!("{} ctx ingest running every 5 min", "✓".green().bold());
    }
    println!();

    if no_install {
        println!(
            "{} Skipped writing ~/.claude/settings.json (--no-install)",
            "i".yellow()
        );
        println!();
        println!("Run:");
        println!("  ctx use <profile>");
        println!("Then reload Claude Code so hooks and filter rules apply.");
    } else if host.supports_node_options() {
        println!("{} Wiring Claude Code (hooks)...", "->".cyan());
        let cfg = crate::config::Config::load();
        let slug = cfg.active_profile.as_deref().unwrap_or("all");
        crate::config::install_statusline_script(DASHBOARD_PORT)?;
        crate::claude_settings::write_native_ctx_to_user_settings(slug, DASHBOARD_PORT)?;
        let _ = crate::claude_settings::sync_experiment_hooks_from_config();
        println!();
        let filter_line = match cfg.filter_mode {
            crate::config::FilterMode::Off => {
                "off by default (no MCP rules; every tool stays available)".to_string()
            }
            crate::config::FilterMode::Soft => {
                "soft (permissions.deny — MCP servers stay connected)".to_string()
            }
            crate::config::FilterMode::Strict => {
                "strict (non-allowlisted MCP servers disconnected)".to_string()
            }
        };
        println!("  Filter:     {filter_line}");
        println!("  Hooks:      UserPromptSubmit + dashboard telemetry");
        println!("  Allowance:  statusLine → dashboard (Pro/Max rate limits)");
        println!("  Dashboard:  http://127.0.0.1:{DASHBOARD_PORT}");
        println!("  Autorun:    {}", autorun_summary(periodic_ingest));
        println!("  Filtering:  opt-in if you want it (`ctx use <profile>`); off by default");
        println!();
        println!("Dashboard: open http://127.0.0.1:{DASHBOARD_PORT} to see spend, insights, and session similarity.");
    } else {
        println!(
            "{} Claude Desktop wiring (analytics + MCP only)...",
            "->".cyan()
        );
        println!();
        println!(
            "  MCP tools:  registered in claude_desktop_config.json (quit + reopen to activate)"
        );
        println!("  Dashboard:  http://127.0.0.1:{DASHBOARD_PORT}");
        println!("  Autorun:    {}", autorun_summary(periodic_ingest));
        println!();
        println!(
            "  Optional MCP filtering requires Claude Code (CLI or IDE). Desktop gets ingest + dashboard."
        );
        println!("  Install Claude Code and re-run `ctx setup` for hooks (filtering stays opt-in).");
    }

    wire_mcp_server(&*host)?;

    if !no_install {
        register_cursor_hook_if_present();
    }

    if !no_install {
        println!();
        println!("Next: {}", host.reload_instruction());
        if host.supports_node_options() {
            println!("This re-reads ~/.claude/settings.json without quitting.");
            if host.ide_kind().is_none() {
                println!("Reload the Claude Code window once after install so hooks apply.");
                println!("Then run {} to pick a profile.", "`ctx use carrier`".bold());
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
    print!(
        "{} Setup complete. Waiting for dashboard...",
        "✓".green().bold()
    );
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

/// Register the live Cursor postToolUse hook in `~/.cursor/hooks.json`, but only when Cursor is
/// actually present (a `~/.cursor` directory exists). We never create that directory for users who
/// do not run Cursor. Best-effort: a failure here must not abort setup. (ADR 0018 / CTX-27)
fn register_cursor_hook_if_present() {
    let present = dirs::home_dir()
        .map(|h| h.join(".cursor").exists())
        .unwrap_or(false);
    if !present {
        return;
    }
    match crate::cursor_hooks::write_ctx_cursor_hook() {
        Ok(()) => {
            println!("  Cursor:     live hooks in ~/.cursor/hooks.json (postToolUse trims MCP results and watches built-ins; preToolUse runs earned shell commands through ctx run; preCompact records compactions)");
        }
        Err(e) => {
            println!("{} Cursor hook registration skipped: {e}", "!".yellow());
        }
    }
}

pub fn uninstall() -> Result<()> {
    if let Err(e) = crate::experiment_plan::backup_experiment_state() {
        println!("{} Experiment backup skipped: {e}", "!".yellow());
    } else if crate::experiment_plan::plan_path().is_file() {
        println!(
            "{} Experiment plan backed up to {}",
            "✓".green(),
            crate::experiment_plan::persistent_experiment_dir().display()
        );
    }

    // Legacy cleanup: older versions could opt into a MITM proxy that wired env vars and a
    // launch agent. The proxy is gone (ADR 0015), but strip any leftovers so we never strand a
    // machine that pointed Claude Code at a now-dead proxy.
    remove_legacy_proxy_artifacts();

    crate::daemon::uninstall_all()?;

    unwire_mcp_server();

    strip_claude_settings_hooks_if_present()?;

    match crate::cursor_hooks::remove_ctx_cursor_hook() {
        Ok(true) => println!("{} Removed ctx hook from ~/.cursor/hooks.json", "✓".green()),
        Ok(false) => {}
        Err(e) => println!(
            "{} Cursor hook removal skipped (edit ~/.cursor/hooks.json if needed): {}",
            "!".yellow(),
            e
        ),
    }

    match crate::config::remove_user_ctx_from_cursor_known_mcp_ids() {
        Ok(true) => println!(
            "{} Cleared stale ctx entry from Cursor MCP cache",
            "✓".green()
        ),
        Ok(false) => {}
        Err(e) => println!(
            "{} Cursor MCP cache cleanup skipped (close Cursor and retry if needed): {}",
            "!".yellow(),
            e
        ),
    }

    println!(
        "{} ctx uninstalled. {}",
        "✓".green(),
        crate::host::uninstall_reload_hint()
    );
    println!("Apply the reload so removed hooks, filter rules, and MCP server config take effect.");
    Ok(())
}

/// Remove leftovers from the retired MITM proxy (ADR 0015): the ctx-owned proxy env vars in
/// Claude Code settings and the legacy `com.ctx.proxy` launch agent. Best effort; only touches
/// values that look ctx-owned (localhost proxy or a path under ~/.ctx).
fn remove_legacy_proxy_artifacts() {
    let path = crate::config::claude_settings_path();
    if path.is_file() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(&text) {
                if crate::claude_settings::strip_ctx_proxy_env(&mut doc) {
                    let _ = crate::config::write_json_atomic(&path, &doc);
                    println!(
                        "{} Removed legacy ctx proxy env from {}",
                        "✓".green(),
                        path.display()
                    );
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = dirs::home_dir() {
        let plist = home
            .join("Library")
            .join("LaunchAgents")
            .join("com.ctx.proxy.plist");
        if plist.exists() {
            let _ = std::process::Command::new("launchctl")
                .args([
                    "bootout",
                    &format!(
                        "gui/{}",
                        std::process::Command::new("id")
                            .arg("-u")
                            .output()
                            .ok()
                            .and_then(|o| String::from_utf8(o.stdout).ok())
                            .map(|s| s.trim().to_string())
                            .unwrap_or_else(|| "501".to_string())
                    ),
                    plist.to_str().unwrap_or(""),
                ])
                .status();
            let _ = std::fs::remove_file(&plist);
            println!("{} Removed legacy com.ctx.proxy launch agent", "✓".green());
        }
    }
}

fn strip_claude_settings_hooks_if_present() -> Result<()> {
    let path = crate::config::claude_settings_path();
    if !path.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&path)?;
    let mut doc: serde_json::Value = serde_json::from_str(&text)?;
    let legacy = crate::config::strip_ctx_managed_hooks_from_settings(&mut doc);
    let native = crate::claude_settings::strip_ctx_native_hooks_from_settings(&mut doc);
    let statusline = crate::claude_settings::strip_ctx_statusline(&mut doc);
    let node = crate::claude_settings::strip_ctx_filter_from_node_options_in_settings(&mut doc);
    let deny = crate::claude_settings::strip_ctx_deny_rules(&mut doc);
    let allow = crate::claude_settings::strip_allowed_mcp_servers(&mut doc);
    if legacy || native || statusline || node || deny || allow {
        crate::config::write_json_atomic(&path, &doc)?;
        println!(
            "{} Removed ctx hook / NODE_OPTIONS filter entries from {}",
            "✓".green(),
            path.display()
        );
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
                    println!(
                        "{} Removed ctx MCP server from {}",
                        "✓".green(),
                        settings_path.display()
                    );
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
                        println!(
                            "{} Removed ctx MCP server from {}",
                            "✓".green(),
                            mcp_path.display()
                        );
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
                        println!(
                            "{} Removed ctx MCP server from {}",
                            "✓".green(),
                            desktop_cfg.display()
                        );
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
        let mut doc: serde_json::Value =
            serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
        crate::config::merge_ctx_into_mcp_servers(&mut doc, &bin)?;
        crate::config::write_json_atomic(&settings_path, &doc)?;
        println!(
            "{} Registered ctx MCP server in {}",
            "✓".green(),
            settings_path.display()
        );
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
        println!(
            "{} Registered ctx MCP server in {}",
            "✓".green(),
            extra.display()
        );
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
            println!(
                "{} Registered ctx MCP server in {}",
                "✓".green(),
                desktop_cfg.display()
            );
        }
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
    let tok_url =
        "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json";

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
