//! Merge Claude Code hooks and MCP filter rules into `~/.claude/settings.json`.

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::config::FilterMode;

pub const CTX_USER_PROMPT_SUBCOMMAND: &str = "hook user-prompt-submit";
pub const CTX_POST_TOOL_SUBCOMMAND: &str = "hook post-tool-use";
pub const CTX_HOOK_EVENT_PATH: &str = "/api/hook/event";
pub const CTX_STATUSLINE_SCRIPT_NAME: &str = "ctx-statusline.sh";

fn hook_commands_from_entry(entry: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(cmd) = entry.get("command").and_then(|c| c.as_str()) {
        out.push(cmd.to_string());
    }
    if let Some(arr) = entry.get("hooks").and_then(|h| h.as_array()) {
        for h in arr {
            if let Some(cmd) = h.get("command").and_then(|c| c.as_str()) {
                out.push(cmd.to_string());
            }
        }
    }
    out
}

fn hook_http_urls_from_entry(entry: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(arr) = entry.get("hooks").and_then(|h| h.as_array()) {
        for h in arr {
            if let Some(u) = h.get("url").and_then(|x| x.as_str()) {
                out.push(u.to_string());
            }
        }
    }
    out
}

/// True when this hook matcher entry is managed by ctx v2 (UserPromptSubmit command hook).
pub fn entry_is_ctx_user_prompt_hook(entry: &Value) -> bool {
    hook_commands_from_entry(entry)
        .iter()
        .any(|c| c.contains(CTX_USER_PROMPT_SUBCOMMAND))
}

/// True when this entry posts to ctx dashboard hook ingest.
pub fn entry_is_ctx_hook_http_endpoint(entry: &Value) -> bool {
    hook_http_urls_from_entry(entry).iter().any(|u| {
        u.contains(CTX_HOOK_EVENT_PATH) && (u.contains("127.0.0.1") || u.contains("localhost"))
    })
}

/// True when this hook matcher entry is managed by ctx PostToolUse compress hook.
pub fn entry_is_ctx_post_tool_hook(entry: &Value) -> bool {
    hook_commands_from_entry(entry)
        .iter()
        .any(|c| c.contains(CTX_POST_TOOL_SUBCOMMAND))
}

/// Remove ctx v2 native hook entries from `settings["hooks"]`. Returns true if modified.
pub fn strip_ctx_native_hooks_from_settings(settings: &mut Value) -> bool {
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return false;
    };
    let mut changed = false;
    for key in [
        "UserPromptSubmit",
        "PostToolUse",
        "SessionStart",
        "SessionEnd",
        "Stop",
    ] {
        if let Some(arr) = hooks.get_mut(key).and_then(|a| a.as_array_mut()) {
            let before = arr.len();
            arr.retain(|entry| {
                if key == "UserPromptSubmit" {
                    !entry_is_ctx_user_prompt_hook(entry)
                } else if key == "PostToolUse" {
                    !entry_is_ctx_post_tool_hook(entry) && !entry_is_ctx_hook_http_endpoint(entry)
                } else {
                    !entry_is_ctx_hook_http_endpoint(entry)
                }
            });
            if arr.len() != before {
                changed = true;
            }
        }
    }
    changed
}

fn resolve_ctx_command_for_hooks() -> String {
    resolve_ctx_subcommand(CTX_USER_PROMPT_SUBCOMMAND)
}

fn resolve_ctx_post_tool_command() -> String {
    resolve_ctx_subcommand(CTX_POST_TOOL_SUBCOMMAND)
}

fn resolve_ctx_subcommand(sub: &str) -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(s) = exe.to_str() {
            if !s.is_empty() {
                return format!("{} {}", s, sub);
            }
        }
    }
    format!("ctx {}", sub)
}

fn http_hook_event_entry(dashboard_port: u16) -> Value {
    let url = format!("http://127.0.0.1:{dashboard_port}{}", CTX_HOOK_EVENT_PATH);
    json!({
        "hooks": [{
            "type": "http",
            "url": url,
            "timeout": 2,
            "async": true
        }]
    })
}

