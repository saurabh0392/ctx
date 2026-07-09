//! PostToolUse output compression before Claude reads tool results.

pub mod activation;
pub mod classify;
pub mod tool_activation;
mod context;
mod edit;
pub mod edit_intent;
mod generic;
pub mod path_role;
mod git;
mod grep;
mod hook_io;
pub mod intent;
mod mcp;
mod read;
mod retain;
mod session_dedup;
pub mod shadow;
mod test_runner;
mod types;

pub use context::{build_task_frame_minimal, SgrMode, TaskFrame};
pub use hook_io::{
    extract_compressible_text, extract_tool_output, post_tool_use, rewind_id_for, tool_response_value,
    trim_marker, wrap_updated_tool_output,
};
pub(crate) use hook_io::record_shadow_decision;
pub use shadow::{compute_shadow_decision, ShadowDecision};
pub use types::{CompressKind, CompressResult};

use crate::config::Config;
use classify::classify_tool;
use context::{adaptive_target_chars, build_context, build_task_frame, load_prompt_from_session};
use git::{compress_git_diff, compress_git_log, compress_git_status};
use grep::compress_grep_output;
use mcp::compress_mcp_output;
use read::compress_read_output;
use retain::apply_line_retention;
use test_runner::compress_test_output;
use types::CompressOptions;

/// A command that explicitly narrowed its output to at most this many lines is left whole (CTX-58).
/// Above it, the narrowing is loose enough that trimming still helps.
const EXPLICIT_NARROW_MAX_LINES: usize = 200;

pub fn compress_tool_output(
    tool_name: &str,
    tool_input: &serde_json::Value,
    raw_output: &str,
    cfg: &Config,
    session_id: Option<&str>,
    cwd: &str,
    sgr_arm: bool,
) -> Option<CompressResult> {
    if !cfg.compress_enabled || raw_output.is_empty() {
        return None;
    }

    if !tool_allowed(tool_name, cfg) {
        return None;
    }

    let command = tool_input.get("command").and_then(|v| v.as_str());
    let file_path = tool_input
        .get("file_path")
        .or_else(|| tool_input.get("path"))
        .and_then(|v| v.as_str());

    // Respect explicit narrowing (CTX-58): a command that already capped its own output to a modest
    // number of lines (`| head -50`, `grep -m 30`) asked for exactly that; trimming below it is the
    // doubly-wrong case that drove the workarounds. Leave it whole.
    if let Some(cap) = command.and_then(classify::explicit_output_cap) {
        if cap <= EXPLICIT_NARROW_MAX_LINES {
            return None;
        }
    }

    let kind = classify_tool(tool_name, command, file_path);
    let prompt = load_prompt_from_session(session_id);
    let ctx = build_context(cwd, &prompt);

    let opts = CompressOptions {
        max_input_chars: cfg.compress_max_output_chars,
        target_chars: cfg.compress_target_chars,
        redact_secrets: cfg.compress_redact_secrets,
        preserve_errors: cfg.compress_preserve_errors,
    };

    let chars_in = raw_output.chars().count();
    if chars_in <= opts.target_chars && !(cfg.compress_sgr_enabled && sgr_arm) {
        return None;
    }

    let mut result = match kind {
        CompressKind::GitStatus => compress_git_status(raw_output, &opts, &ctx),
        CompressKind::GitDiff => compress_git_diff(raw_output, &opts, &ctx),
        CompressKind::GitLog => compress_git_log(raw_output, &opts, &ctx),
        CompressKind::TestRunner => compress_test_output(raw_output, &opts, &ctx),
        CompressKind::Grep => compress_grep_output(raw_output, &opts, &ctx),
        CompressKind::Read => {
            compress_read_output(raw_output, file_path.unwrap_or("unknown"), &opts, &ctx)
        }
        CompressKind::Mcp => compress_mcp_output(raw_output, &opts, &ctx),
        CompressKind::Edit => edit::compress_edit_output(raw_output, &opts, &ctx),
        CompressKind::Generic | CompressKind::Passthrough => {
            generic::compress_generic(raw_output, &opts, &ctx, "generic")
        }
    };

    if result.chars_saved() == 0 && chars_in <= opts.target_chars {
        return None;
    }

    // Floor guard (CTX-40): a strategy must never erase non-trivial output to empty. If it did,
    // treat it as a no-op rather than handing the model an empty result. A genuinely empty input
    // returned early above, so chars_in > 0 here means real content would be lost.
    if result.chars_out == 0 && chars_in > 0 {
        return None;
    }

    if cfg.compress_sgr_enabled && sgr_arm {
        result = apply_sgr(
            result, cfg, session_id, cwd, tool_name, tool_input, raw_output,
        );
    }

    if result.chars_saved() == 0 {
        return None;
    }
    Some(result)
}

