//! Cursor `postToolUse` command hook (ADR 0018 / CTX-27, CTX-33).
//!
//! Cursor runs command hooks the same way Claude Code does: JSON in on stdin, JSON out on stdout.
//! We lift each Cursor tool result into the same canonical [`crate::agent::ToolResult`] the Claude
//! path uses and run the surface-agnostic controller to get the would-do retention decision,
//! recorded stamped `surface = "cursor"`.
//!
//! Cursor lets a `postToolUse` hook replace tool output only for MCP tools, via
//! `updated_mcp_tool_output` (ADR 0018). So this hook *acts* (CTX-33) on MCP results when the gate
//! says trim, recording a real apply, and stays observe-only for built-in Read/Shell/Grep, which
//! Cursor will not let a hook rewrite. We never claim parity with Claude here.

use std::io::Read;

use anyhow::Result;
use serde_json::{json, Value};

use crate::agent::ToolResult;
use crate::compress::CompressResult;
use crate::config::Config;

/// Stable surface tag stamped on every decision this hook records.
pub const CURSOR_SURFACE: &str = "cursor";

/// Read the Cursor postToolUse payload, record a `surface = "cursor"` decision, and for MCP tools
/// that have earned a trim, emit `updated_mcp_tool_output` with the shortened result. Best-effort:
/// a hook that fails or has nothing to act on emits `{}` and never disturbs the Cursor session.
pub fn post_tool_use() -> Result<()> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let payload: Value = serde_json::from_str(buf.trim()).unwrap_or(json!({}));

    let cfg = Config::load();
    if !cfg.compress_enabled {
        print!("{{}}");
        return Ok(());
    }

    let mut output = json!({});
    if let Some(tr) = extract_cursor_tool_result(&payload) {
        let command_or_path = crate::surface::fingerprint_tool_input(&tr.tool_name, &tr.tool_input);
        let decision = crate::agent::decide(&cfg, &tr);

        // On Cursor, ctx can replace output only for MCP tools (`updated_mcp_tool_output`); built-in
        // Read/Shell/Grep stay observe-only because Cursor will not let a hook rewrite them (ADR
        // 0018). So a trim is applied here only when the gate says apply AND this is an MCP tool AND
        // the compressor actually shortened the result. Anything else stays `applied = false`, so a
        // trim ctx did not perform is never recorded as one (the honesty rule from ADR 0020).
        let mut applied = false;
        if decision.apply && crate::compress::classify::is_mcp_tool(&tr.tool_name) {
            if let Some(result) = crate::compress::compress_tool_output(
                &tr.tool_name,
                &tr.tool_input,
                &tr.raw_output,
                &cfg,
                tr.session_id.as_deref(),
                &tr.cwd,
                false,
            ) {
                if result.chars_saved() > 0 {
                    let updated =
                        cursor_mcp_updated_output(payload.get("tool_output"), &result.text);
                    record_cursor_apply(
                        tr.session_id.as_deref(),
                        &tr.tool_name,
                        &command_or_path,
                        &result,
                        &cfg,
                        &tr.cwd,
                    );
                    let note = format!(
                        "ctx trimmed this MCP result ({} to {} chars) to save context. The tool still ran in full.",
                        result.chars_in, result.chars_out
                    );
                    output = json!({
                        "updated_mcp_tool_output": updated,
                        "additional_context": note,
                    });
                    applied = true;
                }
            }
        }

        crate::compress::record_shadow_decision(
            tr.session_id.as_deref(),
            &tr.tool_name,
            &command_or_path,
            decision.shadow.as_ref(),
            applied,
            decision.explore_arm,
            Some(CURSOR_SURFACE),
        );
    }

    print!("{}", serde_json::to_string(&output)?);
    Ok(())
}

/// Cursor `preCompact` command hook (CTX-31 increment 1, ADR 0023). Cursor fires this just before
/// it compacts a conversation. Cursor's transcript carries no compaction marker, so this live event
/// is the only honest signal that a Cursor compaction happened. We persist it (best-effort) so the
/// compaction-harm view can show a real, lower-confidence count for Cursor instead of "not visible
/// yet". Purely observational: we never block or alter the compaction, and always emit `{}`.
pub fn pre_compact() -> Result<()> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let payload: Value = serde_json::from_str(buf.trim()).unwrap_or(json!({}));

    let cfg = Config::load();
    if !cfg.compress_enabled {
        print!("{{}}");
        return Ok(());
    }

    let event = parse_cursor_compaction(&payload);
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let _ = crate::db::insert_cursor_compaction(&conn, &event);
    }

    print!("{{}}");
    Ok(())
}

/// Lift a Cursor `preCompact` payload into a [`crate::db::CursorCompaction`]. `conversation_id` is
/// the stable session id (Cursor also sends `session_id` as the same value on some events, so we
/// fall back to it). Every metric is optional: a missing field is recorded as NULL rather than
/// guessed, so the persisted row never overstates what Cursor told us.
pub fn parse_cursor_compaction(payload: &Value) -> crate::db::CursorCompaction {
    let session_id = payload
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("session_id").and_then(|v| v.as_str()))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    crate::db::CursorCompaction {
        ts: chrono::Utc::now().to_rfc3339(),
        session_id,
        trigger: payload
            .get("trigger")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        context_usage_percent: payload.get("context_usage_percent").and_then(|v| v.as_f64()),
        context_tokens: payload.get("context_tokens").and_then(|v| v.as_i64()),
        context_window_size: payload.get("context_window_size").and_then(|v| v.as_i64()),
        message_count: payload.get("message_count").and_then(|v| v.as_i64()),
        messages_to_compact: payload.get("messages_to_compact").and_then(|v| v.as_i64()),
        is_first_compaction: payload.get("is_first_compaction").and_then(|v| v.as_bool()),
    }
}