/// Merge ctx v2 `UserPromptSubmit` command hook and async HTTP analytics hooks.
/// Removes prior ctx-managed entries for the same events, then appends fresh ones.
pub fn merge_ctx_native_hooks(settings: &mut Value, dashboard_port: u16) -> Result<()> {
    if !settings
        .get("hooks")
        .map(|h| h.is_object())
        .unwrap_or(false)
    {
        settings["hooks"] = json!({});
    }
    strip_ctx_native_hooks_from_settings(settings);
    let hooks = settings
        .get_mut("hooks")
        .and_then(|h| h.as_object_mut())
        .ok_or_else(|| {
            anyhow::anyhow!("settings.hooks must be an object (create empty hooks first)")
        })?;

    let cmd = resolve_ctx_command_for_hooks();
    let ups_entry = json!({
        "hooks": [{
            "type": "command",
            "command": cmd,
            "timeout": 3
        }]
    });

    hooks
        .entry("UserPromptSubmit".to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("hooks.UserPromptSubmit must be array")?
        .push(ups_entry);

    let post_tool_cmd = resolve_ctx_post_tool_command();
    let post_tool_entry = json!({
        "matcher": "Bash|Read|Grep|Glob|mcp__.*",
        "hooks": [{
            "type": "command",
            "command": post_tool_cmd,
            "timeout": 2
        }]
    });
    hooks
        .entry("PostToolUse".to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("hooks.PostToolUse must be array")?
        .push(post_tool_entry);

    for ev in ["SessionStart", "SessionEnd", "Stop"] {
        hooks
            .entry(ev.to_string())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .context(format!("hooks.{ev} must be array"))?
            .push(http_hook_event_entry(dashboard_port));
    }

    Ok(())
}

fn statusline_command_is_ctx_managed(cmd: &str) -> bool {
    cmd.contains(CTX_STATUSLINE_SCRIPT_NAME) || cmd.contains(".ctx/bin/ctx-statusline")
}

/// Remove ctx-managed `statusLine` entry. Returns true if modified.
pub fn strip_ctx_statusline(settings: &mut Value) -> bool {
    let Some(sl) = settings.get("statusLine") else {
        return false;
    };
    let is_ctx = sl
        .get("command")
        .and_then(|c| c.as_str())
        .map(statusline_command_is_ctx_managed)
        .unwrap_or(false);
    if is_ctx {
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("statusLine");
        }
        return true;
    }
    false
}

/// Install ctx statusLine wrapper that records allowance snapshots for the dashboard.
pub fn merge_ctx_statusline(settings: &mut Value) -> Result<()> {
    strip_ctx_statusline(settings);
    let script = crate::config::statusline_script_path();
    settings["statusLine"] = json!({
        "type": "command",
        "command": script.to_string_lossy(),
        "padding": 0,
        "timeoutMs": 2000
    });
    Ok(())
}

/// Build `allowedMcpServers` JSON array for Claude Code, or `None` when all servers are allowed.
pub fn allowed_mcp_servers_value_for_slug(slug: &str) -> Result<Option<Value>> {
    let profile = crate::profiles::get(slug)?;
    let names = crate::profiles::allowed_server_names_for_profile(&profile);
    if names.is_empty() {
        return Ok(None);
    }
    let arr: Vec<Value> = names
        .into_iter()
        .map(|n| json!({ "serverName": n }))
        .collect();
    Ok(Some(Value::Array(arr)))
}

/// Set or remove top-level `allowedMcpServers` on the settings document.
pub fn merge_allowed_mcp_servers(settings: &mut Value, slug: &str) -> Result<()> {
    if let Some(v) = allowed_mcp_servers_value_for_slug(slug)? {
        settings["allowedMcpServers"] = v;
    } else {
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("allowedMcpServers");
        }
    }
    Ok(())
}

/// Remove top-level `allowedMcpServers` (ctx uninstall).
pub fn strip_allowed_mcp_servers(settings: &mut Value) -> bool {
    if let Some(obj) = settings.as_object_mut() {
        return obj.remove("allowedMcpServers").is_some();
    }
    false
}

/// Strip ctx `NODE_OPTIONS` `--require …/filter.js` from `settings.env` when present.
pub fn strip_ctx_filter_from_node_options_in_settings(settings: &mut Value) -> bool {
    let Some(env) = settings.get_mut("env").and_then(|e| e.as_object_mut()) else {
        return false;
    };
    let Some(node) = env.get("NODE_OPTIONS").and_then(|v| v.as_str()) else {
        return false;
    };
    match crate::filter_hook::strip_ctx_require_from_node_options(Some(node)) {
        Some(st) if !st.trim().is_empty() => {
            env.insert("NODE_OPTIONS".into(), Value::String(st));
            true
        }
        _ => {
            env.remove("NODE_OPTIONS");
            true
        }
    }
}

