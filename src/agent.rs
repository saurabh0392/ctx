//! Act 3: agent-agnostic controller interface.
//!
//! The retention controller is transport-independent: it takes a tool result and a task
//! context and returns a decision. The platform's compaction only helps one agent; one
//! learned model keyed by **repo + task** (not by agent) is the portfolio no single
//! vendor will build. This module abstracts the per-agent plumbing behind one trait so
//! Claude Code, Cursor, and Codex all drive the same brain.
//!
//! `ClaudeCodeTransport` is the reference implementation; it reuses the existing
//! PostToolUse extract/wrap helpers. New agents implement `AgentTransport` and get the
//! same shadow collection, evidence gate, and learned model for free.

use serde_json::Value;

use crate::compress::{self, ShadowDecision};
use crate::config::Config;

/// A tool result lifted out of an agent's native payload shape.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_name: String,
    pub tool_input: Value,
    pub raw_output: String,
    pub session_id: Option<String>,
    pub cwd: String,
}

/// What the controller decided for one tool result, independent of agent.
#[derive(Debug, Clone)]
pub struct ControllerDecision {
    pub shadow: Option<ShadowDecision>,
    /// Whether user-facing compression applies (preset allows the kind AND the tool
    /// cleared its evidence gate).
    pub apply: bool,
    pub kind_label: String,
}

/// One agent's plumbing: how to read a tool result out of its payload, and how to put a
/// compressed result back into the shape that agent validates.
pub trait AgentTransport {
    fn agent_name(&self) -> &'static str;
    fn extract(&self, payload: &Value) -> Option<ToolResult>;
    fn wrap(&self, tool_name: &str, original: &Value, compressed: &str) -> Value;
}

/// Compute the controller decision for a tool result. Pure with respect to the agent:
/// every transport runs the same shadow computation, preset check, and evidence gate.
pub fn decide(cfg: &Config, tr: &ToolResult) -> ControllerDecision {
    let shadow = compress::compute_shadow_decision(
        &tr.tool_name,
        &tr.tool_input,
        &tr.raw_output,
        cfg,
        tr.session_id.as_deref(),
        &tr.cwd,
    );
    let kind_label = shadow
        .as_ref()
        .map(|d| d.kind_str().to_string())
        .unwrap_or_else(|| "generic".to_string());
    // A deliberate trial trims the chosen tool live even while the preset stays off and the
    // evidence gate is unmet (the gate cannot pass before any trimmed data exists). Otherwise
    // the normal path: the preset must allow the kind AND the tool must have earned activation.
    let base_apply = cfg.compress_trialing(&tr.tool_name)
        || (cfg.compress_applies_kind(&kind_label)
            && compress::activation::tool_activated(cfg, &tr.tool_name, &kind_label));
    // Edit-intent guard (ADR 0001 / CTX-8): a Read only applies for reference reads (files the
    // agent is not positioned to edit). Working reads of editable project files are never trimmed,
    // even under a trial or after activation, so a re-trial cannot re-create the observed harm.
    let read_guard_blocks = cfg.compress_read_edit_guard
        && kind_label == "read"
        && !compress::edit_intent::read_is_trim_eligible(read_file_path(&tr.tool_input), &tr.cwd);
    let apply = base_apply && !read_guard_blocks;
    ControllerDecision {
        shadow,
        apply,
        kind_label,
    }
}

/// The file path a Read/Edit tool input points at, used by the edit-intent guard. Mirrors the
/// extraction in `compress::shadow::compute_shadow_decision` (file_path, then path).
fn read_file_path(tool_input: &Value) -> Option<&str> {
    tool_input
        .get("file_path")
        .or_else(|| tool_input.get("path"))
        .and_then(|v| v.as_str())
}

/// Reference transport for Claude Code PostToolUse payloads. Delegates to the existing
/// extract/wrap helpers so there is a single source of truth for the wire shape.
pub struct ClaudeCodeTransport;

