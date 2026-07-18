//! Codex plugin hook transport.
//!
//! Codex can rewrite local function input in `PreToolUse`, but Codex 0.144.5 rejects
//! `updatedMCPToolOutput` from `PostToolUse`. A blocking PostToolUse decision can substitute textual
//! feedback, but it is error-shaped rather than a clean tool-native result replacement. Consequently
//! this module observes every supported local result and acts only by wrapping conservative shell
//! commands before they execute.

use std::hash::{Hash, Hasher};
use std::io::Read;

use anyhow::Result;
use serde_json::{json, Value};

use crate::agent::{AgentTransport, ToolResult};
use crate::config::Config;

pub const CODEX_SURFACE: &str = "codex";

pub struct CodexTransport;

impl AgentTransport for CodexTransport {
    fn agent_name(&self) -> &'static str {
        CODEX_SURFACE
    }

    fn extract(&self, payload: &Value) -> Option<ToolResult> {
        extract_tool_result(payload)
    }

    // Clean tool-native PostToolUse replacement is unsupported. This implementation intentionally
    // returns the original value so generic callers can never mistake the transport for an acting
    // path or use the error-shaped feedback substitution accidentally.
    fn wrap(&self, _tool_name: &str, original: &Value, _compressed: &str) -> Value {
        original.clone()
    }
}

pub fn session_start() -> Result<()> {
    let payload = read_payload()?;
    record_event(&payload, "SessionStart");
    emit(&json!({}))
}

