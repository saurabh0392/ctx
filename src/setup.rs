#[cfg(feature = "onnx")]
use anyhow::Context;
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
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
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

fn setup_preview_lines(periodic_ingest: bool, beta: bool) -> Vec<String> {
    let config = if beta {
        format!(
            "Create {} and write beta config (evidence-gated Full output autopilot; MCP filtering off)",
            crate::config::ctx_dir().display()
        )
    } else {
        format!(
            "Create {} and write config (MCP filtering off by default)",
            crate::config::ctx_dir().display()
        )
    };
    let availability = if beta {
        "Keep every tool available; output changes still fail closed behind safety and evidence gates"
            .to_string()
    } else {
        "Keep all tools available; profile filtering stays an opt-in (`ctx use <profile>`)"
            .to_string()
    };
    vec![
        config,
        crate::daemon::dashboard_ingest_summary(DASHBOARD_PORT, periodic_ingest),
        "Merge ctx hooks into ~/.claude/settings.json, with no MCP filter rules (unless --no-install)"
            .to_string(),
        "Index ~/.claude/projects/**/*.jsonl into ~/.ctx/ctx.db so the dashboard and learning have history"
            .to_string(),
        availability,
        "Install the CTX Codex plugin when Codex is present (unless --no-install); hook trust remains an explicit Codex review"
            .to_string(),
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

fn apply_fresh_install_defaults(cfg: &mut crate::config::Config, beta: bool) {
    cfg.filter_mode = crate::config::FilterMode::Off;
    cfg.active_profile = Some("all".to_string());
    cfg.auto_profile_enabled = false;
    cfg.dashboard_port = Some(DASHBOARD_PORT);
    cfg.experiment_hooks_enabled = true;
    if beta {
        // The beta defaults to full output autopilot, but the evidence gate, bounded burn-in,
        // deny-set, and Read edit-intent guard still fail closed. MCP filtering remains off:
        // output control and input-menu pruning are separate bets.
        cfg.compress_preset = crate::config::CompressPreset::Full;
        cfg.compress_enabled = true;
        cfg.compress_shadow_enabled = true;
        cfg.compress_auto_trial = true;
        cfg.compress_force_active = false;
        cfg.compress_trim_all = true;
        cfg.compress_read_edit_guard = true;
        cfg.compress_explore_read_rate = 0.20;
        cfg.pruned_servers.clear();
    }
}

pub fn run(
    no_install: bool,
    _no_zshrc_prompt: bool,
    dry_run: bool,
    yes: bool,
    beta: bool,
) -> Result<()> {
    let config_existed_before = crate::config::ctx_dir().join("config.toml").exists();
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
    for (i, line) in setup_preview_lines(host.needs_periodic_ingest(), beta)
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
    if beta {
        let state = crate::beta::activate_from_environment()?;
        println!(
            "{} Beta channel enrolled as {}. Feedback remains preview-and-send only.",
            "✓".green(),
            state.participant_id
        );
    }
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
            apply_fresh_install_defaults(&mut cfg, beta && !config_existed_before);
            let _ = cfg.save();
        }
    }

    // Default setup is hook-first with no TLS interception or model-API proxy. MCP filtering runs through Claude Code permission
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
        match crate::conversations::ingest_claude_jsonl(false) {
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
        // Register the MCP server too, so the agent can call ctx_expand to recover a trim. Without
        // this the hook trims but the recovery tool the marker points at does not exist, and every
        // trim that cut something needed becomes a re-read instead of a cheap round trip.
        let _ = crate::claude_settings::register_ctx_mcp_server_in_user_config();
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
        println!("  MCP tools:  ctx_expand (recover a trim) + status/spend/waste");
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
        println!(
            "  Install Claude Code and re-run `ctx setup` for hooks (filtering stays opt-in)."
        );
    }

    wire_mcp_server(&*host)?;

    if !no_install {
        register_cursor_hook_if_present();
        match crate::codex_plugin::install_if_present() {
            Ok(true) => {
                println!("  Codex:      CTX plugin installed (review and trust its hooks with /hooks in Codex)");
            }
            Ok(false) => {}
            Err(e) => println!("{} Codex plugin installation skipped: {e}", "!".yellow()),
        }
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

    crate::beta::record_event(
        "setup_completed",
        "cli",
        Some(if beta { "beta" } else { "standard" }),
    );

    Ok(())
}

/// Register the live Cursor postToolUse hook in `~/.cursor/hooks.json`, but only when Cursor is
/// actually present (a `~/.cursor` directory exists). We never create that directory for users who
/// do not run Cursor. Best-effort: a failure here must not abort setup. (ADR 0018 / CTX-27)
fn register_cursor_hook_if_present() {
    let present = crate::config::home_dir_for_paths()
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

pub fn uninstall(purge_data: bool, yes: bool) -> Result<()> {
    let oauth_server_ids = if purge_data {
        confirm_data_purge(yes)?;
        gateway_oauth_server_ids()?
    } else {
        if let Err(e) = crate::experiment_plan::backup_experiment_state() {
            println!("{} Experiment backup skipped: {e}", "!".yellow());
        } else if crate::experiment_plan::plan_path().is_file() {
            println!(
                "{} Experiment plan backed up to {}",
                "✓".green(),
                crate::experiment_plan::persistent_experiment_dir().display()
            );
        }
        Vec::new()
    };

    // Restore Codex MCP definitions before removing the binary/plugin that serves the gateway.
    // This must fail loudly: leaving an MCP server pointed at a dead CTX executable is worse than
    // leaving the rest of CTX installed for the user to retry.
    let restore = crate::mcp_gateway::registry::codex_restore_all()?;
    for name in restore.restored {
        println!(
            "{} Restored direct Codex MCP server {:?}",
            "✓".green(),
            name
        );
    }
    for name in restore.preserved {
        println!(
            "{} Preserved user-modified Codex MCP server {:?}",
            "i".yellow(),
            name
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

    match crate::codex_plugin::uninstall_if_owned() {
        Ok(true) => println!("{} Removed CTX Codex plugin and marketplace", "✓".green()),
        Ok(false) => {}
        Err(e) => println!("{} Codex plugin removal skipped: {e}", "!".yellow()),
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

    if crate::beta::state_path().exists() {
        crate::beta::remove_state()?;
        println!("{} Removed the stored beta capability", "✓".green());
    }

    if purge_data {
        purge_gateway_oauth_credentials(oauth_server_ids)?;
        purge_owned_data()?;
        println!(
            "{} Permanently deleted CTX local data and experiment backups",
            "✓".green()
        );
    }

    println!(
        "{} ctx uninstalled. {}",
        "✓".green(),
        crate::host::uninstall_reload_hint()
    );
    println!("Apply the reload so removed hooks, filter rules, and MCP server config take effect.");
    Ok(())
}

fn confirm_data_purge(yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !stdin().is_terminal() || !stdout().is_terminal() {
        anyhow::bail!(
            "Data purge needs confirmation. Re-run with --purge-data --yes to confirm a non-interactive purge."
        );
    }
    println!(
        "{} This permanently deletes {}, including the database, retained originals, settings, logs, gateway registry, and beta state.",
        "!".red(),
        crate::config::ctx_dir().display()
    );
    println!(
        "It also deletes CTX's experiment backup at {}. This cannot be undone.",
        crate::experiment_plan::persistent_experiment_dir().display()
    );
    print!("Type DELETE CTX DATA to continue: ");
    stdout().flush()?;
    let mut answer = String::new();
    stdin().read_line(&mut answer)?;
    if answer.trim() != "DELETE CTX DATA" {
        anyhow::bail!("Data purge aborted; no uninstall changes were made.");
    }
    Ok(())
}

fn gateway_oauth_server_ids() -> Result<Vec<String>> {
    let registry = crate::mcp_gateway::registry::GatewayRegistry::load()?;
    Ok(registry
        .servers
        .into_iter()
        .filter_map(|(id, server)| {
            matches!(
                server,
                crate::mcp_gateway::registry::GatewayServer::StreamableHttp(_)
            )
            .then_some(id)
        })
        .collect())
}

fn purge_gateway_oauth_credentials(server_ids: Vec<String>) -> Result<()> {
    for id in server_ids {
        crate::mcp_gateway::oauth::logout(&id)?;
    }
    Ok(())
}

fn purge_owned_data() -> Result<()> {
    let ctx_dir = crate::config::ctx_dir();
    if ctx_dir.exists() {
        validate_ctx_purge_target(&ctx_dir)?;
        std::fs::remove_dir_all(&ctx_dir)?;
    }
    crate::experiment_plan::purge_persistent_experiment_state()?;
    Ok(())
}

fn validate_ctx_purge_target(path: &std::path::Path) -> Result<()> {
    let label = "CTX data directory";
    if !path.is_absolute() || path.parent().is_none() {
        anyhow::bail!("refusing to purge unsafe {label}: {}", path.display());
    }
    let resolved = if path.exists() {
        std::fs::canonicalize(path)?
    } else {
        path.to_path_buf()
    };
    if resolved.parent().is_none() {
        anyhow::bail!("refusing to purge unsafe {label}: {}", path.display());
    }
    let home = dirs::home_dir().and_then(|home| std::fs::canonicalize(home).ok());
    if home.as_deref() == Some(resolved.as_path()) {
        anyhow::bail!("refusing to purge the user home as the {label}");
    }
    let default = dirs::home_dir().map(|home| home.join(".ctx"));
    let default = default.map(|path| std::fs::canonicalize(&path).unwrap_or(path));
    let is_default = default.as_deref() == Some(resolved.as_path());
    let has_marker = resolved.join(crate::config::CTX_OWNERSHIP_MARKER).is_file();
    if !is_default && !has_marker {
        anyhow::bail!(
            "refusing to purge unverified custom CTX data directory {} (ownership marker missing)",
            path.display()
        );
    }
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
    if let Some(home) = crate::config::home_dir_for_paths() {
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
    let cursor_mcp = crate::config::home_dir_for_paths()
        .unwrap_or_default()
        .join(".cursor")
        .join("mcp.json");
    let windsurf_mcp = crate::config::home_dir_for_paths()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beta_defaults_are_full_but_still_fail_closed() {
        let mut cfg = crate::config::Config {
            compress_force_active: true,
            pruned_servers: vec!["mcp__unused".into()],
            ..Default::default()
        };

        apply_fresh_install_defaults(&mut cfg, true);

        assert_eq!(cfg.filter_mode, crate::config::FilterMode::Off);
        assert_eq!(cfg.compress_preset, crate::config::CompressPreset::Full);
        assert!(cfg.compress_enabled);
        assert!(cfg.compress_shadow_enabled);
        assert!(cfg.compress_auto_trial);
        assert!(cfg.compress_trim_all);
        assert!(cfg.compress_read_edit_guard);
        assert!(!cfg.compress_force_active);
        assert_eq!(cfg.compress_explore_read_rate, 0.20);
        assert!(cfg.pruned_servers.is_empty());
    }

    #[test]
    fn standard_defaults_do_not_enable_output_autopilot() {
        let mut cfg = crate::config::Config::default();
        apply_fresh_install_defaults(&mut cfg, false);
        assert_eq!(cfg.filter_mode, crate::config::FilterMode::Off);
        assert_eq!(cfg.compress_preset, crate::config::CompressPreset::Off);
    }

    #[test]
    fn beta_preview_describes_the_defaults_that_setup_applies() {
        let preview = setup_preview_lines(false, true).join("\n");
        assert!(preview.contains("evidence-gated Full output autopilot"));
        assert!(preview.contains("MCP filtering off"));
        assert!(preview.contains("fail closed"));
    }

    #[test]
    fn purge_removes_both_ctx_state_locations() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("ctx-data");
        let backup = tmp.path().join("ctx-backup");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(
            data.join(crate::config::CTX_OWNERSHIP_MARKER),
            "CTX owns this state directory.\n",
        )
        .unwrap();
        std::fs::write(data.join("ctx.db"), "old database").unwrap();
        std::fs::write(backup.join("experiment-plan.toml"), "old plan").unwrap();
        std::fs::write(backup.join("keep-me.txt"), "not owned by CTX").unwrap();
        std::env::set_var("CTX_HOME", &data);
        std::env::set_var("CTX_EXPERIMENT_BACKUP_DIR", &backup);

        purge_owned_data().unwrap();

        assert!(!data.exists());
        assert!(backup.join("keep-me.txt").is_file());
        assert!(!backup.join("experiment-plan.toml").exists());
        std::env::remove_var("CTX_HOME");
        std::env::remove_var("CTX_EXPERIMENT_BACKUP_DIR");
    }

    #[test]
    fn purge_refuses_an_unmarked_custom_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let unrelated = tmp.path().join("unrelated");
        std::fs::create_dir_all(&unrelated).unwrap();
        assert!(validate_ctx_purge_target(&unrelated).is_err());
        assert!(unrelated.exists());
    }
}