impl AgentTransport for ClaudeCodeTransport {
    fn agent_name(&self) -> &'static str {
        "claude-code"
    }

    fn extract(&self, payload: &Value) -> Option<ToolResult> {
        let tool_name = payload
            .get("tool_name")
            .or_else(|| payload.get("toolName"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let tool_input = payload
            .get("tool_input")
            .or_else(|| payload.get("toolInput"))
            .cloned()
            .unwrap_or(serde_json::json!({}));
        let response = compress::tool_response_value(payload)?;
        let raw_output = compress::extract_compressible_text(&tool_name, &response);
        if raw_output.is_empty() {
            return None;
        }
        let session_id = payload
            .get("session_id")
            .or_else(|| payload.get("sessionId"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let cwd = payload
            .get("cwd")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Some(ToolResult {
            tool_name,
            tool_input,
            raw_output,
            session_id,
            cwd,
        })
    }

    fn wrap(&self, tool_name: &str, original: &Value, compressed: &str) -> Value {
        compress::wrap_updated_tool_output(tool_name, original, compressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_transport_extracts_bash_output() {
        let t = ClaudeCodeTransport;
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "git status"},
            "tool_response": {"stdout": "on branch main", "stderr": ""},
            "cwd": "/proj",
            "session_id": "s1"
        });
        let tr = t.extract(&payload).expect("extract");
        assert_eq!(tr.tool_name, "Bash");
        assert_eq!(tr.cwd, "/proj");
        assert!(tr.raw_output.contains("branch"));
    }

    #[test]
    fn decision_is_shadow_only_when_preset_off() {
        let cfg = Config {
            compress_preset: crate::config::CompressPreset::Off,
            ..Default::default()
        };
        let tr = ToolResult {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "git status"}),
            raw_output: "a\n".repeat(500),
            session_id: None,
            cwd: "/proj".into(),
        };
        let d = decide(&cfg, &tr);
        assert!(!d.apply, "preset off must never apply");
        assert!(d.shadow.is_some());
    }

    #[test]
    fn trial_tool_applies_even_with_preset_off() {
        // A deliberate trial trims the chosen tool live to collect the "after" arm, even though
        // the preset is off and the tool has not earned the evidence gate.
        let cfg = Config {
            compress_enabled: true,
            compress_preset: crate::config::CompressPreset::Off,
            compress_trial_tools: vec!["Bash".into()],
            ..Default::default()
        };
        let tr = ToolResult {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "git status"}),
            raw_output: "a\n".repeat(500),
            session_id: None,
            cwd: "/proj".into(),
        };
        let d = decide(&cfg, &tr);
        assert!(d.apply, "a trialed tool must apply even with preset off");

        // A tool that is not under trial stays shadow-only on preset off.
        let other = ToolResult {
            tool_name: "Read".into(),
            ..tr.clone()
        };
        assert!(!decide(&cfg, &other).apply, "non-trialed tools must stay shadow only");
    }

    fn read_trial_cfg(guard: bool) -> Config {
        Config {
            compress_enabled: true,
            compress_preset: crate::config::CompressPreset::Off,
            compress_trial_tools: vec!["Read".into()],
            compress_read_edit_guard: guard,
            ..Default::default()
        }
    }

    fn read_tr(file_path: &str) -> ToolResult {
        ToolResult {
            tool_name: "Read".into(),
            tool_input: json!({ "file_path": file_path }),
            raw_output: "line\n".repeat(500),
            session_id: None,
            cwd: "/proj".into(),
        }
    }

    #[test]
    fn edit_guard_blocks_trimming_a_project_read_even_under_trial() {
        // The exact harm from CTX-8: a trialed Read of an editable project file must not trim.
        let cfg = read_trial_cfg(true);
        let d = decide(&cfg, &read_tr("src/foo.rs"));
        assert!(!d.apply, "guard must block trimming an editable project read");
        assert!(d.shadow.is_some(), "shadow decision is still recorded (would-trim)");
    }

    #[test]
    fn edit_guard_allows_trimming_a_reference_read_under_trial() {
        let cfg = read_trial_cfg(true);
        let d = decide(&cfg, &read_tr("/proj/node_modules/react/index.js"));
        assert!(d.apply, "reference reads stay trim-eligible under the guard");
    }

    #[test]
    fn edit_guard_off_restores_pre_guard_trial_behavior() {
        let cfg = read_trial_cfg(false);
        let d = decide(&cfg, &read_tr("src/foo.rs"));
        assert!(d.apply, "with the guard off, a trial trims regardless of edit intent");
    }
}
