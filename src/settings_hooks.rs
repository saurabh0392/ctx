//! Merge ctx-managed entries into `~/.claude/settings.json` `hooks` section.

use anyhow::{Context, Result};
use serde_json::{json, Value};

fn ctx_exe() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| "ctx".to_string())
}

/// Collect shell commands from a hook matcher entry (supports nested `hooks` or flat `command`).
fn commands_from_entry(entry: &Value) -> Vec<String> {
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

fn has_compress_hook(entries: &[Value]) -> bool {
    entries.iter().any(|e| {
        commands_from_entry(e)
            .iter()
            .any(|c| c.contains("ctx") && c.contains(" hook") && !c.contains("gain --brief"))
    })
}

fn has_gain_hook(entries: &[Value]) -> bool {
    entries
        .iter()
        .any(|e| commands_from_entry(e).iter().any(|c| c.contains("gain --brief")))
}

fn ensure_hooks_object(settings: &mut Value) -> Result<&mut serde_json::Map<String, Value>> {
    let root = settings
        .as_object_mut()
        .context("settings root must be an object")?;
    let hooks = root
        .entry("hooks".to_string())
        .or_insert_with(|| json!({}));
    hooks
        .as_object_mut()
        .context("hooks must be an object")
}

/// Prepend ctx compress rewrite (`ctx hook`) for Bash tool calls.
pub fn merge_compress_hook(settings: &mut Value) -> Result<()> {
    let hooks = ensure_hooks_object(settings)?;
    let arr = hooks
        .entry("PreToolUse".to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("PreToolUse hooks must be an array")?;

    if has_compress_hook(arr) {
        return Ok(());
    }

    let exe = ctx_exe();
    arr.insert(
        0,
        json!({
            "matcher": "Bash",
            "hooks": [{ "type": "command", "command": format!("{exe} hook") }]
        }),
    );
    Ok(())
}

/// Remove ctx `ctx hook` PreToolUse entries we added.
pub fn strip_compress_hook(settings: &mut Value) -> Result<()> {
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return Ok(());
    };
    let Some(arr) = hooks.get_mut("PreToolUse").and_then(|a| a.as_array_mut()) else {
        return Ok(());
    };

    arr.retain(|entry| {
        !commands_from_entry(entry)
            .iter()
            .any(|c| c.contains("ctx") && c.contains(" hook") && !c.contains("gain --brief"))
    });
    Ok(())
}

/// Append Stop hook running `ctx gain --brief` after each assistant turn.
pub fn merge_gain_stop_hook(settings: &mut Value) -> Result<()> {
    let hooks = ensure_hooks_object(settings)?;
    let arr = hooks
        .entry("Stop".to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("Stop hooks must be an array")?;

    if has_gain_hook(arr) {
        return Ok(());
    }

    let exe = ctx_exe();
    arr.push(json!({
        "hooks": [{ "type": "command", "command": format!("{exe} gain --brief") }]
    }));
    Ok(())
}

pub fn strip_gain_stop_hook(settings: &mut Value) -> Result<()> {
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return Ok(());
    };
    let Some(arr) = hooks.get_mut("Stop").and_then(|a| a.as_array_mut()) else {
        return Ok(());
    };

    arr.retain(|entry| {
        !commands_from_entry(entry)
            .iter()
            .any(|c| c.contains("gain --brief"))
    });
    Ok(())
}
