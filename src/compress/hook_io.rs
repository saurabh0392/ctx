//! PostToolUse hook I/O: parse Claude Code payload, emit updatedToolOutput.

use std::io::Read;

use anyhow::Result;
use serde_json::{json, Value};

use crate::ab::ab_assign;
use crate::agent::{AgentTransport, ClaudeCodeTransport};
use crate::analytics;
use crate::config::Config;

use super::compress_tool_output;

pub fn post_tool_use() -> Result<()> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let payload: Value = serde_json::from_str(buf.trim()).unwrap_or(json!({}));

    let cfg = Config::load();
    if !cfg.compress_enabled {
        return Ok(());
    }

    // Normalize the native PostToolUse payload into a canonical tool result through the
    // Claude Code surface adapter. Every agent surface goes through this one extraction
    // path, so the controller below never sees an agent-specific shape. Returns early
    // when there is no compressible tool output (no response value, or empty text).
    let Some(tr) = ClaudeCodeTransport.extract(&payload) else {
        return Ok(());
    };
    // The original native response shape is needed to splice compressed text back into
    // the field Claude Code validates (stdout / file.content / mcp content / ...).
    let Some(response_value) = tool_response_value(&payload) else {
        return Ok(());
    };

    // Stable join key shared with the transcript adapters: command, then a path, then
    // the tool name. One definition lives in `surface::fingerprint_tool_input`.
    let command_or_path = crate::surface::fingerprint_tool_input(&tr.tool_name, &tr.tool_input);

    // Surface-agnostic controller: the would-do retention decision (Act 0 self-labeling)
    // plus whether the active preset and the per-tool evidence gate let it apply (Act 1).
    // During the Act 0 window the preset is `off`, so nothing applies and every result is
    // recorded in shadow only.
    let decision = crate::agent::decide(&cfg, &tr);

    record_shadow_decision(
        tr.session_id.as_deref(),
        &tr.tool_name,
        &command_or_path,
        decision.shadow.as_ref(),
        decision.apply,
        decision.explore_arm,
        // Claude Code surface is stamped at outcome-join time, not here (legacy behaviour).
        None,
    );

    if !decision.apply {
        return Ok(());
    }

    // Consume the canonical result into the bindings the apply path below expects.
    let crate::agent::ToolResult {
        tool_name,
        tool_input,
        raw_output: raw,
        session_id,
        cwd,
        recent_intent_text: _,
    } = tr;
    let session_id = session_id.as_deref();
    let cwd = cwd.as_str();
    let tool_name = tool_name.as_str();

    // --- user-facing apply path (only when a tool has earned its turn) ---
    if let Some(ab) = cfg.ab_test.as_ref() {
        let key = crate::ab::request_key(session_id, cwd, tool_name);
        if !ab_assign(ab.compress_pct, "compress", &key) {
            return Ok(());
        }
    }

    let sgr_arm = cfg.compress_sgr_enabled
        && cfg.ab_test.as_ref().map_or(true, |ab| {
            ab_assign(
                ab.compress_sgr_pct,
                "compress_sgr",
                &crate::ab::request_key(session_id, cwd, tool_name),
            )
        });

    let Some(result) =
        compress_tool_output(tool_name, &tool_input, &raw, &cfg, session_id, cwd, sgr_arm)
    else {
        return Ok(());
    };

    let sgr_note = if sgr_arm && cfg.compress_sgr_enabled {
        " Session-grounded retention picked lines for this turn."
    } else {
        ""
    };

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
            &command_or_path,
        );
    }

    analytics::record_compress(
        result.chars_saved(),
        cfg.active_profile.as_deref().unwrap_or("all"),
        cwd,
    );

    let note = format!(
        "ctx compressed this tool output ({} to {} chars). The tool still ran successfully.{}",
        result.chars_in, result.chars_out, sgr_note
    );
    let updated = ClaudeCodeTransport.wrap(tool_name, &response_value, &result.text);
    let out = json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "updatedToolOutput": updated,
            "additionalContext": note
        }
    });
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