/// Remove ctx-managed `permissions.deny` MCP rules. Returns true if modified.
pub fn strip_ctx_deny_rules(settings: &mut Value) -> bool {
    let Some(perms) = settings
        .get_mut("permissions")
        .and_then(|p| p.as_object_mut())
    else {
        return false;
    };
    let Some(deny) = perms.get_mut("deny").and_then(|d| d.as_array_mut()) else {
        return false;
    };
    let before = deny.len();
    deny.retain(|v| {
        v.as_str()
            .map(|s| !crate::profiles::is_ctx_managed_deny_pattern(s))
            .unwrap_or(true)
    });
    deny.len() != before
}

/// Write soft-filter deny rules for `slug`, preserving non-ctx deny entries.
pub fn merge_profile_deny_rules(settings: &mut Value, slug: &str) -> Result<()> {
    let profile = crate::profiles::get(slug)?;
    let cfg = crate::config::Config::load();
    let mut expansion = cfg.session_expansion.clone();
    expansion.extend(cfg.session_semantic_tools.clone());
    let local_names = crate::profiles::local_mcp_server_names(settings);
    let patterns = crate::profiles::deny_patterns_for_profile(&profile, &expansion, &local_names);

    if !settings
        .get("permissions")
        .map(|p| p.is_object())
        .unwrap_or(false)
    {
        settings["permissions"] = json!({});
    }
    let perms = settings
        .get_mut("permissions")
        .and_then(|p| p.as_object_mut())
        .context("settings.permissions must be object")?;

    let mut deny: Vec<String> = perms
        .get("deny")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .filter(|s| !crate::profiles::is_ctx_managed_deny_pattern(s))
                .collect()
        })
        .unwrap_or_default();

    deny.extend(patterns);
    deny.sort();
    deny.dedup();

    if deny.is_empty() {
        if let Some(arr) = perms.get_mut("deny").and_then(|d| d.as_array_mut()) {
            arr.retain(|v| {
                v.as_str()
                    .map(|s| !crate::profiles::is_ctx_managed_deny_pattern(s))
                    .unwrap_or(true)
            });
            if arr.is_empty() {
                perms.remove("deny");
            }
        }
    } else {
        perms.insert(
            "deny".to_string(),
            Value::Array(deny.into_iter().map(Value::String).collect()),
        );
    }
    Ok(())
}

/// Apply profile filter for the given mode (soft / strict / off).
pub fn merge_profile_filter(settings: &mut Value, slug: &str, mode: FilterMode) -> Result<()> {
    match mode {
        FilterMode::Soft => {
            strip_allowed_mcp_servers(settings);
            merge_profile_deny_rules(settings, slug)?;
        }
        FilterMode::Strict => {
            strip_ctx_deny_rules(settings);
            merge_allowed_mcp_servers(settings, slug)?;
        }
        FilterMode::Off => {
            strip_allowed_mcp_servers(settings);
            strip_ctx_deny_rules(settings);
        }
    }
    Ok(())
}

/// Apply native ctx wiring: legacy NODE_OPTIONS cleanup, filter mode, hooks.
pub fn apply_native_ctx_to_settings_doc(
    settings: &mut Value,
    active_slug: &str,
    dashboard_port: u16,
) -> Result<()> {
    strip_ctx_filter_from_node_options_in_settings(settings);
    let mode = crate::config::Config::load().filter_mode;
    merge_profile_filter(settings, active_slug, mode)?;
    if !settings
        .get("hooks")
        .map(|h| h.is_object())
        .unwrap_or(false)
    {
        settings["hooks"] = json!({});
    }
    merge_ctx_native_hooks(settings, dashboard_port)?;
    merge_ctx_statusline(settings)?;
    Ok(())
}

/// True when `~/.claude/settings.json` points statusLine at the ctx-managed script.
pub fn ctx_statusline_wired_in_settings() -> bool {
    let path = crate::config::claude_settings_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    doc.get("statusLine")
        .and_then(|sl| sl.get("command"))
        .and_then(|c| c.as_str())
        .map(|cmd| {
            cmd.contains(CTX_STATUSLINE_SCRIPT_NAME) || cmd.contains(".ctx/bin/ctx-statusline")
        })
        .unwrap_or(false)
}

/// Observation-only wiring for experiment pre-ctx phase: no hooks, no filter rules.
/// Keeps statusLine so ingest and allowance meters still work.
pub fn apply_observation_only_to_settings_doc(settings: &mut Value) -> Result<()> {
    strip_ctx_filter_from_node_options_in_settings(settings);
    merge_profile_filter(settings, "all", FilterMode::Off)?;
    if settings
        .get("hooks")
        .map(|h| h.is_object())
        .unwrap_or(false)
    {
        strip_ctx_native_hooks_from_settings(settings);
    }
    strip_allowed_mcp_servers(settings);
    merge_ctx_statusline(settings)?;
    Ok(())
}

