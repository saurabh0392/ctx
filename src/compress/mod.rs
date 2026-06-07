//! PostToolUse output compression before Claude reads tool results.

pub mod activation;
pub mod classify;
mod context;
pub mod edit_intent;
mod generic;
mod git;
mod grep;
mod hook_io;
mod mcp;
mod read;
mod retain;
mod session_dedup;
pub mod shadow;
mod test_runner;
mod types;

pub use context::{build_task_frame_minimal, SgrMode, TaskFrame};
pub use hook_io::{
    extract_compressible_text, extract_tool_output, post_tool_use, tool_response_value,
    wrap_updated_tool_output,
};
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

    let command = tool_input
        .get("command")
        .and_then(|v| v.as_str());
    let file_path = tool_input
        .get("file_path")
        .or_else(|| tool_input.get("path"))
        .and_then(|v| v.as_str());

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
        CompressKind::Read => compress_read_output(
            raw_output,
            file_path.unwrap_or("unknown"),
            &opts,
            &ctx,
        ),
        CompressKind::Mcp => compress_mcp_output(raw_output, &opts, &ctx),
        CompressKind::Generic | CompressKind::Passthrough => {
            generic::compress_generic(raw_output, &opts, &ctx, "generic")
        }
    };

    if result.chars_saved() == 0 && chars_in <= opts.target_chars {
        return None;
    }

    if cfg.compress_sgr_enabled && sgr_arm {
        result = apply_sgr(result, cfg, session_id, cwd, tool_name, tool_input, raw_output);
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
            if let Some(pointer) =
                session_dedup::duplicate_output_pointer(&conn, session_id, raw_v1_input, result.chars_in)
            {
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
    let target = adaptive_target_chars(cfg.compress_target_chars, &frame, cfg.compress_adaptive_budget);
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

fn tool_allowed(tool_name: &str, cfg: &Config) -> bool {
    let name = tool_name.trim();
    if name.starts_with("mcp__") {
        return true;
    }
    cfg.compress_tools
        .iter()
        .any(|t| t.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> Config {
        Config {
            compress_enabled: true,
            compress_max_output_chars: 12_000,
            compress_target_chars: 800,
            compress_tools: vec![
                "Bash".into(),
                "Read".into(),
                "Grep".into(),
                "Glob".into(),
            ],
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
