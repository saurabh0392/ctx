//! Merge `allowedMcpServers` and Claude Code `hooks` into `~/.claude/settings.json` (native ctx v2 path).

use anyhow::{Context, Result};
use serde_json::{json, Value};

pub const CTX_USER_PROMPT_SUBCOMMAND: &str = "hook user-prompt-submit";
pub const CTX_HOOK_EVENT_PATH: &str = "/api/hook/event";

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
    if let Ok(exe) = std::env::current_exe() {
        if let Some(s) = exe.to_str() {
            if !s.is_empty() {
                return format!("{} {}", s, CTX_USER_PROMPT_SUBCOMMAND);
            }
        }
    }
    format!("ctx {}", CTX_USER_PROMPT_SUBCOMMAND)
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
    if !settings.get("hooks").map(|h| h.is_object()).unwrap_or(false) {
        settings["hooks"] = json!({});
    }
    strip_ctx_native_hooks_from_settings(settings);
    let hooks = settings
        .get_mut("hooks")
        .and_then(|h| h.as_object_mut())
        .ok_or_else(|| anyhow::anyhow!("settings.hooks must be an object (create empty hooks first)"))?;

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

    for ev in ["PostToolUse", "SessionStart", "SessionEnd", "Stop"] {
        hooks
            .entry(ev.to_string())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .context(format!("hooks.{ev} must be array"))?
            .push(http_hook_event_entry(dashboard_port));
    }

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

/// Apply native ctx wiring: remove legacy NODE_OPTIONS filter, set allowlist + hooks.
pub fn apply_native_ctx_to_settings_doc(
    settings: &mut Value,
    active_slug: &str,
    dashboard_port: u16,
) -> Result<()> {
    strip_ctx_filter_from_node_options_in_settings(settings);
    merge_allowed_mcp_servers(settings, active_slug)?;
    if !settings.get("hooks").map(|h| h.is_object()).unwrap_or(false) {
        settings["hooks"] = json!({});
    }
    merge_ctx_native_hooks(settings, dashboard_port)?;
    Ok(())
}

/// Read ~/.claude/settings.json, apply [`apply_native_ctx_to_settings_doc`], write atomically.
pub fn write_native_ctx_to_user_settings(active_slug: &str, dashboard_port: u16) -> Result<()> {
    let path = crate::config::claude_settings_path();
    let mut doc = if path.exists() {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
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
}