pub fn user_prompt_submit() -> Result<()> {
    let payload = read_payload()?;
    if !claim_event(&payload, "UserPromptSubmit") {
        return emit(&json!({}));
    }

    let prompt = payload
        .get("prompt")
        .or_else(|| payload.get("user_prompt"))
        .or_else(|| payload.get("userPrompt"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let correction = matches!(
        crate::outcome_signals::classify_correction(
            prompt,
            crate::outcome_signals::DEFAULT_TERSE_MAX_CHARS,
        ),
        crate::outcome_signals::CorrectionClass::Explicit
    );
    if let Some(session_id) = string_field(&payload, &["session_id", "sessionId"]) {
        if let Ok(conn) = crate::db::open_db() {
            let _ = crate::db::ensure_schema(&conn);
            let _ = crate::db::close_surface_outcomes_for_prompt(
                &conn,
                CODEX_SURFACE,
                session_id,
                correction,
            );
        }
    }
    emit(&json!({}))
}

pub fn pre_tool_use() -> Result<()> {
    let payload = read_payload()?;
    // A delivery retry still needs the same deterministic rewrite response, so event claiming is
    // telemetry-only here and never suppresses the response.
    record_event(&payload, "PreToolUse");
    let output = decide_pre_tool_use(&Config::load(), &payload).unwrap_or_else(|| json!({}));
    emit(&output)
}

pub fn post_tool_use() -> Result<()> {
    let payload = read_payload()?;
    if !claim_event(&payload, "PostToolUse") {
        return emit(&json!({}));
    }

    // `ctx run` owns both the applied decision and the output accounting. The resulting Bash tool
    // event is only an envelope around that already-recorded wrapper execution.
    if shell_command(&payload)
        .map(crate::cursor_hook::is_ctx_run_wrapped)
        .unwrap_or(false)
    {
        return emit(&json!({}));
    }

    let cfg = Config::load();
    if !cfg.compress_enabled {
        return emit(&json!({}));
    }
    if let Some(tr) = extract_tool_result(&payload) {
        let fingerprint = crate::surface::fingerprint_tool_input(&tr.tool_name, &tr.tool_input);
        if let Some(session_id) = tr.session_id.as_deref() {
            if let Ok(conn) = crate::db::open_db() {
                let _ = crate::db::ensure_schema(&conn);
                let _ =
                    crate::db::mark_surface_retouch(&conn, CODEX_SURFACE, session_id, &fingerprint);
            }
        }
        let decision = crate::agent::decide_for_surface(&cfg, &tr, CODEX_SURFACE);
        // This path is observation-only. Do not persist an exploration arm: Codex did not randomly
        // withhold an otherwise-controllable PostToolUse transform; it cannot perform one at all.
        crate::compress::record_shadow_decision(
            tr.session_id.as_deref(),
            &tr.tool_name,
            &fingerprint,
            decision.shadow.as_ref(),
            false,
            None,
            Some(CODEX_SURFACE),
        );
    }
    emit(&json!({}))
}

pub fn pre_compact() -> Result<()> {
    record_compaction("pre")
}

pub fn post_compact() -> Result<()> {
    record_compaction("post")
}

pub fn stop() -> Result<()> {
    let payload = read_payload()?;
    record_event(&payload, "Stop");
    emit(&json!({}))
}

fn record_compaction(phase: &str) -> Result<()> {
    let payload = read_payload()?;
    let event_name = if phase == "pre" {
        "PreCompact"
    } else {
        "PostCompact"
    };
    let key = event_key(&payload, event_name);
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let _ = crate::db::claim_surface_hook_event(
            &conn,
            &key,
            CODEX_SURFACE,
            event_name,
            string_field(&payload, &["session_id", "sessionId"]),
            string_field(&payload, &["turn_id", "turnId"]),
            None,
        );
        let _ = crate::db::insert_native_compaction(
            &conn,
            &key,
            CODEX_SURFACE,
            phase,
            string_field(&payload, &["session_id", "sessionId"]),
            string_field(&payload, &["turn_id", "turnId"]),
            string_field(&payload, &["trigger", "compact_trigger"]),
        );
    }
    emit(&json!({}))
}

pub fn extract_tool_result(payload: &Value) -> Option<ToolResult> {
    let native_name = string_field(payload, &["tool_name", "toolName"])?;
    let tool_name = normalize_tool_name(native_name);
    let tool_input = payload
        .get("tool_input")
        .or_else(|| payload.get("toolInput"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let response = payload
        .get("tool_response")
        .or_else(|| payload.get("toolResponse"))?;
    let raw_output = crate::compress::extract_compressible_text(&tool_name, response);
    if raw_output.trim().is_empty() {
        return None;
    }
    Some(ToolResult {
        tool_name,
        tool_input,
        raw_output,
        session_id: string_field(payload, &["session_id", "sessionId"]).map(str::to_string),
        cwd: string_field(payload, &["cwd"]).unwrap_or("").to_string(),
        // Transcript parsing is deliberately not part of the live plugin contract.
        recent_intent_text: None,
    })
}

pub fn normalize_tool_name(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "bash" | "shell" | "exec_command" | "unified_exec" => "Shell".to_string(),
        "applypatch" | "apply_patch" => "apply_patch".to_string(),
        "edit" => "Edit".to_string(),
        "write" => "Write".to_string(),
        _ => name.trim().to_string(),
    }
}

fn decide_pre_tool_use(cfg: &Config, payload: &Value) -> Option<Value> {
    if !cfg.compress_enabled {
        return None;
    }
    let native_name = string_field(payload, &["tool_name", "toolName"])?;
    if normalize_tool_name(native_name) != "Shell" {
        return None;
    }
    let command = shell_command(payload)?;
    let session_id = string_field(payload, &["session_id", "sessionId"]);
    let wrapped = crate::cursor_hook::decide_shell_rewrite_for_surface(
        cfg,
        command,
        CODEX_SURFACE,
        session_id,
    )?;
    let mut updated = payload
        .get("tool_input")
        .or_else(|| payload.get("toolInput"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    updated.insert("command".into(), json!(wrapped));
    Some(json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "updatedInput": Value::Object(updated)
        }
    }))
}

fn shell_command(payload: &Value) -> Option<&str> {
    payload
        .get("tool_input")
        .or_else(|| payload.get("toolInput"))
        .and_then(|v| v.get("command"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}

fn string_field<'a>(payload: &'a Value, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| payload.get(*name).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
}

fn read_payload() -> Result<Value> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    Ok(serde_json::from_str(input.trim()).unwrap_or_else(|_| json!({})))
}

fn emit(value: &Value) -> Result<()> {
    print!("{}", serde_json::to_string(value)?);
    Ok(())
}

fn event_key(payload: &Value, event_name: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    CODEX_SURFACE.hash(&mut hasher);
    event_name.hash(&mut hasher);
    string_field(payload, &["session_id", "sessionId"])
        .unwrap_or("")
        .hash(&mut hasher);
    string_field(payload, &["turn_id", "turnId"])
        .unwrap_or("")
        .hash(&mut hasher);
    string_field(payload, &["tool_use_id", "toolUseId"])
        .unwrap_or("")
        .hash(&mut hasher);
    string_field(payload, &["source", "trigger"])
        .unwrap_or("")
        .hash(&mut hasher);
    format!("codex-{event_name}-{:016x}", hasher.finish())
}

fn claim_event(payload: &Value, event_name: &str) -> bool {
    let key = event_key(payload, event_name);
    let Ok(conn) = crate::db::open_db() else {
        // Fail open: DB unavailability must not block the Codex session.
        return true;
    };
    let _ = crate::db::ensure_schema(&conn);
    crate::db::claim_surface_hook_event(
        &conn,
        &key,
        CODEX_SURFACE,
        event_name,
        string_field(payload, &["session_id", "sessionId"]),
        string_field(payload, &["turn_id", "turnId"]),
        string_field(payload, &["tool_use_id", "toolUseId"]),
    )
    .unwrap_or(true)
}

fn record_event(payload: &Value, event_name: &str) {
    let _ = claim_event(payload, event_name);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trial_cfg() -> Config {
        Config {
            compress_enabled: true,
            compress_trial_tools: vec!["Shell".into()],
            ..Default::default()
        }
    }

    #[test]
    fn extracts_verified_bash_post_tool_payload() {
        let payload = json!({
            "hook_event_name": "PostToolUse",
            "session_id": "session-1",
            "turn_id": "turn-1",
            "tool_use_id": "exec-1",
            "tool_name": "Bash",
            "tool_input": {"command": "git status"},
            "tool_response": "On branch main\n",
            "cwd": "/work/repo"
        });
        let result = extract_tool_result(&payload).expect("tool result");
        assert_eq!(result.tool_name, "Shell");
        assert_eq!(result.session_id.as_deref(), Some("session-1"));
        assert_eq!(result.raw_output, "On branch main\n");
    }

    #[test]
    fn emits_verified_codex_pre_tool_rewrite_shape() {
        let payload = json!({
            "hook_event_name": "PreToolUse",
            "session_id": "session-1",
            "tool_name": "Bash",
            "tool_input": {"command": "git status", "timeout": 1000}
        });
        let out = decide_pre_tool_use(&trial_cfg(), &payload).expect("rewrite");
        assert_eq!(
            out["hookSpecificOutput"]["hookEventName"],
            json!("PreToolUse")
        );
        assert_eq!(
            out["hookSpecificOutput"]["permissionDecision"],
            json!("allow")
        );
        let command = out["hookSpecificOutput"]["updatedInput"]["command"]
            .as_str()
            .unwrap();
        assert!(command.contains("--surface 'codex'"));
        assert!(command.contains("--session 'session-1'"));
        assert_eq!(
            out["hookSpecificOutput"]["updatedInput"]["timeout"],
            json!(1000)
        );
    }

    #[test]
    fn post_tool_transport_never_replaces_output() {
        let t = CodexTransport;
        let original = json!({"content": [{"type": "text", "text": "whole"}]});
        assert_eq!(t.wrap("mcp__x__y", &original, "short"), original);
    }

    #[test]
    fn malformed_and_non_shell_pre_tool_payloads_are_inert() {
        assert!(decide_pre_tool_use(&trial_cfg(), &json!({})).is_none());
        assert!(decide_pre_tool_use(
            &trial_cfg(),
            &json!({"tool_name":"apply_patch","tool_input":{"patch":"x"}})
        )
        .is_none());
    }
}
