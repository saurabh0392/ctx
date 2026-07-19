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
use clap::ValueEnum;

use crate::agent::ToolResult;
use crate::config::Config;

/// What `ctx run` decided to do with a command's output, kept separate from any I/O or process exit
/// so the core is unit-testable.
struct RunOutcome {
    code: i32,
    raw_stdout: Vec<u8>,
    raw_stderr: Vec<u8>,
    /// `Some` when ctx prepared a recoverable stdout compaction. Stderr always remains separate.
    compacted: Option<PreparedShellTrim>,
}

#[derive(Debug, Clone)]
struct PreparedShellTrim {
    text: String,
    rewind_id: String,
    session_id: Option<String>,
    tool_name: String,
    command_or_path: String,
    strategy: String,
    chars_in: usize,
    chars_out: usize,
    lines_in: usize,
    lines_out: usize,
    surface: String,
    cwd: String,
    prepared_at: String,
}

/// Explicit cross-platform execution contract for the wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ShellKind {
    Auto,
    Posix,
    PowerShell,
    Cmd,
    Wsl,
}

/// Entry point for `ctx run <command...>`. Runs the command through a shell, optionally compacts the
/// output, writes the result, and exits with the command's own exit code. Does not return on the
/// normal path: it calls [`std::process::exit`] so the exit status is faithful.
pub fn exec(
    command_parts: Vec<String>,
    surface: &str,
    session_id: Option<&str>,
    shell: ShellKind,
) -> Result<()> {
    if crate::surface::SurfaceId::parse(surface).is_none() {
        anyhow::bail!("unknown agent surface `{surface}`; expected claude-code, cursor, or codex");
    }
    let command = command_parts.join(" ");
    if command.trim().is_empty() {
        std::process::exit(0);
    }

    // Interactive, streaming, and background commands must retain the real terminal contract.
    // They bypass capture entirely, so CTX cannot accidentally remove TTY behavior just by being
    // present in a rewritten command.
    if !shell_output_semantically_safe(&command) {
        let mut process = shell_command(shell, &command);
        let status = process
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();
        let code = status.as_ref().map(exit_code).unwrap_or(127);
        if let Err(error) = status {
            eprintln!("ctx run: failed to start passthrough command: {error}");
        }
        std::process::exit(code);
    }

    let outcome = run_command(&command, surface, session_id, shell);
    write_outcome(&outcome)?;
    if let Some(prepared) = &outcome.compacted {
        if let Err(error) = mark_shell_trim_emitted(prepared) {
            eprintln!("ctx run: output was emitted but applied receipt failed: {error}");
        }
    }
    std::process::exit(outcome.code);
}

/// Run `command` through the user's shell and decide whether to compact its output. On any spawn
/// failure this returns a 127 outcome carrying the error on stderr, matching shell convention for a
/// command that could not be executed.
fn run_command(
    command: &str,
    surface: &str,
    session_id: Option<&str>,
    shell: ShellKind,
) -> RunOutcome {
    let mut shell_cmd = shell_command(shell, command);
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

    let code = exit_code(&output.status);
    let compacted = if output.status.success() {
        std::str::from_utf8(&output.stdout)
            .ok()
            .and_then(|stdout| try_compact(command, stdout, surface, session_id))
    } else {
        None
    };

    RunOutcome {
        code,
        raw_stdout: output.stdout,
        raw_stderr: output.stderr,
        compacted,
    }
}

/// Emit the outcome. When ctx compacted, the agent reads the compacted text on stdout. Otherwise we
/// replicate the real streams byte-for-byte so nothing downstream sees a difference.
fn write_outcome(outcome: &RunOutcome) -> Result<()> {
    if let Some(prepared) = &outcome.compacted {
        let mut out = std::io::stdout().lock();
        out.write_all(prepared.text.as_bytes())?;
        if !prepared.text.ends_with('\n') {
            out.write_all(b"\n")?;
        }
        out.flush()?;
        let mut err = std::io::stderr().lock();
        err.write_all(&outcome.raw_stderr)?;
        err.flush()?;
        return Ok(());
    }

    let mut out = std::io::stdout().lock();
    out.write_all(&outcome.raw_stdout)?;
    out.flush()?;
    let mut err = std::io::stderr().lock();
    err.write_all(&outcome.raw_stderr)?;
    err.flush()?;
    Ok(())
}