/// Build Cursor's `updated_mcp_tool_output` from the trimmed text, mirroring the MCP result
/// envelope Cursor sends in. Verified live against a real Cursor 3.7 postToolUse payload (ADR
/// 0018): `tool_output` is a JSON-stringified `{"content":[{"type":"text","text":...}],
/// "isError":false}`. We parse it so sibling fields (e.g. `isError`) survive, and replace only the
/// text content with the trimmed text, so the model reads the shorter result in the same shape.
fn cursor_mcp_updated_output(original_output: Option<&Value>, compressed: &str) -> Value {
    let mut env = original_output
        .and_then(|v| v.as_str())
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    env.insert(
        "content".into(),
        json!([{ "type": "text", "text": compressed }]),
    );
    env.entry("isError".to_string()).or_insert(json!(false));
    Value::Object(env)
}

/// Record a live Cursor MCP trim as a real apply: a compress_event for the savings feed and the
/// analytics counter, mirroring the Claude apply path so the cross-surface view shows Cursor's
/// savings (CTX-33). The decision row's applied flag is set by the caller via
/// `record_shadow_decision`; this only adds the event and counter.
fn record_cursor_apply(
    session_id: Option<&str>,
    tool_name: &str,
    command_or_path: &str,
    result: &CompressResult,
    cfg: &Config,
    cwd: &str,
) {
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let _ = crate::db::insert_compress_event(
            &conn,
            &chrono::Utc::now().to_rfc3339(),
            session_id,
            tool_name,
            &result.strategy,
            result.chars_in,
            result.chars_out,
            command_or_path,
        );
    }
    crate::analytics::record_compress(
        result.chars_saved(),
        cfg.active_profile.as_deref().unwrap_or("all"),
        cwd,
    );
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

    #[test]
    fn cursor_mcp_tool_name_is_detected() {
        // Cursor names MCP tools `MCP:<tool>`; Claude uses `mcp__server__tool`. Both must read as MCP
        // so only MCP results get an apply path. Built-ins must not.
        assert!(crate::compress::classify::is_mcp_tool("MCP:get_issue"));
        assert!(crate::compress::classify::is_mcp_tool("mcp__linear__get_issue"));
        assert!(!crate::compress::classify::is_mcp_tool("Shell"));
        assert!(!crate::compress::classify::is_mcp_tool("Read"));
    }

    #[test]
    fn updated_mcp_output_replaces_text_and_keeps_envelope() {
        // Real Cursor MCP envelope shape (verified live, ADR 0018): a JSON-stringified
        // {"content":[{"type":"text","text":...}],"isError":false}. The trim must land in the text
        // content and leave isError intact, so the model reads a shorter result in the same shape.
        let original = json!(
            "{\"content\":[{\"type\":\"text\",\"text\":\"a very long original result\"}],\"isError\":false}"
        );
        let updated = cursor_mcp_updated_output(Some(&original), "short");
        assert_eq!(updated["isError"], json!(false));
        assert_eq!(updated["content"][0]["type"], json!("text"));
        assert_eq!(updated["content"][0]["text"], json!("short"));
    }

    #[test]
    fn parses_cursor_pre_compact_payload() {
        // A Cursor preCompact payload (shape per the hooks docs, CTX-31). Every metric must land,
        // and conversation_id must be the session id so the row joins to live Cursor activity.
        let payload = json!({
            "conversation_id": "conv-42",
            "hook_event_name": "preCompact",
            "trigger": "auto",
            "context_usage_percent": 91.5,
            "context_tokens": 184000,
            "context_window_size": 200000,
            "message_count": 128,
            "messages_to_compact": 40,
            "is_first_compaction": true
        });
        let c = parse_cursor_compaction(&payload);
        assert_eq!(c.session_id.as_deref(), Some("conv-42"));
        assert_eq!(c.trigger.as_deref(), Some("auto"));
        assert_eq!(c.context_usage_percent, Some(91.5));
        assert_eq!(c.context_tokens, Some(184000));
        assert_eq!(c.context_window_size, Some(200000));
        assert_eq!(c.message_count, Some(128));
        assert_eq!(c.messages_to_compact, Some(40));
        assert_eq!(c.is_first_compaction, Some(true));
        assert!(!c.ts.is_empty());
    }

    #[test]
    fn parses_minimal_pre_compact_payload_without_guessing() {
        // A sparse payload: only what Cursor sent is recorded, everything else stays None (NULL),
        // so the row never overstates the signal. session_id falls back to `session_id`.
        let payload = json!({
            "session_id": "sess-7",
            "hook_event_name": "preCompact"
        });
        let c = parse_cursor_compaction(&payload);
        assert_eq!(c.session_id.as_deref(), Some("sess-7"));
        assert_eq!(c.trigger, None);
        assert_eq!(c.context_usage_percent, None);
        assert_eq!(c.message_count, None);
        assert_eq!(c.is_first_compaction, None);
    }

    #[test]
    fn updated_mcp_output_handles_missing_or_unparsable_original() {
        // No original (or a non-JSON one): still return a valid MCP envelope carrying the trimmed
        // text, defaulting isError to false, so Cursor always gets a well-formed replacement.
        let updated = cursor_mcp_updated_output(None, "trimmed");
        assert_eq!(updated["content"][0]["text"], json!("trimmed"));
        assert_eq!(updated["isError"], json!(false));
    }
}
