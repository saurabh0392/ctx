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

use std::{borrow::Cow, io::Read, io::Write};

use anyhow::Result;
use serde_json::{json, Value};

use crate::agent::ToolResult;
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

    // Dedup with the preToolUse Shell rewrite (CTX-41): a command we rewrote to `ctx run …` was
    // already compacted and accounted for by the wrapper. Recording it again here would double
    // count, so this hook stays out of the way for ctx-wrapped commands.
    if cursor_shell_command(&payload)
        .map(|c| is_ctx_run_wrapped(&c))
        .unwrap_or(false)
    {
        print!("{{}}");
        return Ok(());
    }

    let mut output = json!({});
    if let Some(tr) = extract_cursor_tool_result(&payload, true) {
        let command_or_path = crate::surface::fingerprint_tool_input(&tr.tool_name, &tr.tool_input);
        let decision = crate::agent::decide_for_surface(&cfg, &tr, CURSOR_SURFACE);

        // On Cursor, ctx can replace output only for MCP tools (`updated_mcp_tool_output`); built-in
        // Read/Shell/Grep stay observe-only because Cursor will not let a hook rewrite them (ADR
        // 0018). So a trim is applied here only when the gate says apply AND this is an MCP tool AND
        // the compressor actually shortened the result. Anything else stays `applied = false`, so a
        // trim ctx did not perform is never recorded as one (the honesty rule from ADR 0020).
        let mut applied_by_transaction = false;
        if decision.apply && crate::compress::classify::is_mcp_tool(&tr.tool_name) {
            if let (Some(canonical), Ok(candidate)) = (
                tr.canonical_mcp.as_ref(),
                tr.canonical_mcp
                    .as_ref()
                    .map_or(Err("not-mcp"), |canonical| {
                        crate::compress::propose_mcp_apply_candidate(
                            canonical,
                            None,
                            &tr.tool_input,
                            &cfg,
                            &tr.cwd,
                        )
                    }),
            ) {
                let server_id = mcp_server_id(&tr.tool_name);
                let request = crate::tool_result::McpApplyRequest {
                    surface: CURSOR_SURFACE,
                    server_id: &server_id,
                    protocol_version: "cursor-native-hook-v1",
                    tool_name: &tr.tool_name,
                    tool_input: &tr.tool_input,
                    session_id: tr.session_id.as_deref(),
                    command_or_path: &command_or_path,
                    contract: None,
                    manifest: candidate.manifest,
                    proposal: &candidate.proposal,
                    authorized: true,
                };
                if let crate::tool_result::McpPrepareOutcome::Ready(prepared) =
                    crate::tool_result::prepare_mcp_trim(canonical, &request)
                {
                    let note = format!(
                        "ctx shortened this MCP result from {} to {} characters; the exact original is recoverable as {}.",
                        prepared.validated.chars_in,
                        prepared.validated.chars_out,
                        prepared.rewind_id
                    );
                    output = json!({
                        "updated_mcp_tool_output": prepared.result,
                        "additional_context": note,
                    });
                    let encoded = serde_json::to_vec(&output)?;
                    let mut stdout = std::io::stdout().lock();
                    stdout.write_all(&encoded)?;
                    stdout.flush()?;
                    if let Err(error) = crate::tool_result::mark_mcp_trim_emitted(&prepared) {
                        eprintln!("ctx Cursor hook: emitted trim receipt failed: {error}");
                    }
                    applied_by_transaction = true;
                }
            }
        }

        if !applied_by_transaction {
            crate::compress::record_shadow_decision(
                tr.session_id.as_deref(),
                &tr.tool_name,
                &command_or_path,
                decision.shadow.as_ref(),
                false,
                decision.explore_arm,
                Some(CURSOR_SURFACE),
            );
        } else {
            return Ok(());
        }
    }

    print!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn mcp_server_id(tool_name: &str) -> String {
    tool_name
        .split("__")
        .nth(1)
        .filter(|part| !part.is_empty())
        .unwrap_or("cursor-native")
        .to_owned()
}