/// Build the canonical tool result for this Shell command, run the shared gate, and compact only
/// when the gate says apply and the compressor actually shortens the result. Returns the text to
/// show, or `None` to pass the command through untouched.
fn try_compact(
    command: &str,
    stdout: &str,
    surface: &str,
    session_id: Option<&str>,
) -> Option<PreparedShellTrim> {
    let cfg = Config::load();
    if !cfg.compress_enabled {
        return None;
    }

    // Preserve undecodable bytes and terminal control streams exactly. Stderr never enters this
    // function, so diagnostics remain on the original channel even when stdout is shortened.
    if stdout.trim().is_empty() || stdout.contains('\x1b') {
        return None;
    }

    let tr = ToolResult {
        tool_name: "Shell".to_string(),
        tool_input: serde_json::json!({ "command": command }),
        raw_output: stdout.to_owned(),
        canonical_mcp: None,
        // The preToolUse hook passes Cursor's conversation id through this env var when it rewrites
        // the command; absent that, the decision still runs with less session context.
        session_id: session_id
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                std::env::var("CTX_CURSOR_CONVERSATION_ID")
                    .ok()
                    .filter(|s| !s.is_empty())
            }),
        cwd: std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        recent_intent_text: None,
    };

    let decision = crate::agent::decide_for_surface(&cfg, &tr, surface);
    let command_or_path = crate::surface::fingerprint_tool_input(&tr.tool_name, &tr.tool_input);
    if !decision.apply {
        crate::compress::record_shadow_decision(
            tr.session_id.as_deref(),
            &tr.tool_name,
            &command_or_path,
            decision.shadow.as_ref(),
            false,
            decision.explore_arm,
            Some(surface),
        );
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
    );
    let Some(result) = result else {
        crate::compress::record_shadow_decision(
            tr.session_id.as_deref(),
            &tr.tool_name,
            &command_or_path,
            decision.shadow.as_ref(),
            false,
            decision.explore_arm,
            Some(surface),
        );
        return None;
    };
    if result.chars_saved() == 0 {
        crate::compress::record_shadow_decision(
            tr.session_id.as_deref(),
            &tr.tool_name,
            &command_or_path,
            decision.shadow.as_ref(),
            false,
            decision.explore_arm,
            Some(surface),
        );
        return None;
    }

    let rewind_id = sha256_rewind_id(command, stdout.as_bytes());
    let prepared_at = chrono::Utc::now().to_rfc3339();
    let marked = format!(
        "{}{}",
        result.text,
        crate::compress::trim_marker(&rewind_id)
    );
    if marked.chars().count() >= stdout.chars().count() {
        return None;
    }
    let lines_out = marked.lines().count();
    let mut conn = crate::db::open_db().ok()?;
    crate::db::ensure_schema(&conn).ok()?;
    let transaction = conn.transaction().ok()?;
    crate::db::insert_rewind_checked(
        &transaction,
        &rewind_id,
        &prepared_at,
        tr.session_id.as_deref(),
        &tr.tool_name,
        &command_or_path,
        stdout,
        &marked,
    )
    .ok()?;
    transaction.commit().ok()?;

    Some(PreparedShellTrim {
        chars_out: marked.chars().count(),
        text: marked,
        rewind_id,
        session_id: tr.session_id,
        tool_name: tr.tool_name,
        command_or_path,
        strategy: result.strategy,
        chars_in: stdout.chars().count(),
        lines_in: stdout.lines().count(),
        lines_out,
        surface: surface.to_owned(),
        cwd: tr.cwd,
        prepared_at,
    })
}

/// Record a live Cursor Shell trim as a real apply: a compress_event for the savings feed plus the
/// analytics counter, stamped `surface = "cursor"`. Mirrors the MCP apply path in `cursor_hook` so
/// the cross-surface dashboard reflects Shell savings honestly (CTX-41).
fn mark_shell_trim_emitted(prepared: &PreparedShellTrim) -> Result<()> {
    let mut conn = crate::db::open_db()?;
    crate::db::ensure_schema(&conn)?;
    let transaction = conn.transaction()?;
    let features_json = serde_json::json!({
        "adapter": "shell-wrapper-v2",
        "rewind": true,
    })
    .to_string();
    crate::db::insert_compress_decision(
        &transaction,
        &crate::db::CompressDecision {
            ts: &prepared.prepared_at,
            session_id: prepared.session_id.as_deref(),
            tool_name: &prepared.tool_name,
            server_prefix: None,
            kind: "shell",
            task_mode: "wrapper",
            lines_total: prepared.lines_in,
            lines_keep: prepared.lines_out,
            lines_drop: prepared.lines_in.saturating_sub(prepared.lines_out),
            chars_in: prepared.chars_in,
            would_chars_out: prepared.chars_out,
            features_json: &features_json,
            command_or_path: &prepared.command_or_path,
            applied: true,
            explore_arm: None,
            surface: Some(&prepared.surface),
        },
    )?;
    let decision_id = transaction.last_insert_rowid();
    transaction.execute(
        "UPDATE compress_decisions SET rewind_id=?2 WHERE id=?1",
        rusqlite::params![decision_id, prepared.rewind_id],
    )?;
    crate::db::insert_compress_event(
        &transaction,
        &prepared.prepared_at,
        prepared.session_id.as_deref(),
        &prepared.tool_name,
        &prepared.strategy,
        prepared.chars_in,
        prepared.chars_out,
        &prepared.command_or_path,
    )?;
    transaction.commit()?;
    crate::analytics::record_compress(
        prepared.chars_in.saturating_sub(prepared.chars_out),
        Config::load().active_profile.as_deref().unwrap_or("all"),
        &prepared.cwd,
    );
    Ok(())
}

