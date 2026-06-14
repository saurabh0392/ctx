//! Register ctx's live Cursor hook in `~/.cursor/hooks.json` (ADR 0018 / CTX-27).
//!
//! This mirrors `claude_settings.rs`: merge a single ctx-owned command hook into the user's
//! Cursor hooks file without disturbing other hooks, and strip exactly that entry on uninstall.
//! Cursor's hooks file is schema version 1: `{ "version": 1, "hooks": { "<event>": [ ... ] } }`.

use anyhow::{Context, Result};
use serde_json::{json, Value};

/// ctx subcommand wired as the Cursor postToolUse command hook.
pub const CTX_CURSOR_POST_TOOL_SUBCOMMAND: &str = "hook cursor-post-tool-use";
/// ctx subcommand wired as the Cursor preToolUse Shell hook (CTX-41): it rewrites a Shell command
/// to `ctx run <cmd>` so the compacted result returns as Shell's own output (the RTK approach).
pub const CTX_CURSOR_PRE_TOOL_SUBCOMMAND: &str = "hook cursor-pre-tool-use";

/// One ctx-managed Cursor hook: which event it registers under, the ctx subcommand it runs, and an
/// optional tool matcher (Cursor scopes a hook to a tool type via `matcher`).
struct CtxCursorHook {
    event: &'static str,
    subcommand: &'static str,
    matcher: Option<&'static str>,
}

/// Every Cursor hook ctx owns. postToolUse observes all tools (and trims MCP results); preToolUse
/// is scoped to Shell so only shell commands are considered for the `ctx run` input rewrite.
fn ctx_cursor_hooks() -> [CtxCursorHook; 2] {
    [
        CtxCursorHook {
            event: "postToolUse",
            subcommand: CTX_CURSOR_POST_TOOL_SUBCOMMAND,
            matcher: None,
        },
        CtxCursorHook {
            event: "preToolUse",
            subcommand: CTX_CURSOR_PRE_TOOL_SUBCOMMAND,
            matcher: Some("Shell"),
        },
    ]
}

fn resolve_ctx_cursor_command(subcommand: &str) -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(s) = exe.to_str() {
            if !s.is_empty() {
                return format!("{s} {subcommand}");
            }
        }
    }
    format!("ctx {subcommand}")
}

/// True when this Cursor hook entry is any ctx-managed command hook. ctx subcommands are namespaced
/// `hook cursor-...`, so matching that prefix catches every ctx entry across events.
fn entry_is_ctx_cursor_hook(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(|c| c.as_str())
        .map(|c| c.contains("hook cursor-"))
        .unwrap_or(false)
}

/// Remove every ctx-managed entry from every event, pruning empty containers. Returns true if it
/// changed anything. Scanning all events (not just the ones ctx currently re-adds) means setup
/// reconciles the file to exactly the hooks this ctx binary supports, so a stale ctx entry left by a
/// different ctx version (e.g. a hook subcommand this binary no longer has) gets cleaned up instead
/// of dangling. Every non-ctx hook is left untouched.
pub fn strip_ctx_cursor_hook(doc: &mut Value) -> bool {
    let Some(hooks) = doc.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return false;
    };
    let mut changed = false;
    let event_keys: Vec<String> = hooks.keys().cloned().collect();
    for event in event_keys {
        let Some(arr) = hooks.get_mut(&event).and_then(|a| a.as_array_mut()) else {
            continue;
        };
        let before = arr.len();
        arr.retain(|e| !entry_is_ctx_cursor_hook(e));
        if arr.len() != before {
            changed = true;
        }
        if arr.is_empty() {
            hooks.remove(&event);
        }
    }
    changed
}