#[cfg(test)]
fn cursor_mcp_updated_output(original_output: Option<&Value>, compressed: &str) -> Value {
    let mut env = original_output
        .and_then(Value::as_str)
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    env.insert(
        "content".into(),
        json!([{ "type": "text", "text": compressed }]),
    );
    env.entry("isError").or_insert(json!(false));
    Value::Object(env)
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
        let key = format!(
            "cursor-compact-{}-{}-{}",
            event.session_id.as_deref().unwrap_or("unknown"),
            event.trigger.as_deref().unwrap_or("unknown"),
            event.ts
        );
        let _ = crate::db::insert_native_compaction(
            &conn,
            &key,
            CURSOR_SURFACE,
            "pre",
            event.session_id.as_deref(),
            None,
            event.trigger.as_deref(),
        );
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
        context_usage_percent: payload
            .get("context_usage_percent")
            .and_then(|v| v.as_f64()),
        context_tokens: payload.get("context_tokens").and_then(|v| v.as_i64()),
        context_window_size: payload.get("context_window_size").and_then(|v| v.as_i64()),
        message_count: payload.get("message_count").and_then(|v| v.as_i64()),
        messages_to_compact: payload.get("messages_to_compact").and_then(|v| v.as_i64()),
        is_first_compaction: payload.get("is_first_compaction").and_then(|v| v.as_bool()),
    }
}

/// Cursor `preToolUse` Shell hook (CTX-41 / ADR 0024).
///
/// Cursor will not let a `postToolUse` hook rewrite built-in Shell *output* (ADR 0018/0021), but it
/// lets a `preToolUse` hook rewrite the Shell *command* before it runs via `updated_input`. So when
/// a shell command is safe to wrap and Shell has earned trimming, this rewrites `<cmd>` to
/// `ctx run <cmd>`. The wrapper runs the real command, re-checks the same gate with the real output,
/// and either compacts or passes through. Anything else emits `{}` and leaves the command untouched.
pub fn pre_tool_use() -> Result<()> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let payload: Value = serde_json::from_str(buf.trim()).unwrap_or(json!({}));

    let cfg = Config::load();
    let output = decide_pre_tool_use(&cfg, &payload).unwrap_or_else(|| json!({}));
    print!("{}", serde_json::to_string(&output)?);
    Ok(())
}

/// Pure decision for the preToolUse hook: given config and the Cursor payload, return the rewrite
/// response, or `None` to emit `{}` (no change). Split out so it is unit-testable without stdio.
fn decide_pre_tool_use(cfg: &Config, payload: &Value) -> Option<Value> {
    if !cfg.compress_enabled {
        return None;
    }
    // Scoped to Shell by the hook matcher, but verify defensively.
    let tool_name = payload.get("tool_name").and_then(|v| v.as_str())?;
    if !tool_name.eq_ignore_ascii_case("shell") {
        return None;
    }
    let command = cursor_shell_command(payload)?;
    let wrapped = decide_shell_rewrite(cfg, &command)?;
    let mut updated = payload
        .get("tool_input")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    updated.insert("command".into(), json!(wrapped));
    // Pair the rewrite with permission "allow", matching the proven RTK pattern. preToolUse does not
    // drive the shell approval prompt (Cursor does not enforce "ask" here; beforeShellExecution is
    // the approval gate), so this does not bypass the user's command approval.
    Some(json!({
        "permission": "allow",
        "updated_input": Value::Object(updated),
    }))
}

