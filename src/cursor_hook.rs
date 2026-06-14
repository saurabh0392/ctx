//! Cursor `postToolUse` command hook (ADR 0018 / CTX-27).
//!
//! Cursor runs command hooks the same way Claude Code does: JSON in on stdin, JSON out on stdout.
//! Increment 1 is observe-only. We lift each Cursor tool result into the same canonical
//! [`crate::agent::ToolResult`] the Claude path uses, run the surface-agnostic controller to get
//! the would-do retention decision, and record it stamped `surface = "cursor"`. We do not modify
//! Cursor's tool output yet; acting on MCP outputs via `updated_mcp_tool_output` is increment 2.

use std::io::Read;

use anyhow::Result;
use serde_json::{json, Value};

use crate::agent::ToolResult;
use crate::config::Config;

/// Stable surface tag stamped on every decision this hook records.
pub const CURSOR_SURFACE: &str = "cursor";

/// Read the Cursor postToolUse payload, record a `surface = "cursor"` decision, emit `{}`.
/// Best-effort and always silent on stdout: a hook that fails or has nothing to say must never
/// disturb the Cursor session.
pub fn post_tool_use() -> Result<()> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let payload: Value = serde_json::from_str(buf.trim()).unwrap_or(json!({}));

    let cfg = Config::load();
    if !cfg.compress_enabled {
        return Ok(());
    }

    if let Some(tr) = extract_cursor_tool_result(&payload) {
        let command_or_path = crate::surface::fingerprint_tool_input(&tr.tool_name, &tr.tool_input);
        let decision = crate::agent::decide(&cfg, &tr);
        // Observe-only (increment 1): ctx never rewrites Cursor output here, so `applied` must be
        // false no matter what the controller would do on Claude. Recording `decision.apply` would
        // overstate ("trims applied" on Cursor) and wrongly drop these runs into the causal trimmed
        // arm. The would-do retention still rides along in the shadow decision. Acting on Cursor MCP
        // output is CTX-33; that increment will record a real apply.
        let applied_on_cursor = false;
        crate::compress::record_shadow_decision(
            tr.session_id.as_deref(),
            &tr.tool_name,
            &command_or_path,
            decision.shadow.as_ref(),
            applied_on_cursor,
            decision.explore_arm,
            Some(CURSOR_SURFACE),
        );
    }

    // Observe-only: no output rewrite. An empty object is the safe "nothing to change" reply.
    print!("{}", serde_json::to_string(&json!({}))?);
    Ok(())
}

/// Lift a Cursor `postToolUse` payload into the canonical tool result. Returns `None` when there
/// is no compressible output (a write/delete-style tool, or an empty result), so the caller stays
/// silent rather than recording an empty decision.
///
/// Cursor's payload shape (verified against the hooks docs, ADR 0018):
/// `conversation_id` is the stable session id, `workspace_roots[0]` is the cwd, `tool_name` is the
/// tool type ("Shell", "Read", "Grep", or an MCP tool), and `tool_output` is the result as a
/// JSON-stringified string.
pub fn extract_cursor_tool_result(payload: &Value) -> Option<ToolResult> {
    let tool_name = payload
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if tool_name.is_empty() {
        return None;
    }
    let tool_input = payload
        .get("tool_input")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let raw_output = cursor_tool_output_text(&tool_name, payload.get("tool_output"));
    if raw_output.trim().is_empty() {
        return None;
    }
    let session_id = payload
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let cwd = payload
        .get("workspace_roots")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(ToolResult {
        tool_name,
        tool_input,
        raw_output,
        session_id,
        cwd,
        // Cursor narration is not in the hook payload; the read guard's intent signal stays a
        // Claude-only capability for now (ADR 0011). Observe-only does not need it.
        recent_intent_text: None,
    })
}