/// Merge every ctx Cursor hook into the document, replacing any prior ctx entries first so repeated
/// setups stay idempotent. Leaves the user's own hooks and events untouched.
pub fn merge_ctx_cursor_hook(doc: &mut Value) -> Result<()> {
    if !doc.is_object() {
        *doc = json!({});
    }
    doc["version"] = json!(1);
    if !doc.get("hooks").map(|h| h.is_object()).unwrap_or(false) {
        doc["hooks"] = json!({});
    }
    strip_ctx_cursor_hook(doc);

    for hook in ctx_cursor_hooks() {
        let mut entry = json!({ "command": resolve_ctx_cursor_command(hook.subcommand) });
        if let Some(m) = hook.matcher {
            entry["matcher"] = json!(m);
        }
        doc["hooks"]
            .as_object_mut()
            .context("hooks must be an object")?
            .entry(hook.event.to_string())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .with_context(|| format!("hooks.{} must be an array", hook.event))?
            .push(entry);
    }
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

        let post = doc["hooks"]["postToolUse"].as_array().unwrap();
        assert_eq!(post.len(), 1);
        assert!(post[0]["command"]
            .as_str()
            .unwrap()
            .contains(CTX_CURSOR_POST_TOOL_SUBCOMMAND));

        // preToolUse is registered too, scoped to Shell via matcher.
        let pre = doc["hooks"]["preToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 1);
        assert!(pre[0]["command"]
            .as_str()
            .unwrap()
            .contains(CTX_CURSOR_PRE_TOOL_SUBCOMMAND));
        assert_eq!(pre[0]["matcher"].as_str(), Some("Shell"));

        assert!(strip_ctx_cursor_hook(&mut doc));
        assert!(doc.get("hooks").and_then(|h| h.get("postToolUse")).is_none());
        assert!(doc.get("hooks").and_then(|h| h.get("preToolUse")).is_none());
    }

    #[test]
    fn strips_stale_ctx_entry_from_unmanaged_event() {
        // A ctx entry left under an event this binary no longer manages (e.g. a preCompact hook from
        // a different ctx version) must be reconciled away, not left dangling at a missing subcommand.
        let mut doc = json!({
            "version": 1,
            "hooks": {
                "preCompact": [{ "command": "/abs/ctx hook cursor-pre-compact" }],
                "postToolUse": [{ "command": "./hooks/user-audit.sh" }]
            }
        });
        assert!(strip_ctx_cursor_hook(&mut doc));
        assert!(
            doc["hooks"].get("preCompact").is_none(),
            "stale ctx preCompact entry must be removed"
        );
        assert_eq!(
            doc["hooks"]["postToolUse"].as_array().unwrap().len(),
            1,
            "user's own hook stays"
        );
    }

    #[test]
    fn merge_is_idempotent() {
        let mut doc = json!({});
        merge_ctx_cursor_hook(&mut doc).unwrap();
        merge_ctx_cursor_hook(&mut doc).unwrap();
        assert_eq!(doc["hooks"]["postToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(doc["hooks"]["preToolUse"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn preserves_user_hooks() {
        let mut doc = json!({
            "version": 1,
            "hooks": {
                "postToolUse": [{ "command": "./hooks/user-audit.sh" }],
                "preToolUse": [{ "command": "./hooks/user-pre.sh" }],
                "beforeShellExecution": [{ "command": "./hooks/guard.sh" }]
            }
        });
        merge_ctx_cursor_hook(&mut doc).unwrap();
        let post = doc["hooks"]["postToolUse"].as_array().unwrap();
        assert_eq!(post.len(), 2, "user's postToolUse hook must survive");
        assert!(post
            .iter()
            .any(|e| e["command"].as_str() == Some("./hooks/user-audit.sh")));
        let pre = doc["hooks"]["preToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2, "user's preToolUse hook must survive");

        assert!(strip_ctx_cursor_hook(&mut doc));
        let post = doc["hooks"]["postToolUse"].as_array().unwrap();
        assert_eq!(post.len(), 1);
        assert_eq!(post[0]["command"].as_str(), Some("./hooks/user-audit.sh"));
        let pre = doc["hooks"]["preToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 1, "stripping ctx leaves the user's preToolUse hook");
        assert_eq!(pre[0]["command"].as_str(), Some("./hooks/user-pre.sh"));
        // The user's other event is never touched.
        assert!(doc["hooks"]["beforeShellExecution"].is_array());
    }
}