/// Decide whether to wrap a shell command in `ctx run`. Returns the wrapped command string, or
/// `None` to leave it alone. Two conditions must hold: the command is on the safe, non-interactive
/// allowlist (so capturing its output can never break an editor/pager/REPL), and Shell has earned
/// trimming for this command's kind (trial, activation, or burn-in). The wrapper still re-checks the
/// gate against the real output, so this is the cheap front gate, not the final say.
pub(crate) fn decide_shell_rewrite(cfg: &Config, command: &str) -> Option<String> {
    decide_shell_rewrite_for_surface(cfg, command, CURSOR_SURFACE, None)
}

/// Shared conservative shell rewrite used by Cursor and Codex. The surface and session travel
/// into the wrapper as explicit provenance, so the post-execution gate cannot accidentally borrow
/// another transport's evidence.
pub(crate) fn decide_shell_rewrite_for_surface(
    cfg: &Config,
    command: &str,
    surface: &str,
    session_id: Option<&str>,
) -> Option<String> {
    let trimmed = command.trim();
    if trimmed.is_empty() || is_ctx_run_wrapped(trimmed) {
        return None;
    }
    if !is_safe_to_wrap(trimmed) {
        return None;
    }

    let kind = crate::compress::classify::classify_tool("Shell", Some(trimmed), None);
    let kind_label = crate::compress::shadow::kind_str(kind);
    let eligible = cfg.compress_trialing("Shell")
        || (cfg.compress_applies_kind(kind_label)
            && (crate::compress::activation::tool_activated_on_surface(
                cfg, "Shell", kind_label, surface,
            ) || crate::compress::activation::tool_in_burn_in_on_surface(
                cfg, "Shell", surface,
            )));
    if !eligible {
        return None;
    }

    let session = session_id
        .filter(|s| !s.is_empty())
        .map(|s| format!(" --session {}", shell_quote_for_host(s)))
        .unwrap_or_default();
    #[cfg(windows)]
    let wrapped = format!(
        "& {} run --surface {}{} -- {}",
        shell_quote_for_host(&ctx_exe()),
        shell_quote_for_host(surface),
        session,
        shell_quote_for_host(trimmed)
    );
    #[cfg(not(windows))]
    let wrapped = format!(
        "{} run --surface {}{} -- {}",
        shell_quote_for_host(&ctx_exe()),
        shell_quote_for_host(surface),
        session,
        shell_quote_for_host(trimmed)
    );
    Some(wrapped)
}

/// Resolve the ctx executable path for the rewrite, falling back to bare `ctx` on PATH.
fn ctx_exe() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "ctx".to_string())
}

/// True when a command already runs through the ctx wrapper, so we never double-wrap (and so the
/// postToolUse hook can recognize and skip it).
pub(crate) fn is_ctx_run_wrapped(command: &str) -> bool {
    let c = command.trim_start();
    let mut words = c.split_whitespace();
    let first = words.next();
    let executable = if first == Some("&") {
        words.next()
    } else {
        first
    };
    if let Some(executable) = executable {
        let executable = executable.trim_matches(['\'', '"']);
        let is_ctx_bin = executable == "ctx"
            || std::path::Path::new(executable)
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| matches!(name, "ctx" | "ctx.exe"));
        if is_ctx_bin {
            return words.next() == Some("run");
        }
    }
    false
}

