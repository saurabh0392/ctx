//! Golden contract for the PostToolUse hot path after it was routed through the
//! canonical Claude Code surface adapter (Phase 2).
//!
//! The refactor must be inert: the bytes the hook emits to Claude Code have to match
//! what you get by composing the public functions directly (extract -> compress -> wrap).
//! This drives the real `ctx hook post-tool-use` binary end to end and compares its
//! `updatedToolOutput` to that direct composition.

mod harness;

use harness::CtxHarness;
use serde_json::{json, Value};
use serial_test::serial;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
#[serial]
fn post_tool_use_output_matches_direct_composition() {
    let bin = env!("CARGO_BIN_EXE_ctx");
    let h = CtxHarness::new();
    // Force the apply path: preset full allows every kind, force_active bypasses the
    // Act 1 evidence gate (which would otherwise fail closed with no collected labels).
    h.write_config(
        r#"
compress_enabled = true
compress_preset = "full"
compress_force_active = true
compress_tools = ["Bash", "Read", "Grep", "Glob"]
compress_max_output_chars = 12000
compress_target_chars = 400
compress_redact_secrets = true
compress_preserve_errors = true
compress_explore_rate = 0.0
"#,
    );

    // A git status result with enough output to clear the target and compress.
    let mut stdout = String::from(
        "On branch main\nYour branch is up to date with 'origin/main'.\n\nChanges not staged for commit:\n",
    );
    for i in 0..80 {
        stdout.push_str(&format!("\tmodified:   src/file_{i}.rs\n"));
    }
    let command = "git status";
    let cwd = "/tmp";
    let session_id = "golden-sess";

    let payload = json!({
        "session_id": session_id,
        "cwd": cwd,
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "tool_response": {"stdout": stdout, "stderr": ""}
    });

    let mut child = Command::new(bin)
        .args(["hook", "post-tool-use"])
        .current_dir(h.tmp.path())
        .env("CTX_HOME", h.tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn ctx hook");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write payload");
    let out = child.wait_with_output().expect("hook output");
    assert!(out.status.success(), "hook should exit cleanly");

    let stdout_str = String::from_utf8_lossy(&out.stdout);
    let emitted: Value =
        serde_json::from_str(stdout_str.trim()).expect("hook must emit a JSON object");

    let hso = emitted
        .get("hookSpecificOutput")
        .expect("hookSpecificOutput present");
    assert_eq!(
        hso.get("hookEventName").and_then(|v| v.as_str()),
        Some("PostToolUse")
    );
    let uto = hso
        .get("updatedToolOutput")
        .expect("updatedToolOutput present");
    let compressed_stdout = uto
        .get("stdout")
        .and_then(|v| v.as_str())
        .expect("Bash updatedToolOutput keeps a stdout string");
    assert!(
        compressed_stdout.len() < stdout.len(),
        "the hot path must actually compress the output"
    );

    // Golden: the emitted shape equals composing the public functions directly under the
    // same config and CTX_HOME. Proves the adapter routing did not alter the result.
    let cfg = ctx::config::Config::load();
    let response = json!({"stdout": stdout, "stderr": ""});
    let result = ctx::compress::compress_tool_output(
        "Bash",
        &json!({"command": command}),
        &stdout,
        &cfg,
        Some(session_id),
        cwd,
        false,
    )
    .expect("direct compose should compress");
    // The hook appends the reversible-trim marker (CTX-51) before wrapping, so the faithful golden
    // must do the same. Shared helpers keep the id and marker text in lockstep with the hook.
    let rewind_id = ctx::compress::rewind_id_for(&stdout);
    let marked = format!("{}{}", result.text, ctx::compress::trim_marker(&rewind_id));
    let expected = ctx::compress::wrap_updated_tool_output("Bash", &response, &marked);

    assert_eq!(
        uto, &expected,
        "routing through ClaudeCodeTransport must be byte-identical to direct composition"
    );
}