fn shell_command(kind: ShellKind, command: &str) -> Command {
    let resolved = match kind {
        ShellKind::Auto => {
            if cfg!(windows) {
                ShellKind::PowerShell
            } else {
                ShellKind::Posix
            }
        }
        explicit => explicit,
    };
    match resolved {
        ShellKind::Posix => {
            let mut process =
                Command::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()));
            process.arg("-c").arg(command);
            process
        }
        ShellKind::PowerShell => {
            let mut process = Command::new(if cfg!(windows) {
                "powershell.exe"
            } else {
                "pwsh"
            });
            process
                .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
                .arg(command);
            process
        }
        ShellKind::Cmd => {
            let mut process = Command::new("cmd.exe");
            process.args(["/D", "/S", "/C"]).arg(command);
            process
        }
        ShellKind::Wsl => {
            let mut process = Command::new("wsl.exe");
            process.args(["--", "sh", "-lc"]).arg(command);
            process
        }
        ShellKind::Auto => unreachable!(),
    }
}

fn exit_code(status: &std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|signal| 128 + signal).unwrap_or(1)
    }
    #[cfg(not(unix))]
    1
}

fn shell_output_semantically_safe(command: &str) -> bool {
    let normalized = command.to_ascii_lowercase();
    !normalized.contains('\0')
        && !normalized.contains(" --interactive")
        && !normalized.contains(" -i ")
        && !normalized.contains("tail -f")
        && !normalized.contains("tail --follow")
        && !normalized.contains("watch ")
        && !normalized.contains("less ")
        && !normalized.contains("more ")
        && !normalized.contains("top ")
        && !normalized.contains("htop ")
        && !normalized.trim_end().ends_with('&')
}

fn sha256_rewind_id(command: &str, stdout: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"ctx-shell-rewind-v1\0");
    hash.update(command.as_bytes());
    hash.update([0]);
    hash.update(stdout);
    format!("shell-{:x}", hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_exit_code() {
        let outcome = run_command("exit 7", "cursor", None, ShellKind::Auto);
        assert_eq!(outcome.code, 7);
    }

    #[test]
    fn preserves_zero_exit_and_stdout() {
        #[cfg(not(windows))]
        let command = "printf 'hello world'";
        #[cfg(windows)]
        let command = "[Console]::Out.Write('hello world')";
        let outcome = run_command(command, "cursor", None, ShellKind::Auto);
        assert_eq!(outcome.code, 0);
        assert_eq!(String::from_utf8_lossy(&outcome.raw_stdout), "hello world");
    }

    #[test]
    fn captures_stderr_separately() {
        #[cfg(not(windows))]
        let command = "printf 'oops' 1>&2";
        #[cfg(windows)]
        let command = "[Console]::Error.Write('oops')";
        let outcome = run_command(command, "cursor", None, ShellKind::Auto);
        assert_eq!(outcome.code, 0);
        assert!(outcome.raw_stdout.is_empty());
        assert_eq!(String::from_utf8_lossy(&outcome.raw_stderr), "oops");
    }

    #[test]
    fn small_output_passes_through_untouched() {
        // Tiny output is below any compaction budget, so it must pass through (compacted == None),
        // preserving the exact bytes.
        #[cfg(not(windows))]
        let command = "echo small";
        #[cfg(windows)]
        let command = "[Console]::Out.WriteLine('small')";
        let outcome = run_command(command, "cursor", None, ShellKind::Auto);
        assert!(
            outcome.compacted.is_none(),
            "small output must not be compacted"
        );
        // The shell's exact bytes pass through untouched, including its native line ending.
        #[cfg(not(windows))]
        assert_eq!(String::from_utf8_lossy(&outcome.raw_stdout), "small\n");
        #[cfg(windows)]
        assert_eq!(String::from_utf8_lossy(&outcome.raw_stdout), "small\r\n");
    }

    #[cfg(not(windows))]
    #[test]
    fn nonzero_exit_still_passes_through() {
        let outcome = run_command(
            "printf 'partial output'; exit 2",
            "cursor",
            None,
            ShellKind::Auto,
        );
        assert_eq!(outcome.code, 2);
        assert_eq!(
            String::from_utf8_lossy(&outcome.raw_stdout),
            "partial output"
        );
    }

    #[cfg(windows)]
    #[test]
    fn nonzero_exit_still_passes_through() {
        // Auto uses PowerShell on Windows; `exit N` sets its process exit code.
        let outcome = run_command(
            "[Console]::Out.Write('partial output'); exit 2",
            "cursor",
            None,
            ShellKind::Auto,
        );
        assert_eq!(outcome.code, 2);
        assert_eq!(
            String::from_utf8_lossy(&outcome.raw_stdout).trim_end(),
            "partial output"
        );
    }

    #[test]
    fn interactive_and_background_contracts_never_compact() {
        assert!(!shell_output_semantically_safe("tail -f app.log"));
        assert!(!shell_output_semantically_safe("watch cargo test"));
        assert!(!shell_output_semantically_safe("server &"));
        assert!(shell_output_semantically_safe("cargo test --locked"));
    }
}
