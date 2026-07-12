//! `ctx run` — run a shell command and compact its output before the agent reads it (CTX-41).
//!
//! This is the runtime engine for acting on Cursor's built-in Shell output. Cursor will not let a
//! `postToolUse` hook rewrite Shell output (ADR 0018/0021), but a `preToolUse` hook can rewrite the
//! Shell *command* before it runs (the RTK approach, confirmed in the CTX-39 spike). The hook
//! rewrites `<cmd>` to `ctx run <cmd>`, so the compacted result comes back as Shell's own output.
//!
//! Three guarantees shape this code:
//! - **Bulletproof passthrough.** Unless a clean, gated compaction happens, the command's real
//!   stdout, stderr, and exit code reach the agent unchanged. ctx must never break a command or
//!   silently alter a result it chose not to compact.
//! - **Same gate as everywhere else.** Output is compacted only when the surface-agnostic
//!   controller (`agent::decide`) says apply, so Shell earns trimming like every other tool. The
//!   gate runs here, after execution, where the real output finally exists.
//! - **Honest accounting.** A real compaction records a compress_event + analytics under
//!   `surface = "cursor"`, so dashboard savings stay truthful and match what the agent actually saw.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::Result;

use crate::agent::ToolResult;
use crate::config::Config;
use crate::cursor_hook::CURSOR_SURFACE;

/// What `ctx run` decided to do with a command's output, kept separate from any I/O or process exit
/// so the core is unit-testable.
struct RunOutcome {
    code: i32,
    raw_stdout: Vec<u8>,
    raw_stderr: Vec<u8>,
    /// `Some` when ctx compacted the combined output; the agent should read this instead of the raw
    /// streams. `None` means pass the raw streams through untouched.
    compacted: Option<String>,
}

/// Entry point for `ctx run <command...>`. Runs the command through a shell, optionally compacts the
/// output, writes the result, and exits with the command's own exit code. Does not return on the
/// normal path: it calls [`std::process::exit`] so the exit status is faithful.
pub fn exec(command_parts: Vec<String>) -> Result<()> {
    let command = command_parts.join(" ");
    if command.trim().is_empty() {
        std::process::exit(0);
    }

    let outcome = run_command(&command);
    write_outcome(&outcome);
    std::process::exit(outcome.code);
}

/// Run `command` through the user's shell and decide whether to compact its output. On any spawn
/// failure this returns a 127 outcome carrying the error on stderr, matching shell convention for a
/// command that could not be executed.
fn run_command(command: &str) -> RunOutcome {
    // Windows has no /bin/sh; earned commands run through cmd.exe there.
    #[cfg(windows)]
    let mut shell_cmd = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command);
        c
    };
    #[cfg(not(windows))]
    let mut shell_cmd = {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut c = Command::new(shell);
        c.arg("-c").arg(command);
        c
    };
    let output = shell_cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            return RunOutcome {
                code: 127,
                raw_stdout: Vec::new(),
                raw_stderr: format!("ctx run: failed to start command: {e}\n").into_bytes(),
                compacted: None,
            };
        }
    };

    let code = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let compacted = try_compact(command, &stdout, &stderr);

    RunOutcome {
        code,
        raw_stdout: output.stdout,
        raw_stderr: output.stderr,
        compacted,
    }
}

/// Emit the outcome. When ctx compacted, the agent reads the compacted text on stdout. Otherwise we
/// replicate the real streams byte-for-byte so nothing downstream sees a difference.
fn write_outcome(outcome: &RunOutcome) {
    if let Some(text) = &outcome.compacted {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(text.as_bytes());
        if !text.ends_with('\n') {
            let _ = out.write_all(b"\n");
        }
        let _ = out.flush();
        return;
    }

    let mut out = std::io::stdout().lock();
    let _ = out.write_all(&outcome.raw_stdout);
    let _ = out.flush();
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(&outcome.raw_stderr);
    let _ = err.flush();
}