/// Persist the would-do retention decision to `compress_decisions` for forward label
/// collection. Best-effort: never fails the hook. `surface` stamps the originating agent for
/// surfaces ctx observes live (e.g. "cursor"); pass `None` for Claude Code, whose surface is
/// stamped at outcome-join time.
pub(crate) fn record_shadow_decision(
    session_id: Option<&str>,
    tool_name: &str,
    command_or_path: &str,
    decision: Option<&super::ShadowDecision>,
    applied: bool,
    explore_arm: Option<&str>,
    surface: Option<&str>,
) {
    let Some(d) = decision else {
        return;
    };
    let cfg_shadow = Config::load().compress_shadow_enabled;
    if !cfg_shadow {
        return;
    }
    let Ok(conn) = crate::db::open_db() else {
        return;
    };
    if crate::db::ensure_schema(&conn).is_err() {
        return;
    }
    let features_json = d.features_json();
    let ts = chrono::Utc::now().to_rfc3339();
    let row = crate::db::CompressDecision {
        ts: &ts,
        session_id,
        tool_name,
        server_prefix: d.server_prefix.as_deref(),
        kind: d.kind_str(),
        task_mode: &d.task_mode,
        lines_total: d.lines_total,
        lines_keep: d.lines_keep,
        lines_drop: d.lines_drop,
        chars_in: d.chars_in,
        would_chars_out: d.would_chars_out,
        features_json: &features_json,
        command_or_path,
        applied,
        explore_arm,
        surface,
    };
    let _ = crate::db::insert_compress_decision(&conn, &row);
}

/// Raw tool result from the PostToolUse hook payload.
pub fn tool_response_value(payload: &Value) -> Option<Value> {
    for key in [
        "tool_response",
        "toolResponse",
        "tool_result",
        "toolResult",
        "tool_output",
        "toolOutput",
    ] {
        if let Some(v) = payload.get(key) {
            if v.is_null() {
                continue;
            }
            return Some(v.clone());
        }
    }
    None
}