fn apply_sgr(
    mut result: CompressResult,
    cfg: &Config,
    session_id: Option<&str>,
    cwd: &str,
    tool_name: &str,
    tool_input: &serde_json::Value,
    raw_v1_input: &str,
) -> CompressResult {
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        if cfg.compress_sgr_dedup {
            if let Some(pointer) = session_dedup::duplicate_output_pointer(
                &conn,
                session_id,
                raw_v1_input,
                result.chars_in,
            ) {
                let chars_out = pointer.chars().count();
                return CompressResult {
                    text: pointer,
                    chars_in: result.chars_in,
                    chars_out,
                    strategy: format!("{}+sgr-dedup", result.strategy),
                };
            }
        }
    }

    let profile = cfg.active_profile.as_deref().unwrap_or("all");
    let frame = build_task_frame(
        session_id,
        cwd,
        tool_name,
        tool_input,
        profile,
        cfg.compress_sgr_dedup,
    );
    let target = adaptive_target_chars(
        cfg.compress_target_chars,
        &frame,
        cfg.compress_adaptive_budget,
    );
    let opts = CompressOptions {
        target_chars: target,
        max_input_chars: cfg.compress_max_output_chars,
        redact_secrets: cfg.compress_redact_secrets,
        preserve_errors: cfg.compress_preserve_errors,
    };

    let retained = apply_line_retention(&result.text, &frame, &opts);
    result.text = retained.text;
    result.chars_out = retained.chars_out;
    result.strategy = format!("{}+sgr-{}", result.strategy, frame.mode.as_str());

    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        if cfg.compress_sgr_dedup {
            session_dedup::record_output_fingerprint(&conn, session_id, raw_v1_input);
            session_dedup::record_line_fingerprints(&conn, session_id, &result.text);
        }
    }

    result
}

/// True for a tool on ctx's own MCP server (`mcp__ctx__*`), which carries the recovery tools
/// (ctx_expand, ctx_status, ctx_waste). Trimming that server's output would hide the surface that
/// makes every other trim reversible, so it is never trimmed. Mirrors the un-prunable rule in
/// filter_control.rs (`prune_server` refuses `mcp__ctx__`).
fn is_ctx_server_tool(name: &str) -> bool {
    name.trim()
        .get(..10)
        .is_some_and(|p| p.eq_ignore_ascii_case("mcp__ctx__"))
}

/// Whether a tool's output is a mutation/one-shot the agent consumes once and acts on, rather than
/// a read-back it can re-read. This matters because the earn-it harm signal in `agent::decide` judges
/// output tools by re-read and edit tools by re-edit: a bad trim of a mutation result is never
/// re-read, so the gate can't see the harm and would wrongly pass it. Under the spike we hold these
/// out of trimming entirely.
///
/// Built-in one-shot/state tools (todowrite, task, taskoutput) are named directly. For MCP tools we
/// classify by the method verb: the segment after the last `__`, first recognized token split on `_`
/// or `-`. Vendor-prefixed methods (Notion's `notion-fetch`, `notion-update-page`) put a non-verb
/// segment first, so we scan left to right for the first token we recognize rather than blindly
/// taking token 0. An unrecognized verb defaults to MUTATION (held): without a known read verb we
/// can't assume a re-read would surface a bad trim, so we fail safe toward not trimming. The tradeoff
/// is that a genuinely read-shaped tool with an unusual verb stays ineligible until its verb is added.
fn is_mutation_tool(name: &str) -> bool {
    // Read verbs: output the agent can (and does) re-read, so a bad trim shows up as a re-read.
    const READ_VERBS: &[&str] = &[
        "get", "list", "fetch", "search", "read", "query", "download", "export", "whoami",
        "describe", "view", "resolve",
    ];
    // Mutation verbs: a write/action whose result is consumed once. No re-read, so no harm signal.
    const MUTATION_VERBS: &[&str] = &[
        "save", "create", "update", "delete", "remove", "add", "set", "patch", "upload", "post",
        "send", "merge", "close", "archive", "move", "duplicate", "write", "prepare", "cancel",
        "assign", "comment",
    ];

    let n = name.trim();
    if matches!(
        n.to_ascii_lowercase().as_str(),
        "todowrite" | "task" | "taskoutput"
    ) {
        return true;
    }
    if !classify::is_mcp_tool(n) {
        return false;
    }
    let method = n.rsplit("__").next().unwrap_or(n).to_ascii_lowercase();
    for tok in method.split(|c| c == '_' || c == '-') {
        if READ_VERBS.contains(&tok) {
            return false;
        }
        if MUTATION_VERBS.contains(&tok) {
            return true;
        }
    }
    // Unrecognized verb: hold it.
    true
}