/// Build the canonical tool result for this Shell command, run the shared gate, and compact only
/// when the gate says apply and the compressor actually shortens the result. Returns the text to
/// show, or `None` to pass the command through untouched.
fn try_compact(command: &str, stdout: &str, stderr: &str) -> Option<String> {
    let cfg = Config::load();
    if !cfg.compress_enabled {
        return None;
    }

    // Cursor folds stdout and stderr into one "output" blob, so compact the combined view the agent
    // would otherwise read.
    let combined = combined_output(stdout, stderr);
    if combined.trim().is_empty() {
        return None;
    }

    let tr = ToolResult {
        tool_name: "Shell".to_string(),
        tool_input: serde_json::json!({ "command": command }),
        raw_output: combined,
        // The preToolUse hook passes Cursor's conversation id through this env var when it rewrites
        // the command; absent that, the decision still runs with less session context.
        session_id: std::env::var("CTX_CURSOR_CONVERSATION_ID")
            .ok()
            .filter(|s| !s.is_empty()),
        cwd: std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        recent_intent_text: None,
    };

    let decision = crate::agent::decide(&cfg, &tr);
    if !decision.apply {
        return None;
    }

    let result = crate::compress::compress_tool_output(
        &tr.tool_name,
        &tr.tool_input,
        &tr.raw_output,
        &cfg,
        tr.session_id.as_deref(),
        &tr.cwd,
        false,
    )?;
    if result.chars_saved() == 0 {
        return None;
    }

    let command_or_path = crate::surface::fingerprint_tool_input(&tr.tool_name, &tr.tool_input);
    record_apply(
        tr.session_id.as_deref(),
        &tr.tool_name,
        &command_or_path,
        result.strategy.as_str(),
        result.chars_in,
        result.chars_out,
        &cfg,
        &tr.cwd,
    );

    Some(result.text)
}

/// Join the command's stdout and stderr the way Cursor presents a Shell result (a single output
/// blob). Streams are kept in stdout-then-stderr order; interleaving is not preserved, which is fine
/// for the inspection commands this path targets.
fn combined_output(stdout: &str, stderr: &str) -> String {
    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (false, false) => format!("{stdout}\n{stderr}"),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (true, true) => String::new(),
    }
}

/// Record a live Cursor Shell trim as a real apply: a compress_event for the savings feed plus the
/// analytics counter, stamped `surface = "cursor"`. Mirrors the MCP apply path in `cursor_hook` so
/// the cross-surface dashboard reflects Shell savings honestly (CTX-41).
#[allow(clippy::too_many_arguments)]
fn record_apply(
    session_id: Option<&str>,
    tool_name: &str,
    command_or_path: &str,
    strategy: &str,
    chars_in: usize,
    chars_out: usize,
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
            strategy,
            chars_in,
            chars_out,
            command_or_path,
        );
    }
    crate::analytics::record_compress(
        chars_in.saturating_sub(chars_out),
        cfg.active_profile.as_deref().unwrap_or("all"),
        cwd,
    );
    let _ = CURSOR_SURFACE;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_exit_code() {
        let outcome = run_command("exit 7");
        assert_eq!(outcome.code, 7);
    }

    #[test]
    fn preserves_zero_exit_and_stdout() {
        let outcome = run_command("printf 'hello world'");
        assert_eq!(outcome.code, 0);
        assert_eq!(String::from_utf8_lossy(&outcome.raw_stdout), "hello world");
    }

    #[test]
    fn captures_stderr_separately() {
        let outcome = run_command("printf 'oops' 1>&2");
        assert_eq!(outcome.code, 0);
        assert!(outcome.raw_stdout.is_empty());
        assert_eq!(String::from_utf8_lossy(&outcome.raw_stderr), "oops");
    }

    #[test]
    fn small_output_passes_through_untouched() {
        // Tiny output is below any compaction budget, so it must pass through (compacted == None),
        // preserving the exact bytes.
        let outcome = run_command("echo small");
        assert!(
            outcome.compacted.is_none(),
            "small output must not be compacted"
        );
        assert_eq!(String::from_utf8_lossy(&outcome.raw_stdout), "small\n");
    }

    #[test]
    fn nonzero_exit_still_passes_through() {
        let outcome = run_command("printf 'partial output'; exit 2");
        assert_eq!(outcome.code, 2);
        assert_eq!(String::from_utf8_lossy(&outcome.raw_stdout), "partial output");
    }

    #[test]
    fn combined_output_orders_stdout_then_stderr() {
        assert_eq!(combined_output("out", "err"), "out\nerr");
        assert_eq!(combined_output("out", ""), "out");
        assert_eq!(combined_output("", "err"), "err");
        assert_eq!(combined_output("", ""), "");
    }
}
