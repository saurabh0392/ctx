//! CLI-facing filter mode and session expansion controls.

use anyhow::{bail, Result};
use colored::Colorize;

use crate::config::{Config, FilterMode};

pub fn set_filter_mode(mode: FilterMode) -> Result<()> {
    let mut cfg = Config::load();
    let prev = cfg.filter_mode;
    cfg.filter_mode = mode;
    cfg.save()?;

    let slug = cfg.active_profile.as_deref().unwrap_or("all");
    let dash = cfg.dashboard_port.unwrap_or(8789);
    crate::claude_settings::write_native_ctx_to_user_settings(slug, dash)?;

    println!(
        "{} Filter mode: {} → {}",
        "✓".green().bold(),
        prev.as_str(),
        mode.as_str()
    );
    match mode {
        FilterMode::Soft => {
            println!("  MCP servers stay connected; unused tools hidden via permissions.deny");
        }
        FilterMode::Strict => {
            println!(
                "{} strict mode disconnects MCP servers outside the active profile allowlist",
                "!".yellow()
            );
        }
        FilterMode::Off => {
            println!("  No ctx-managed MCP filter rules");
        }
    }
    Ok(())
}

pub fn expand_session_target(target: &str) -> Result<()> {
    let mut cfg = Config::load();
    let key = target.trim();
    if key.is_empty() {
        bail!("usage: ctx filter expand <server-or-tool>");
    }
    if !cfg.session_expansion.iter().any(|s| s.eq_ignore_ascii_case(key)) {
        cfg.session_expansion.push(key.to_string());
        cfg.save()?;
    }

    let slug = cfg.active_profile.as_deref().unwrap_or("all");
    let dash = cfg.dashboard_port.unwrap_or(8789);
    crate::claude_settings::write_native_ctx_to_user_settings(slug, dash)?;

    let kind = if key.starts_with("mcp__") { "tool" } else { "server" };
    println!(
        "{} Session expansion: {} ({kind}; un-denied until config reset or `ctx filter clear-expansion`)",
        "✓".green().bold(),
        key.bold()
    );
    Ok(())
}

pub fn expand_session_server(server: &str) -> Result<()> {
    expand_session_target(server)
}

pub fn clear_session_expansion() -> Result<()> {
    let mut cfg = Config::load();
    if cfg.session_expansion.is_empty() {
        println!("No session expansions active.");
        return Ok(());
    }
    cfg.session_expansion.clear();
    cfg.save()?;

    let slug = cfg.active_profile.as_deref().unwrap_or("all");
    let dash = cfg.dashboard_port.unwrap_or(8789);
    crate::claude_settings::write_native_ctx_to_user_settings(slug, dash)?;
    println!("{} Cleared session expansion list", "✓".green().bold());
    Ok(())
}

/// Strip ctx-managed deny rules for A/B control prompts without changing pinned profile.
pub fn hook_apply_control_filter(quiet: bool) -> Result<()> {
    let cfg = Config::load();
    if cfg.filter_mode != FilterMode::Soft {
        return Ok(());
    }
    let dash = cfg.dashboard_port.unwrap_or(8789);
    crate::claude_settings::write_native_ctx_to_user_settings("all", dash)?;
    if !quiet {
        eprintln!("[ctx] A/B control — profile filter off for this prompt");
    }
    Ok(())
}

/// Lightweight profile + soft-filter sync from the UserPromptSubmit hook.
pub fn hook_sync_profile(
    new_slug: &str,
    prompt: &str,
    cwd: &str,
    quiet: bool,
    run_semantic_mix: bool,
) -> Result<Vec<crate::semantic_tools::ToolExpansionEntry>> {
    let mut cfg = Config::load();
    let prev = cfg.active_profile.clone().unwrap_or_else(|| "all".into());
    if prev == new_slug && cfg.filter_mode != FilterMode::Soft {
        return Ok(vec![]);
    }

    let mut expansions = Vec::new();
    if cfg.filter_mode == FilterMode::Soft {
        if let Ok(profile) = crate::profiles::get(new_slug) {
            expansions.extend(crate::semantic_tools::expand_from_prompt_keywords(
                prompt, cwd, &profile,
            )?);
        }
        if run_semantic_mix {
            expansions.extend(crate::semantic_tools::apply_hook_semantic_tool_mix(
                new_slug, prompt, cwd,
            )?);
        }
        cfg = Config::load();
    }

    cfg.active_profile = Some(new_slug.to_string());
    cfg.save()?;

    if cfg.filter_mode == FilterMode::Soft {
        let dash = cfg.dashboard_port.unwrap_or(8789);
        crate::claude_settings::write_native_ctx_to_user_settings(new_slug, dash)?;
    }

    if !quiet {
        if expansions.is_empty() {
            eprintln!("[ctx] auto-profile → {new_slug} (soft filter, servers stay connected)");
        } else {
            let names: Vec<_> = expansions.iter().map(|e| e.display.as_str()).collect();
            eprintln!(
                "[ctx] auto-profile → {new_slug}; un-denied {} for this session ({})",
                expansions.len(),
                names.join(", ")
            );
        }
    }
    Ok(expansions)
}