/// A tool that must never be trimmed: ctx's own recovery server (always, both modes), any name the
/// user listed in `compress_deny_tools` (always), or, only under the `compress_trim_all` spike, a
/// mutation/one-shot tool the harm signal can't watch. The mutation deny is gated on `trim_all` so
/// allow-list mode keeps exactly today's behavior.
pub fn is_trim_denied(name: &str, cfg: &Config) -> bool {
    let n = name.trim();
    if is_ctx_server_tool(n) {
        return true;
    }
    if cfg
        .compress_deny_tools
        .iter()
        .any(|d| d.trim().eq_ignore_ascii_case(n))
    {
        return true;
    }
    cfg.compress_trim_all && is_mutation_tool(n)
}

/// Why a tool is held out of trimming, for the dashboard. `None` means the tool is trim-eligible
/// (burn-in still decides when it actually trims). Mirrors `is_trim_denied`: ctx's own recovery
/// server reads as a recovery tool, everything else that is held reads as a mutation the harm gate
/// can't watch (a user-listed deny falls here too, which is close enough for the surface).
pub fn held_reason(name: &str, cfg: &Config) -> Option<String> {
    if !is_trim_denied(name, cfg) {
        return None;
    }
    if is_ctx_server_tool(name.trim()) {
        Some("Recovery tool, never trimmed.".to_string())
    } else {
        Some(
            "Measured only. The agent acts on it once and never re-reads it, so a bad trim would be invisible to the gate."
                .to_string(),
        )
    }
}

