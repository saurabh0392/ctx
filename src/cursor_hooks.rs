//! Register ctx's live Cursor hook in `~/.cursor/hooks.json` (ADR 0018 / CTX-27).
//!
//! This mirrors `claude_settings.rs`: merge a single ctx-owned command hook into the user's
//! Cursor hooks file without disturbing other hooks, and strip exactly that entry on uninstall.
//! Cursor's hooks file is schema version 1: `{ "version": 1, "hooks": { "<event>": [ ... ] } }`.

use anyhow::{Context, Result};
use serde_json::{json, Value};

/// ctx subcommand wired as the Cursor postToolUse command hook.
pub const CTX_CURSOR_POST_TOOL_SUBCOMMAND: &str = "hook cursor-post-tool-use";

/// Cursor hook event ctx registers under.
const CURSOR_POST_TOOL_EVENT: &str = "postToolUse";

fn resolve_ctx_cursor_command() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(s) = exe.to_str() {
            if !s.is_empty() {
                return format!("{} {}", s, CTX_CURSOR_POST_TOOL_SUBCOMMAND);
            }
        }
    }
    format!("ctx {}", CTX_CURSOR_POST_TOOL_SUBCOMMAND)
}

/// True when this Cursor hook entry is the ctx-managed postToolUse command hook.
fn entry_is_ctx_cursor_hook(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(|c| c.as_str())
        .map(|c| c.contains(CTX_CURSOR_POST_TOOL_SUBCOMMAND))
        .unwrap_or(false)
}

/// Remove ctx-managed entries from `hooks.postToolUse`, pruning empty containers. Returns true if
/// it changed anything. Leaves every non-ctx hook (and every other event) untouched.
pub fn strip_ctx_cursor_hook(doc: &mut Value) -> bool {
    let Some(hooks) = doc.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return false;
    };
    let Some(arr) = hooks
        .get_mut(CURSOR_POST_TOOL_EVENT)
        .and_then(|a| a.as_array_mut())
    else {
        return false;
    };
    let before = arr.len();
    arr.retain(|e| !entry_is_ctx_cursor_hook(e));
    let changed = arr.len() != before;
    if arr.is_empty() {
        hooks.remove(CURSOR_POST_TOOL_EVENT);
    }
    changed
}

/// Merge the ctx postToolUse command hook into the document, replacing any prior ctx entry first
/// so repeated setups stay idempotent. No matcher: increment 1 observes every tool, and the
/// handler stays silent on tools with no compressible output.
pub fn merge_ctx_cursor_hook(doc: &mut Value) -> Result<()> {
    if !doc.is_object() {
        *doc = json!({});
    }
    doc["version"] = json!(1);
    if !doc.get("hooks").map(|h| h.is_object()).unwrap_or(false) {
        doc["hooks"] = json!({});
    }
    strip_ctx_cursor_hook(doc);

    let entry = json!({
        "command": resolve_ctx_cursor_command()
    });
    doc["hooks"]
        .as_object_mut()
        .context("hooks must be an object")?
        .entry(CURSOR_POST_TOOL_EVENT.to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("hooks.postToolUse must be an array")?
        .push(entry);
    Ok(())
}

/// Read `~/.cursor/hooks.json` (or start fresh), merge the ctx hook, write atomically.
pub fn write_ctx_cursor_hook() -> Result<()> {
    let path = crate::config::cursor_hooks_path();
    let mut doc = read_doc(&path)?;
    merge_ctx_cursor_hook(&mut doc)?;
    crate::config::write_json_atomic(&path, &doc)?;
    Ok(())
}

/// Strip the ctx hook from `~/.cursor/hooks.json`. If the file would be left as an empty hooks
/// document ctx created, remove it; otherwise rewrite it preserving the user's other hooks.
pub fn remove_ctx_cursor_hook() -> Result<bool> {
    let path = crate::config::cursor_hooks_path();
    if !path.exists() {
        return Ok(false);
    }
    let mut doc = read_doc(&path)?;
    let changed = strip_ctx_cursor_hook(&mut doc);
    if !changed {
        return Ok(false);
    }
    let hooks_empty = doc
        .get("hooks")
        .and_then(|h| h.as_object())
        .map(|m| m.is_empty())
        .unwrap_or(true);
    if hooks_empty {
        // Only delete a file that is now just the version stamp ctx wrote; if the user kept other
        // top-level keys, leave the file in place with the ctx hook removed.
        let only_known_keys = doc
            .as_object()
            .map(|m| m.keys().all(|k| k == "version" || k == "hooks"))
            .unwrap_or(false);
        if only_known_keys {
            std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
            return Ok(true);
        }
    }
    crate::config::write_json_atomic(&path, &doc)?;
    Ok(true)
}

fn read_doc(path: &std::path::Path) -> Result<Value> {
    if path.exists() {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Ok(serde_json::from_str(&text).unwrap_or_else(|_| json!({})))
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(json!({}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_then_strip_roundtrip() {
        let mut doc = json!({});
        merge_ctx_cursor_hook(&mut doc).unwrap();
        assert_eq!(doc["version"], json!(1));
        let arr = doc["hooks"]["postToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert!(arr[0]["command"]
            .as_str()
            .unwrap()
            .contains(CTX_CURSOR_POST_TOOL_SUBCOMMAND));

        assert!(strip_ctx_cursor_hook(&mut doc));
        assert!(doc.get("hooks").and_then(|h| h.get("postToolUse")).is_none());
    }

    #[test]
    fn merge_is_idempotent() {
        let mut doc = json!({});
        merge_ctx_cursor_hook(&mut doc).unwrap();
        merge_ctx_cursor_hook(&mut doc).unwrap();
        assert_eq!(doc["hooks"]["postToolUse"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn preserves_user_hooks() {
        let mut doc = json!({
            "version": 1,
            "hooks": {
                "postToolUse": [{ "command": "./hooks/user-audit.sh" }],
                "beforeShellExecution": [{ "command": "./hooks/guard.sh" }]
            }
        });
        merge_ctx_cursor_hook(&mut doc).unwrap();
        let post = doc["hooks"]["postToolUse"].as_array().unwrap();
        assert_eq!(post.len(), 2, "user's postToolUse hook must survive");
        assert!(post
            .iter()
            .any(|e| e["command"].as_str() == Some("./hooks/user-audit.sh")));

        assert!(strip_ctx_cursor_hook(&mut doc));
        let post = doc["hooks"]["postToolUse"].as_array().unwrap();
        assert_eq!(post.len(), 1);
        assert_eq!(post[0]["command"].as_str(), Some("./hooks/user-audit.sh"));
        // The user's other event is never touched.
        assert!(doc["hooks"]["beforeShellExecution"].is_array());
    }
}
