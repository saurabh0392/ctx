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
use crate::shell_spool::ShellTrimSpool;

/// What `ctx run` decided to do with a command's output, kept separate from any I/O or process exit
/// so the core is unit-testable.
struct RunOutcome {
    code: i32,
    raw_stdout: Vec<u8>,
    raw_stderr: Vec<u8>,
    /// `Some` when ctx prepared a recoverable stdout compaction. Stderr always remains separate.
    compacted: Option<ShellTrimSpool>,
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
    hook_authorized: bool,
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

    let outcome = run_command(&command, surface, session_id, shell, hook_authorized);
    write_outcome(&outcome)?;
    if let Some(prepared) = &outcome.compacted {
        // A hook-authorized Codex child cannot write beside ~/.ctx; its PostToolUse hook imports the
        // already-durable temp receipt outside the sandbox. Legacy/unsandboxed callers keep their
        // direct persistence path and delete the spool only after the accounting transaction lands.
        if !hook_authorized {
            if let Err(error) = mark_shell_trim_emitted(prepared) {
                eprintln!("ctx run: output was emitted but applied receipt failed: {error}");
            }
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
    hook_authorized: bool,
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
            .and_then(|stdout| try_compact(command, stdout, surface, session_id, hook_authorized))
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
        out.write_all(prepared.trimmed.as_bytes())?;
        if !prepared.trimmed.ends_with('\n') {
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
    hook_authorized: bool,
) -> Option<ShellTrimSpool> {
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

    let command_or_path = crate::surface::fingerprint_tool_input(&tr.tool_name, &tr.tool_input);
    // The trusted pre-tool hook already performed the DB-backed activation/burn-in check outside
    // Codex's sandbox. Repeating it here is what made every normal Codex command fail closed:
    // SQLite could not create its WAL/SHM files under ~/.ctx. Direct callers still use the complete
    // controller locally.
    let decision = if hook_authorized {
        None
    } else {
        let decision = crate::agent::decide_for_surface(&cfg, &tr, surface);
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
        Some(decision)
    };

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
        if let Some(decision) = decision.as_ref() {
            crate::compress::record_shadow_decision(
                tr.session_id.as_deref(),
                &tr.tool_name,
                &command_or_path,
                decision.shadow.as_ref(),
                false,
                decision.explore_arm,
                Some(surface),
            );
        }
        return None;
    };
    if result.chars_saved() == 0 {
        if let Some(decision) = decision.as_ref() {
            crate::compress::record_shadow_decision(
                tr.session_id.as_deref(),
                &tr.tool_name,
                &command_or_path,
                decision.shadow.as_ref(),
                false,
                decision.explore_arm,
                Some(surface),
            );
        }
        return None;
    }

    let rewind_id = crate::shell_spool::rewind_id(command, stdout.as_bytes());
    let prepared_at = chrono::Utc::now().to_rfc3339();
    let marked = format!(
        "{}{}",
        result.text,
        crate::compress::trim_marker(&rewind_id)
    );
    if marked.chars().count() >= stdout.chars().count() {
        return None;
    }
    let prepared = ShellTrimSpool::new(
        command,
        stdout,
        marked,
        prepared_at,
        tr.session_id,
        command_or_path,
        result.strategy,
        surface.to_owned(),
        tr.cwd,
    );
    debug_assert_eq!(prepared.rewind_id, rewind_id);
    // The exact original must be durable before the shortened text can reach the model. This temp
    // receipt is writable inside Codex's sandbox and remains available to ctx_expand if import
    // fails, so a database outage can never make a trim irreversible.
    crate::shell_spool::write(&prepared).ok()?;
    Some(prepared)
}

/// Import a sandbox-safe shell receipt into the normal CTX database. Idempotent across hook retries
/// and crashes after commit: an already-emitted rewind is never counted twice.
pub(crate) fn import_shell_trim(rewind_id: &str) -> Result<bool> {
    let Some(prepared) = crate::shell_spool::load(rewind_id)? else {
        return Ok(false);
    };
    persist_shell_trim(&prepared)?;
    crate::shell_spool::remove(rewind_id)?;
    Ok(true)
}

/// Record a live Shell trim as a real apply: one decision, one compress event, and the analytics
/// counter, stamped with the originating surface. The spool is removed only after this succeeds.
fn mark_shell_trim_emitted(prepared: &ShellTrimSpool) -> Result<()> {
    persist_shell_trim(prepared)?;
    crate::shell_spool::remove(&prepared.rewind_id)
}

fn persist_shell_trim(prepared: &ShellTrimSpool) -> Result<()> {
    let mut conn = crate::db::open_db()?;
    crate::db::ensure_schema(&conn)?;
    let transaction = conn.transaction()?;
    crate::db::insert_rewind_checked(
        &transaction,
        &prepared.rewind_id,
        &prepared.prepared_at,
        prepared.session_id.as_deref(),
        &prepared.tool_name,
        &prepared.command_or_path,
        &prepared.original,
        &prepared.trimmed,
    )?;
    let already_emitted: i64 = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM compress_decisions
             WHERE applied=1 AND rewind_id=?1
         )",
        [&prepared.rewind_id],
        |row| row.get(0),
    )?;
    if already_emitted == 1 {
        transaction.commit()?;
        return Ok(());
    }
    let features_json = serde_json::json!({
        "adapter": "shell-wrapper-v3",
        "rewind": true,
        "sandbox_spool": true,
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
    crate::db::mark_decision_emitted(
        &transaction,
        decision_id,
        &prepared.rewind_id,
        prepared.chars_out,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn restore_env(name: &str, previous: Option<String>) {
        match previous {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    #[test]
    fn preserves_exit_code() {
        let outcome = run_command("exit 7", "cursor", None, ShellKind::Auto, false);
        assert_eq!(outcome.code, 7);
    }

    #[test]
    fn preserves_zero_exit_and_stdout() {
        #[cfg(not(windows))]
        let command = "printf 'hello world'";
        #[cfg(windows)]
        let command = "[Console]::Out.Write('hello world')";
        let outcome = run_command(command, "cursor", None, ShellKind::Auto, false);
        assert_eq!(outcome.code, 0);
        assert_eq!(String::from_utf8_lossy(&outcome.raw_stdout), "hello world");
    }

    #[test]
    fn captures_stderr_separately() {
        #[cfg(not(windows))]
        let command = "printf 'oops' 1>&2";
        #[cfg(windows)]
        let command = "[Console]::Error.Write('oops')";
        let outcome = run_command(command, "cursor", None, ShellKind::Auto, false);
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
        let outcome = run_command(command, "cursor", None, ShellKind::Auto, false);
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
            false,
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
            false,
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

    #[test]
    fn hook_authorized_trim_spools_then_imports_exactly_once() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let ctx_home = temp.path().join("ctx-home");
        let spool = temp.path().join("spool");
        let previous_home = std::env::var("CTX_HOME").ok();
        let previous_spool = std::env::var("CTX_TEST_SHELL_SPOOL").ok();
        std::env::set_var("CTX_HOME", &ctx_home);
        std::env::set_var("CTX_TEST_SHELL_SPOOL", &spool);

        let original = (0..5_000)
            .map(|line| format!("docs/file-{line}.md: repeated needle and surrounding context\n"))
            .collect::<String>();
        let prepared = try_compact(
            "rg needle docs",
            &original,
            "codex",
            Some("session-1"),
            true,
        )
        .expect("authorized output should trim without opening the database");
        assert!(prepared.trimmed.len() < original.len());
        assert!(!crate::config::db_path().exists());
        assert!(crate::shell_spool::load(&prepared.rewind_id)
            .unwrap()
            .is_some());

        assert!(import_shell_trim(&prepared.rewind_id).unwrap());
        let conn = crate::db::open_db().unwrap();
        crate::db::ensure_schema(&conn).unwrap();
        let stored = crate::db::get_rewind(&conn, &prepared.rewind_id).unwrap();
        assert_eq!(stored.original, original);
        let applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM compress_decisions
                 WHERE applied=1 AND rewind_id=?1",
                [&prepared.rewind_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);
        drop(conn);
        assert!(crate::shell_spool::load(&prepared.rewind_id)
            .unwrap()
            .is_none());

        // Simulate a crash after the database commit but before spool deletion. Re-importing the
        // same validated receipt must clean it up without duplicating the applied decision.
        crate::shell_spool::write(&prepared).unwrap();
        assert!(import_shell_trim(&prepared.rewind_id).unwrap());
        let conn = crate::db::open_db().unwrap();
        let applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM compress_decisions
                 WHERE applied=1 AND rewind_id=?1",
                [&prepared.rewind_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);

        restore_env("CTX_HOME", previous_home);
        restore_env("CTX_TEST_SHELL_SPOOL", previous_spool);
    }
}