fn tool_allowed(tool_name: &str, cfg: &Config) -> bool {
    let name = tool_name.trim();
    // Deny-set first, in both modes. This also closes a latent gap: before the deny-set, the MCP
    // branch below returned true unconditionally, so ctx's own recovery output (ctx_expand) was
    // itself trim-eligible. Denying it here fixes that regardless of the trim_all flag.
    if is_trim_denied(name, cfg) {
        return false;
    }
    // SPIKE: earn-it governs every tool. Any non-denied tool is eligible; the preset / burn-in /
    // activation gate in agent::decide still decides whether an eligible tool actually trims, so
    // newly-eligible tools only trim after a clean baseline and back off on harm.
    if cfg.compress_trim_all {
        return true;
    }
    if classify::is_mcp_tool(name) {
        return true;
    }
    // Cursor's "Shell" is Claude's "Bash": the same surface under two names. Treat them as
    // interchangeable so a config that allows one allows the other (matches the classify unification
    // in CTX-41), otherwise the Cursor Shell `ctx run` path can never compress.
    let aliases: &[&str] = if name.eq_ignore_ascii_case("shell") || name.eq_ignore_ascii_case("bash")
    {
        &["shell", "bash"]
    } else {
        std::slice::from_ref(&name)
    };
    cfg.compress_tools
        .iter()
        .any(|t| aliases.iter().any(|a| t.eq_ignore_ascii_case(a)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> Config {
        Config {
            compress_enabled: true,
            compress_max_output_chars: 12_000,
            compress_target_chars: 800,
            compress_tools: vec!["Bash".into(), "Read".into(), "Grep".into(), "Glob".into()],
            compress_redact_secrets: true,
            compress_preserve_errors: true,
            ..Default::default()
        }
    }

    fn test_cfg_sgr() -> Config {
        Config {
            compress_sgr_enabled: true,
            compress_adaptive_budget: true,
            ..test_cfg()
        }
    }

    #[test]
    fn shell_is_allowed_like_bash() {
        // Cursor names the shell tool "Shell"; the default config lists "Bash". The `ctx run` path
        // must still be allowed to compress, or Cursor Shell output never compacts (CTX-41).
        let cfg = test_cfg();
        assert!(tool_allowed("Shell", &cfg));
        assert!(tool_allowed("shell", &cfg));
        assert!(tool_allowed("Bash", &cfg));
        assert!(!tool_allowed("Write", &cfg));
    }

    #[test]
    fn trim_all_makes_read_shaped_tools_eligible() {
        // SPIKE: with the flag on, the earn-it gate governs everything. tool_allowed becomes a
        // deny-set check: read/edit tools the allow-list stranded (Write, Edit, MultiEdit) are now
        // eligible, while ctx's recovery server, configured deny tools, and mutation/one-shot tools
        // (TodoWrite, Task, TaskOutput) are held. Eligibility is not application: agent::decide's
        // preset/burn-in gate still decides whether an eligible tool actually trims.
        let cfg = Config {
            compress_trim_all: true,
            ..test_cfg()
        };
        for t in ["Write", "Edit", "MultiEdit", "Bash", "Read"] {
            assert!(tool_allowed(t, &cfg), "{t} should be eligible under trim_all");
        }
        assert!(!tool_allowed("mcp__ctx__ctx_expand", &cfg));
        assert!(!tool_allowed("mcp__ctx__ctx_status", &cfg), "whole ctx server is denied by prefix");
        // Mutation / one-shot tools are held even without an explicit deny entry.
        for t in ["TodoWrite", "Task", "TaskOutput"] {
            assert!(!tool_allowed(t, &cfg), "{t} is a mutation/one-shot: held under trim_all");
        }
    }

    #[test]
    fn is_mutation_tool_classifies_by_consumption() {
        // Built-in one-shot/state tools.
        for t in ["TodoWrite", "todowrite", "Task", "TaskOutput"] {
            assert!(is_mutation_tool(t), "{t} is a built-in mutation/one-shot");
        }
        // MCP writes across servers, classified by method verb.
        for t in [
            "mcp__claude_ai_Linear__save_issue",
            "mcp__claude_ai_Notion__notion-create-pages",
            "mcp__claude_ai_Notion__notion-update-page",
            "mcp__claude_ai_Linear__delete_attachment",
        ] {
            assert!(is_mutation_tool(t), "{t} is an MCP mutation");
        }
        // MCP reads: trimmable.
        for t in [
            "mcp__claude_ai_Linear__list_issues",
            "mcp__claude_ai_Linear__get_issue",
            "mcp__claude_ai_Notion__notion-fetch",
            "mcp__claude_ai_Notion__notion-search",
        ] {
            assert!(!is_mutation_tool(t), "{t} is an MCP read");
        }
        // Non-MCP, non-built-in: not a mutation (falls through to the allow-list logic).
        assert!(!is_mutation_tool("Read"));
        assert!(!is_mutation_tool("Bash"));
    }

    #[test]
    fn trim_all_gate_splits_mcp_reads_from_writes() {
        let cfg = Config { compress_trim_all: true, ..test_cfg() };
        // Read-shaped MCP and built-in tools are eligible.
        assert!(tool_allowed("mcp__claude_ai_Notion__notion-fetch", &cfg));
        assert!(tool_allowed("mcp__claude_ai_Linear__list_issues", &cfg));
        assert!(tool_allowed("Edit", &cfg));
        assert!(tool_allowed("Write", &cfg));
        // Mutation MCP writes, ctx server, and built-in one-shots are held.
        assert!(!tool_allowed("mcp__claude_ai_Linear__save_issue", &cfg));
        assert!(!tool_allowed("mcp__ctx__ctx_expand", &cfg));
        assert!(!tool_allowed("TodoWrite", &cfg));
        assert!(!tool_allowed("Task", &cfg));
    }

    #[test]
    fn held_reason_matches_eligibility() {
        let cfg = Config { compress_trim_all: true, ..test_cfg() };
        // ctx recovery server: held, recovery reason.
        assert_eq!(
            held_reason("mcp__ctx__ctx_expand", &cfg).as_deref(),
            Some("Recovery tool, never trimmed.")
        );
        // Mutation / one-shot: held, measured-only reason.
        for t in ["mcp__claude_ai_Linear__save_issue", "TaskOutput"] {
            let r = held_reason(t, &cfg);
            assert!(r.is_some(), "{t} should be held");
            assert!(r.unwrap().starts_with("Measured only."), "{t} gets the measured-only reason");
        }
        // Read-shaped tools are eligible: no reason.
        for t in [
            "mcp__claude_ai_Notion__notion-fetch",
            "mcp__claude_ai_Linear__get_issue",
        ] {
            assert!(held_reason(t, &cfg).is_none(), "{t} is trim-eligible");
            assert!(!is_trim_denied(t, &cfg), "{t} is not denied");
        }
    }

    #[test]
    fn deny_set_applies_in_both_modes_for_ctx_server() {
        // The ctx server is never trimmed regardless of the flag. Off-mode used to return true for
        // any MCP tool unconditionally; the deny-set now holds ctx's recovery tools back in both
        // modes, closing that latent gap.
        let off = test_cfg(); // compress_trim_all defaults false
        assert!(!tool_allowed("mcp__ctx__ctx_expand", &off));
        assert!(tool_allowed("mcp__claude_ai_Linear__get_issue", &off), "other MCP still allowed off");
        let on = Config { compress_trim_all: true, ..test_cfg() };
        assert!(!tool_allowed("mcp__ctx__ctx_expand", &on));
    }

    #[test]
    fn trim_all_off_preserves_allow_list() {
        // Flag off: exactly today's behavior. Allow-list members trim, others do not, MCP is allowed.
        let cfg = test_cfg();
        assert!(!cfg.compress_trim_all);
        assert!(tool_allowed("Bash", &cfg));
        assert!(tool_allowed("Read", &cfg));
        assert!(!tool_allowed("Write", &cfg));
        assert!(!tool_allowed("Edit", &cfg));
        assert!(!tool_allowed("TaskOutput", &cfg));
        assert!(tool_allowed("mcp__claude_ai_Notion__notion-fetch", &cfg));
    }

    #[test]
    fn compresses_large_shell_git_log() {
        // The exact Cursor path: tool_name "Shell", a big git log. Must classify as git-log and
        // compress, not fall through to no-op.
        let mut body = String::new();
        for i in 0..400 {
            body.push_str(&format!(
                "commit {i:040x}\nAuthor: A <a@b.c>\nDate: today\n\n    message {i} with enough text to grow\n\n"
            ));
        }
        let cfg = test_cfg();
        let r = compress_tool_output(
            "Shell",
            &serde_json::json!({"command": "git log -n 400"}),
            &body,
            &cfg,
            None,
            "/tmp",
            false,
        )
        .expect("Shell git log must compress");
        assert!(r.chars_saved() > 1000, "should save a lot, saved {}", r.chars_saved());
        assert_eq!(r.strategy, "git-log");
    }

    #[test]
    fn compresses_large_git_status() {
        let mut body = String::from("On branch main\n\nUntracked files:\n");
        for i in 0..200 {
            body.push_str(&format!("  file_{i}.rs\n"));
        }
        let cfg = test_cfg();
        let r = compress_tool_output(
            "Bash",
            &serde_json::json!({"command": "git status"}),
            &body,
            &cfg,
            None,
            "/tmp",
            false,
        )
        .unwrap();
        assert!(r.chars_saved() > 100);
        assert_eq!(r.strategy, "git-status");
    }

    #[test]
    fn sgr_keeps_focus_path_in_read_output() {
        let mut raw = String::new();
        for i in 0..120 {
            raw.push_str(&format!("// noise module lib/other_{i}.rs\n"));
        }
        raw.push_str("pub fn compress_tool_output() {}\n");
        raw.push_str("// error in src/foo.rs: expected type\n");
        let cfg = test_cfg_sgr();
        let r = compress_tool_output(
            "Read",
            &serde_json::json!({"file_path": "src/foo.rs"}),
            &raw,
            &cfg,
            Some("fix foo.rs type error"),
            "/tmp/project",
            true,
        )
        .expect("sgr should compress");
        assert!(r.strategy.contains("sgr"));
        assert!(r.text.contains("foo.rs") || r.text.contains("compress_tool_output"));
    }

    #[test]
    fn passthrough_small_output() {
        let cfg = test_cfg();
        assert!(compress_tool_output(
            "Bash",
            &serde_json::json!({"command": "echo hi"}),
            "hi\n",
            &cfg,
            None,
            "/tmp",
            false,
        )
        .is_none());
    }
}