/// Strip ctx intervention hooks and filters; keep statusLine for telemetry.
pub fn write_observation_only_to_user_settings() -> Result<()> {
    let path = crate::config::claude_settings_path();
    let mut doc = if path.exists() {
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        json!({})
    };
    apply_observation_only_to_settings_doc(&mut doc)?;
    crate::config::write_json_atomic(&path, &doc)?;
    Ok(())
}

/// Apply experiment hook mode from config (`experiment_hooks_enabled`).
pub fn sync_experiment_hooks_from_config() -> Result<()> {
    let cfg = crate::config::Config::load();
    if cfg.experiment_hooks_enabled {
        let slug = cfg.active_profile.as_deref().unwrap_or("all");
        let port = cfg.dashboard_port.unwrap_or(8789);
        write_native_ctx_to_user_settings(slug, port)
    } else {
        write_observation_only_to_user_settings()
    }
}

/// Read ~/.claude/settings.json, apply [`apply_native_ctx_to_settings_doc`], write atomically.
/// Respects `experiment_hooks_enabled` (pre-ctx phase writes observation-only settings).
pub fn write_native_ctx_to_user_settings(active_slug: &str, dashboard_port: u16) -> Result<()> {
    if !crate::config::Config::load().experiment_hooks_enabled {
        return write_observation_only_to_user_settings();
    }
    let path = crate::config::claude_settings_path();
    let mut doc = if path.exists() {
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        json!({})
    };
    apply_native_ctx_to_settings_doc(&mut doc, active_slug, dashboard_port)?;
    crate::config::write_json_atomic(&path, &doc)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_soft_deny_rules_for_data_profile() {
        let mut doc = json!({
            "permissions": {
                "deny": ["Bash(rm -rf *)"]
            }
        });
        merge_profile_filter(&mut doc, "data", FilterMode::Soft).unwrap();
        assert!(doc.get("allowedMcpServers").is_none());
        let deny = doc["permissions"]["deny"].as_array().unwrap();
        assert!(deny.iter().any(|v| v.as_str() == Some("Bash(rm -rf *)")));
        assert!(deny.iter().any(|v| {
            v.as_str()
                .map(|s| s.starts_with("mcp__claude_ai_") && s.ends_with("__*"))
                .unwrap_or(false)
        }));
        assert!(!deny
            .iter()
            .any(|v| v.as_str() == Some("mcp__claude_ai_Data_Shippo__*")));
    }

    #[test]
    fn strip_ctx_deny_preserves_user_rules() {
        let mut doc = json!({
            "permissions": {
                "deny": ["Bash(rm *)", "mcp__claude_ai_Figma__*"]
            }
        });
        assert!(strip_ctx_deny_rules(&mut doc));
        let deny = doc["permissions"]["deny"].as_array().unwrap();
        assert_eq!(deny.len(), 1);
        assert_eq!(deny[0], "Bash(rm *)");
    }

    #[test]
    fn merge_idempotent_allowed_mcp() {
        let mut doc = json!({});
        merge_allowed_mcp_servers(&mut doc, "data").unwrap();
        assert!(doc.get("allowedMcpServers").unwrap().is_array());
        let n = doc["allowedMcpServers"].as_array().unwrap().len();
        assert!(n > 0);
        merge_allowed_mcp_servers(&mut doc, "all").unwrap();
        assert!(doc.get("allowedMcpServers").is_none());
    }

    #[test]
    fn strip_and_merge_hooks_roundtrip() {
        let mut doc = json!({ "hooks": {}});
        merge_ctx_native_hooks(&mut doc, 8789).unwrap();
        assert!(doc["hooks"]["UserPromptSubmit"].as_array().unwrap().len() >= 1);
        assert!(strip_ctx_native_hooks_from_settings(&mut doc));
        assert_eq!(
            doc["hooks"]["UserPromptSubmit"].as_array().unwrap().len(),
            0
        );
        merge_ctx_native_hooks(&mut doc, 8789).unwrap();
        assert!(doc["hooks"]["Stop"].as_array().unwrap().len() >= 1);
    }

    #[test]
    fn strip_and_merge_statusline_roundtrip() {
        let mut doc = json!({});
        merge_ctx_statusline(&mut doc).unwrap();
        assert!(doc["statusLine"]["command"]
            .as_str()
            .unwrap()
            .contains("ctx-statusline.sh"));
        assert!(strip_ctx_statusline(&mut doc));
        assert!(doc.get("statusLine").is_none());
    }
}