/// Turn Cursor's JSON-stringified `tool_output` into the text the compressors reason over.
///
/// Cursor's Shell results carry their terminal text under an `output` key
/// (`{"output":"...","exitCode":0}`), which differs from the `stdout`-shaped example in Cursor's
/// docs and from Claude Code's Bash shape, so we read `output` first. Everything else (Read, Grep,
/// MCP) reuses the same extraction the Claude path uses.
fn cursor_tool_output_text(tool_name: &str, out: Option<&Value>) -> String {
    let out = match out {
        Some(o) => o,
        None => return String::new(),
    };
    // Parse the JSON-stringified payload; pass plain (non-JSON) strings straight through.
    let parsed: Value = if let Some(s) = out.as_str() {
        let trimmed = s.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => return s.to_string(),
            }
        } else {
            return s.to_string();
        }
    } else {
        out.clone()
    };

    // Cursor Shell/terminal results put the text under "output".
    if let Some(o) = parsed.get("output").and_then(|x| x.as_str()) {
        return o.to_string();
    }

    let extract_name = if tool_name.eq_ignore_ascii_case("shell") {
        "Bash"
    } else {
        tool_name
    };
    crate::compress::extract_compressible_text(extract_name, &parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_shell_stdout_from_stringified_output() {
        let payload = json!({
            "conversation_id": "conv-1",
            "generation_id": "gen-1",
            "hook_event_name": "postToolUse",
            "workspace_roots": ["/proj"],
            "tool_name": "Shell",
            "tool_input": {"command": "git status"},
            "tool_output": "{\"exitCode\":0,\"stdout\":\"on branch main\",\"stderr\":\"\"}"
        });
        let tr = extract_cursor_tool_result(&payload).expect("extract");
        assert_eq!(tr.tool_name, "Shell");
        assert_eq!(tr.cwd, "/proj");
        assert_eq!(tr.session_id.as_deref(), Some("conv-1"));
        assert!(
            tr.raw_output.contains("on branch main"),
            "stdout should be lifted, got: {}",
            tr.raw_output
        );
    }

    #[test]
    fn extracts_shell_output_field_real_cursor_shape() {
        // Cursor's real Shell payload uses "output" (not "stdout") and an empty top-level cwd, so
        // the text must come from "output" and the cwd from workspace_roots. Captured live from a
        // Cursor 3.7 postToolUse event (ADR 0018).
        let payload = json!({
            "conversation_id": "conv-9",
            "generation_id": "gen-9",
            "hook_event_name": "postToolUse",
            "workspace_roots": ["/Users/me/proj"],
            "tool_name": "Shell",
            "tool_input": {"command": "ls -la", "cwd": ""},
            "cwd": "",
            "tool_output": "{\"output\":\"total 8\\ndrwxr-xr-x  2 me staff\",\"exitCode\":0}"
        });
        let tr = extract_cursor_tool_result(&payload).expect("extract");
        assert_eq!(tr.tool_name, "Shell");
        assert_eq!(tr.cwd, "/Users/me/proj");
        assert!(
            tr.raw_output.contains("total 8"),
            "Shell 'output' field must be lifted, got: {}",
            tr.raw_output
        );
        assert!(
            !tr.raw_output.contains("exitCode"),
            "should be just the terminal text, not the wrapper json"
        );
    }

    #[test]
    fn none_when_output_empty() {
        let payload = json!({
            "conversation_id": "conv-1",
            "workspace_roots": ["/proj"],
            "tool_name": "Write",
            "tool_input": {"path": "a.rs"},
            "tool_output": ""
        });
        assert!(extract_cursor_tool_result(&payload).is_none());
    }

    #[test]
    fn none_when_tool_name_missing() {
        let payload = json!({
            "conversation_id": "conv-1",
            "tool_output": "something"
        });
        assert!(extract_cursor_tool_result(&payload).is_none());
    }

    #[test]
    fn plain_string_output_passes_through() {
        let payload = json!({
            "conversation_id": "conv-2",
            "workspace_roots": ["/w"],
            "tool_name": "Grep",
            "tool_input": {"pattern": "fn main"},
            "tool_output": "src/main.rs:1:fn main() {}"
        });
        let tr = extract_cursor_tool_result(&payload).expect("extract");
        assert_eq!(tr.tool_name, "Grep");
        assert!(tr.raw_output.contains("fn main"));
    }
}
