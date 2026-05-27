//! Subprocess: `ctx hook user-prompt-submit` reads JSON stdin and emits `additionalContext` when adaptive file exists.

mod harness;

use harness::CtxHarness;
use serde_json::Value;
use serial_test::serial;
use std::io::Write;
use std::process::Command;

#[test]
#[serial]
fn hook_user_prompt_submit_includes_adaptive_in_additional_context() {
    let bin = option_env!("CARGO_BIN_EXE_ctx").expect("integration tests need the ctx binary");
    let h = CtxHarness::new();
    h.write_config(
        r#"
active_profile = "all"
inject_enabled = false
coaching_enabled = false
auto_profile_enabled = false
adaptive_prefix_enabled = true
"#,
    );
    let mark = "HOOK_CONTRACT_ADAPTIVE_MARK_9f3a";
    std::fs::write(
        h.tmp.path().join("adaptive_prefix.md"),
        format!("# adaptive test\n{mark}\n"),
    )
    .unwrap();
    let conn = h.open();
    ctx::db::ensure_schema(&conn).unwrap();
    drop(conn);

    let stdin_json = serde_json::json!({
        "cwd": "/tmp",
        "prompt": "ping",
        "model": "claude-sonnet-4-20250514"
    })
    .to_string();

    let mut child = Command::new(bin)
        .args(["hook", "user-prompt-submit"])
        .current_dir(h.tmp.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ctx hook");
    {
        let mut sin = child.stdin.take().expect("stdin");
        sin.write_all(stdin_json.as_bytes()).expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait ctx hook");

    assert!(
        out.status.success(),
        "hook stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).expect("stdout json");
    let ctx = v
        .pointer("/hookSpecificOutput/additionalContext")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    assert!(
        ctx.contains(mark),
        "additionalContext missing adaptive mark; got: {v:?}"
    );

    let conn = h.open();
    let fired: i64 = conn
        .query_row(
            "SELECT adaptive_fired FROM hook_traces ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("adaptive_fired row");
    assert_eq!(fired, 1, "hook_trace.adaptive_fired should be 1");
}