/// Allowlist gate: only wrap read-only, non-interactive inspection commands whose output is safe to
/// capture and worth compacting. Default-deny: anything not recognized is left untouched, so editors,
/// pagers, REPLs, and prompts (vim, less, git commit, npm init, ssh) are never wrapped. This is a
/// safety boundary, not a compression heuristic; the gate decides whether compaction actually fires.
pub(crate) fn is_safe_to_wrap(command: &str) -> bool {
    let normalized = command.to_ascii_lowercase();
    if [";", "&&", "||", "|", ">", "<", "`", "$("]
        .iter()
        .any(|operator| normalized.contains(operator))
        || normalized.contains(" --interactive")
        || normalized.contains("tail -f")
        || normalized.contains("tail --follow")
        || normalized.trim_end().ends_with('&')
    {
        return false;
    }
    let mut tokens = command.split_whitespace();
    let Some(first_token) = tokens.next() else {
        return false;
    };
    if first_token.contains('=') {
        return false;
    }
    let executable = std::path::Path::new(first_token)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(first_token)
        .trim_matches(['\'', '"'])
        .to_ascii_lowercase();
    match executable.as_str() {
        "git" | "git.exe" => {
            let subcommand = tokens.find(|token| !token.starts_with('-'));
            matches!(
                subcommand,
                Some(
                    "status"
                        | "diff"
                        | "log"
                        | "show"
                        | "branch"
                        | "ls-files"
                        | "blame"
                        | "shortlog"
                        | "describe"
                        | "diff-tree"
                )
            )
        }
        "cargo" | "cargo.exe" => matches!(
            tokens.next(),
            Some("test" | "check" | "build" | "clippy" | "fmt" | "metadata" | "tree")
        ),
        // POSIX read-only inspection capabilities.
        "ls" | "grep" | "rg" | "find" | "cat" | "tree" | "head" | "tail" | "wc" | "pwd" | "du"
        | "df" | "stat" | "file" | "ps" => true,
        // PowerShell inspection cmdlets (case-normalized above).
        "get-childitem" | "get-content" | "select-string" | "get-item" | "get-location"
        | "get-process" | "get-command" => true,
        // cmd.exe inspection built-ins and common read-only utilities.
        "dir" | "type" | "findstr" | "where" | "ver" => true,
        _ => false,
    }
}

#[cfg(windows)]
fn shell_quote_for_host(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(not(windows))]
fn shell_quote_for_host(value: &str) -> String {
    shell_single_quote(value)
}

/// POSIX single-quote a command so it survives as one argument to `ctx run` (which re-runs it via
/// `sh -c`). Embedded single quotes are closed, escaped, and reopened: `it's` -> `'it'\''s'`.
pub(crate) fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Pull the Shell command string out of a Cursor hook payload (`tool_input.command`).
fn cursor_shell_command(payload: &Value) -> Option<String> {
    payload
        .get("tool_input")
        .and_then(|v| v.get("command"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
}

/// Build Cursor's `updated_mcp_tool_output` from the trimmed text, mirroring the MCP result
/// envelope Cursor sends in. Verified live against a real Cursor 3.7 postToolUse payload (ADR
/// 0018): `tool_output` is a JSON-stringified `{"content":[{"type":"text","text":...}],
/// "isError":false}`. We parse it so sibling fields (e.g. `isError`) survive, and replace only the
/// text content with the trimmed text, so the model reads the shorter result in the same shape.
/// Lift a Cursor `postToolUse` payload into the canonical tool result. Returns `None` when there
/// is no compressible output (a write/delete-style tool, or an empty result), so the caller stays
/// silent rather than recording an empty decision.
///
/// Cursor's payload shape (verified against the hooks docs, ADR 0018):
/// `conversation_id` is the stable session id, `workspace_roots[0]` is the cwd, `tool_name` is the
/// tool type ("Shell", "Read", "Grep", or an MCP tool), and `tool_output` is the result as a
/// JSON-stringified string.
pub fn extract_cursor_tool_result(
    payload: &Value,
    capture_canonical_mcp: bool,
) -> Option<ToolResult> {
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
    let (raw_output, canonical_mcp) = cursor_tool_result_views(
        &tool_name,
        payload.get("tool_output"),
        capture_canonical_mcp,
    );
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
        canonical_mcp,
        session_id,
        cwd,
        // Cursor narration is not in the hook payload; the read guard's intent signal stays a
        // Claude-only capability for now (ADR 0011). Observe-only does not need it.
        recent_intent_text: None,
    })
}

