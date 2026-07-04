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
    if !cfg
        .session_expansion
        .iter()
        .any(|s| s.eq_ignore_ascii_case(key))
    {
        cfg.session_expansion.push(key.to_string());
        cfg.save()?;
    }

    let slug = cfg.active_profile.as_deref().unwrap_or("all");
    let dash = cfg.dashboard_port.unwrap_or(8789);
    crate::claude_settings::write_native_ctx_to_user_settings(slug, dash)?;

    let kind = if key.starts_with("mcp__") {
        "tool"
    } else {
        "server"
    };
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

/// Normalize a server reference to its canonical `mcp__..__` prefix for deny rules and
/// `pruned_servers`. Any already-`mcp__` prefix is kept as-is (claude.ai connectors AND local
/// servers like `mcp__ctx__`); only a bare display name or id gets the `mcp__claude_ai_` prefix.
fn canonical_server_prefix(server: &str) -> String {
    let s = server.trim();
    if s.starts_with("mcp__") {
        let base = s.trim_end_matches('*').trim_end_matches('_');
        return format!("{base}__");
    }
    let id = s.replace([' ', '-'], "_");
    format!("mcp__claude_ai_{id}__")
}

/// Prune a whole MCP server from the tool menu (CTX-64): persistently deny its tools in soft mode,
/// reversible via a session reach (`ctx filter expand`) or `unprune_server`. Records the prune as an
/// insight-action. Returns true when the server was newly pruned (state changed).
pub fn prune_server(server: &str) -> Result<bool> {
    let prefix = canonical_server_prefix(server);
    // ctx's own server carries the recovery tools (ctx_expand, ctx_status, ctx_waste). Pruning it
    // would hide the very tools that make trimming reversible, so it is never a prune target.
    if prefix.eq_ignore_ascii_case("mcp__ctx__") {
        bail!("the ctx server is not prunable: it holds ctx_expand and the other recovery tools");
    }
    let mut cfg = Config::load();
    if cfg
        .pruned_servers
        .iter()
        .any(|p| p.eq_ignore_ascii_case(&prefix))
    {
        return Ok(false);
    }
    // A fresh prune overrides any leftover session reach for this server.
    cfg.session_expansion
        .retain(|e| !crate::profiles::prefix_covers_expansion_entry(&prefix, e));
    cfg.pruned_servers.push(prefix.clone());
    cfg.save()?;

    let slug = cfg.active_profile.as_deref().unwrap_or("all");
    let dash = cfg.dashboard_port.unwrap_or(8789);
    crate::claude_settings::write_native_ctx_to_user_settings(slug, dash)?;

    // Record the insight-action: a prune that removed a server (feeds the Home counter).
    let display = crate::profiles::mcp_prefix_to_server_display(&prefix);
    if let Ok(conn) = crate::db::open_db() {
        let removed = serde_json::to_string(&[&display]).unwrap_or_else(|_| "[]".into());
        let _ = crate::db::insert_profile_change(&conn, slug, slug, "[]", &removed);
    }
    Ok(true)
}

/// Reverse a prune (CTX-64): drop the server from `pruned_servers` and any lingering session reach,
/// then rewrite settings so the full server returns. Returns true when it had been pruned.
pub fn unprune_server(server: &str) -> Result<bool> {
    let prefix = canonical_server_prefix(server);
    let mut cfg = Config::load();
    let before = cfg.pruned_servers.len();
    cfg.pruned_servers
        .retain(|p| !p.eq_ignore_ascii_case(&prefix));
    if cfg.pruned_servers.len() == before {
        return Ok(false);
    }
    cfg.save()?;

    let slug = cfg.active_profile.as_deref().unwrap_or("all");
    let dash = cfg.dashboard_port.unwrap_or(8789);
    crate::claude_settings::write_native_ctx_to_user_settings(slug, dash)?;
    Ok(true)
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

#[cfg(test)]
mod tests {
    use super::canonical_server_prefix;

    #[test]
    fn canonical_prefix_keeps_local_servers_and_prefixes() {
        // Local server (ctx): must not get a second mcp__claude_ai_ prefix.
        assert_eq!(canonical_server_prefix("mcp__ctx__"), "mcp__ctx__");
        // A claude.ai prefix passes through, wildcard and stray underscores normalized.
        assert_eq!(
            canonical_server_prefix("mcp__claude_ai_Canva__"),
            "mcp__claude_ai_Canva__"
        );
        assert_eq!(
            canonical_server_prefix("mcp__claude_ai_Canva__*"),
            "mcp__claude_ai_Canva__"
        );
        // A bare display name becomes a claude.ai prefix.
        assert_eq!(canonical_server_prefix("Canva"), "mcp__claude_ai_Canva__");
        assert_eq!(
            canonical_server_prefix("Data Shippo"),
            "mcp__claude_ai_Data_Shippo__"
        );
    }
}