/// Text passed into the compressors from a structured or legacy tool response.
pub fn extract_compressible_text(tool_name: &str, response: &Value) -> String {
    if let Some(s) = response.as_str() {
        return s.to_string();
    }
    let Some(obj) = response.as_object() else {
        return fallback_serialize(response);
    };

    match tool_name.trim().to_ascii_lowercase().as_str() {
        "bash" => {
            let stdout = obj.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
            let stderr = obj.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
            if stderr.is_empty() {
                stdout.to_string()
            } else if stdout.is_empty() {
                stderr.to_string()
            } else {
                format!("{stdout}\n{stderr}")
            }
        }
        "read" => read_content_from_object(obj).unwrap_or_else(|| fallback_serialize(response)),
        "grep" | "glob" => obj
            .get("content")
            .or_else(|| obj.get("output"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| fallback_serialize(response)),
        _ if tool_name.starts_with("mcp__") => mcp_text_from_object(response),
        _ => mcp_text_from_object(response),
    }
}

/// Put compressed text back into the tool-native output shape Claude Code validates.
pub fn wrap_updated_tool_output(tool_name: &str, original: &Value, compressed: &str) -> Value {
    if original.is_string() {
        return Value::String(compressed.to_string());
    }
    if !original.is_object() {
        return try_parse_compressed_json(compressed)
            .unwrap_or_else(|| Value::String(compressed.to_string()));
    }

    let mut out = original.clone();
    let obj = out.as_object_mut().expect("object");
    match tool_name.trim().to_ascii_lowercase().as_str() {
        "bash" => {
            obj.insert("stdout".into(), json!(compressed));
        }
        "read" => {
            if let Some(file) = obj.get_mut("file").and_then(|f| f.as_object_mut()) {
                file.insert("content".into(), json!(compressed));
            } else {
                obj.insert("content".into(), json!(compressed));
            }
        }
        "grep" | "glob" => {
            if obj.contains_key("content") {
                obj.insert("content".into(), json!(compressed));
            } else if obj.contains_key("output") {
                obj.insert("output".into(), json!(compressed));
            } else {
                obj.insert("content".into(), json!(compressed));
            }
        }
        _ if tool_name.starts_with("mcp__") => {
            wrap_mcp_object(obj, compressed);
        }
        _ => {
            if obj.contains_key("content") {
                obj.insert("content".into(), json!(compressed));
            } else if let Some(parsed) = try_parse_compressed_json(compressed) {
                return parsed;
            } else {
                obj.insert("content".into(), json!(compressed));
            }
        }
    }
    out
}

pub fn extract_tool_output(payload: &Value) -> String {
    tool_response_value(payload)
        .map(|v| {
            let tool_name = payload
                .get("tool_name")
                .or_else(|| payload.get("toolName"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            extract_compressible_text(tool_name, &v)
        })
        .unwrap_or_default()
}

fn read_content_from_object(obj: &serde_json::Map<String, Value>) -> Option<String> {
    if let Some(file) = obj.get("file") {
        if let Some(content) = file.get("content").and_then(|v| v.as_str()) {
            return Some(content.to_string());
        }
    }
    obj.get("content")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn mcp_text_from_object(response: &Value) -> String {
    if let Some(s) = response.as_str() {
        return s.to_string();
    }
    if let Some(arr) = response.as_array() {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .map(|s| s.to_string())
            .collect();
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }
    if let Some(content) = response.get("content") {
        if let Some(s) = content.as_str() {
            return s.to_string();
        }
        if let Some(arr) = content.as_array() {
            let parts: Vec<String> = arr
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .map(|s| s.to_string())
                .collect();
            if !parts.is_empty() {
                return parts.join("\n");
            }
        }
    }
    fallback_serialize(response)
}

fn wrap_mcp_object(obj: &mut serde_json::Map<String, Value>, compressed: &str) {
    if let Some(parsed) = try_parse_compressed_json(compressed) {
        if parsed.is_object() || parsed.is_array() {
            if obj.contains_key("content") {
                obj.insert("content".into(), parsed);
                return;
            }
            *obj = parsed.as_object().cloned().unwrap_or_else(|| {
                let mut m = serde_json::Map::new();
                m.insert("content".into(), parsed);
                m
            });
            return;
        }
    }
    if obj.get("content").map(|c| c.is_array()).unwrap_or(false) {
        obj.insert(
            "content".into(),
            json!([{"type": "text", "text": compressed}]),
        );
    } else if obj.contains_key("content") {
        obj.insert("content".into(), json!(compressed));
    } else {
        obj.insert(
            "content".into(),
            json!([{"type": "text", "text": compressed}]),
        );
    }
}

fn try_parse_compressed_json(compressed: &str) -> Option<Value> {
    let trimmed = compressed.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

fn fallback_serialize(response: &Value) -> String {
    if let Some(s) = response.as_str() {
        return s.to_string();
    }
    serde_json::to_string_pretty(response).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_string_tool_response() {
        let p = json!({"tool_response": "hello world"});
        assert_eq!(extract_tool_output(&p), "hello world");
    }

    #[test]
    fn extract_content_array() {
        let p = json!({"toolResponse": {"content": [{"type":"text","text":"line1"}]}});
        assert_eq!(extract_tool_output(&p), "line1");
    }

    #[test]
    fn extract_bash_structured_stdout() {
        let resp = json!({
            "stdout": "hello\nworld",
            "stderr": "",
            "interrupted": false,
            "isImage": false
        });
        assert_eq!(extract_compressible_text("Bash", &resp), "hello\nworld");
    }

    #[test]
    fn wrap_bash_preserves_object_shape() {
        let original = json!({
            "stdout": "x".repeat(5000),
            "stderr": "",
            "interrupted": false,
            "isImage": false,
            "noOutputExpected": false
        });
        let wrapped = wrap_updated_tool_output("Bash", &original, "short");
        assert!(wrapped.is_object());
        assert_eq!(
            wrapped.get("stdout").and_then(|v| v.as_str()),
            Some("short")
        );
        assert_eq!(wrapped.get("stderr").and_then(|v| v.as_str()), Some(""));
        assert_eq!(
            wrapped.get("interrupted").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn extract_read_structured_file_content() {
        let resp = json!({
            "file": {
                "filePath": "/tmp/a.rs",
                "content": "fn main() {}\n".repeat(100)
            }
        });
        let text = extract_compressible_text("Read", &resp);
        assert!(text.contains("fn main()"));
    }

    #[test]
    fn wrap_read_preserves_file_object() {
        let original = json!({
            "file": {
                "filePath": "/tmp/a.rs",
                "content": "fn main() {}\n".repeat(100),
                "numLines": 100
            }
        });
        let wrapped = wrap_updated_tool_output("Read", &original, "fn main() {}");
        assert!(wrapped.get("file").is_some());
        assert_eq!(
            wrapped.pointer("/file/content").and_then(|v| v.as_str()),
            Some("fn main() {}")
        );
        assert_eq!(
            wrapped.pointer("/file/filePath").and_then(|v| v.as_str()),
            Some("/tmp/a.rs")
        );
    }
}