/// Normalize Cursor's JSON-stringified result once, then share that value between the existing text
/// extractor and optional canonical MCP capture. Large MCP results must not be decoded twice on the
/// hot hook path.
fn cursor_tool_result_views(
    tool_name: &str,
    output: Option<&Value>,
    capture_canonical_mcp: bool,
) -> (String, Option<crate::tool_result::CanonicalMcpResult>) {
    let Some(output) = output else {
        return (String::new(), None);
    };
    let parsed = if let Some(s) = output.as_str() {
        let trimmed = s.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            match serde_json::from_str(trimmed) {
                Ok(value) => Cow::Owned(value),
                Err(_) => return (s.to_string(), None),
            }
        } else {
            return (s.to_string(), None);
        }
    } else {
        Cow::Borrowed(output)
    };

    // Cursor Shell/terminal results put the text under "output".
    let raw_output = if let Some(output) = parsed.get("output").and_then(Value::as_str) {
        output.to_string()
    } else {
        let extract_name = if tool_name.eq_ignore_ascii_case("shell") {
            "Bash"
        } else {
            tool_name
        };
        crate::compress::extract_compressible_text(extract_name, parsed.as_ref())
    };
    let canonical_mcp =
        if capture_canonical_mcp && crate::compress::classify::is_mcp_tool(tool_name) {
            crate::tool_result::parse_mcp_result(parsed.as_ref()).ok()
        } else {
            None
        };
    (raw_output, canonical_mcp)
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
        let tr = extract_cursor_tool_result(&payload, false).expect("extract");
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
        let tr = extract_cursor_tool_result(&payload, false).expect("extract");
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
        assert!(extract_cursor_tool_result(&payload, false).is_none());
    }

    #[test]
    fn none_when_tool_name_missing() {
        let payload = json!({
            "conversation_id": "conv-1",
            "tool_output": "something"
        });
        assert!(extract_cursor_tool_result(&payload, false).is_none());
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
        let tr = extract_cursor_tool_result(&payload, false).expect("extract");
        assert_eq!(tr.tool_name, "Grep");
        assert!(tr.raw_output.contains("fn main"));
    }

    #[test]
    fn cursor_mcp_tool_name_is_detected() {
        // Cursor names MCP tools `MCP:<tool>`; Claude uses `mcp__server__tool`. Both must read as MCP
        // so only MCP results get an apply path. Built-ins must not.
        assert!(crate::compress::classify::is_mcp_tool("MCP:get_issue"));
        assert!(crate::compress::classify::is_mcp_tool(
            "mcp__linear__get_issue"
        ));
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

    fn shell_trial_cfg() -> Config {
        Config {
            compress_enabled: true,
            compress_trial_tools: vec!["Shell".into()],
            ..Default::default()
        }
    }

    #[test]
    fn safe_to_wrap_allows_read_only_inspection() {
        assert!(is_safe_to_wrap("git status"));
        assert!(is_safe_to_wrap("git diff HEAD~5 --stat"));
        assert!(is_safe_to_wrap("git log --oneline -n 200"));
        assert!(is_safe_to_wrap("ls -la"));
        assert!(is_safe_to_wrap("grep -rn foo src/"));
        assert!(is_safe_to_wrap("rg pattern"));
        assert!(is_safe_to_wrap("cargo build"));
        assert!(is_safe_to_wrap("/usr/bin/git status"));
        assert!(is_safe_to_wrap("Get-ChildItem -Recurse"));
        assert!(is_safe_to_wrap("findstr /S needle *.txt"));
    }

    #[test]
    fn safe_to_wrap_blocks_interactive_and_unknown() {
        // Editors, pagers, REPLs, prompts: capturing their output would break them.
        assert!(!is_safe_to_wrap("vim src/main.rs"));
        assert!(!is_safe_to_wrap("less big.log"));
        assert!(!is_safe_to_wrap("git commit -m wip"));
        assert!(!is_safe_to_wrap("git rebase -i HEAD~3"));
        assert!(!is_safe_to_wrap("npm init"));
        assert!(!is_safe_to_wrap("python"));
        // Leading environment mutation remains ambiguous and is held out.
        assert!(!is_safe_to_wrap("FOO=1 git status"));
        assert!(!is_safe_to_wrap("cargo publish"));
        assert!(!is_safe_to_wrap("git status && rm -rf build"));
    }

    #[test]
    fn shell_single_quote_escapes_embedded_quotes() {
        assert_eq!(shell_single_quote("git status"), "'git status'");
        assert_eq!(shell_single_quote("echo it's"), "'echo it'\\''s'");
        // Round-trip through a real POSIX shell: the quoted form is one sh argument equal to the
        // original. sh-only, so the execution check is unix-gated (the escaping asserts above run
        // everywhere).
        #[cfg(unix)]
        {
            let original = "grep -n \"a'b\" src/";
            let quoted = shell_single_quote(original);
            let out = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("printf '%s' {quoted}"))
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), original);
        }
    }

    #[test]
    fn detects_ctx_run_wrapped_commands() {
        assert!(is_ctx_run_wrapped("ctx run 'git status'"));
        assert!(is_ctx_run_wrapped("/Users/me/.cargo/bin/ctx run 'ls'"));
        assert!(!is_ctx_run_wrapped("git status"));
        assert!(!is_ctx_run_wrapped("ctx status"));
        assert!(!is_ctx_run_wrapped("ctxrun foo"));
    }

    #[test]
    fn rewrite_wraps_eligible_safe_command() {
        let cfg = shell_trial_cfg();
        let wrapped = decide_shell_rewrite(&cfg, "git status").expect("should wrap");
        assert!(wrapped.contains(" run "), "must invoke the run wrapper");
        assert!(
            wrapped.contains("'git status'"),
            "original command must be passed as one quoted arg, got: {wrapped}"
        );
    }

    #[test]
    fn rewrite_skips_unsafe_and_wrapped_and_disabled() {
        let cfg = shell_trial_cfg();
        assert!(
            decide_shell_rewrite(&cfg, "vim foo").is_none(),
            "interactive command must never be wrapped"
        );
        assert!(
            decide_shell_rewrite(&cfg, "ctx run 'git status'").is_none(),
            "already-wrapped command must not be double-wrapped"
        );
        let off = Config {
            compress_enabled: false,
            ..shell_trial_cfg()
        };
        assert!(
            decide_shell_rewrite(&off, "git status").is_none(),
            "compression disabled means no rewrite"
        );
    }

    #[test]
    fn pre_tool_use_emits_updated_input_for_eligible_shell() {
        let cfg = shell_trial_cfg();
        let payload = json!({
            "tool_name": "Shell",
            "tool_input": {"command": "git diff HEAD~3 --stat", "working_directory": "/proj"}
        });
        let out = decide_pre_tool_use(&cfg, &payload).expect("should rewrite");
        assert_eq!(out["permission"], json!("allow"));
        let cmd = out["updated_input"]["command"].as_str().unwrap();
        assert!(cmd.contains(" run "));
        assert!(cmd.contains("'git diff HEAD~3 --stat'"));
        // Sibling input fields are preserved.
        assert_eq!(out["updated_input"]["working_directory"], json!("/proj"));
    }

    #[test]
    fn pre_tool_use_ignores_non_shell_and_ineligible() {
        let cfg = shell_trial_cfg();
        let read_payload = json!({"tool_name": "Read", "tool_input": {"path": "a.rs"}});
        assert!(decide_pre_tool_use(&cfg, &read_payload).is_none());

        // A safe command but Shell not earned (no trial, no activation): leave it alone.
        let plain = Config {
            compress_enabled: true,
            ..Default::default()
        };
        let shell_payload = json!({"tool_name": "Shell", "tool_input": {"command": "vim x"}});
        assert!(
            decide_pre_tool_use(&plain, &shell_payload).is_none(),
            "interactive command stays untouched even when Shell could be eligible"
        );
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
