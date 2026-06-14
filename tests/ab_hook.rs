//! A/B hook integration: control profile assignment and null ab_group when inactive.

mod harness;

use harness::CtxHarness;
use serde_json::Value;
use serial_test::serial;
use std::io::Write;
use std::process::Command;

fn run_hook(h: &CtxHarness, stdin_json: &str) -> Value {
    let bin = option_env!("CARGO_BIN_EXE_ctx").expect("integration tests need the ctx binary");
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
    serde_json::from_slice(&out.stdout).expect("stdout json")
}

#[test]
#[serial]
fn hook_profile_control_zero_pct_strips_no_tools_in_trace() {
    let h = CtxHarness::new();
    h.write_config(
        r#"
active_profile = "all"
inject_enabled = false
coaching_enabled = false
auto_profile_enabled = true
adaptive_prefix_enabled = false

[ab_test]
profile_pct = 0
inject_pct = 100
adaptive_pct = 100
coaching_pct = 100
"#,
    );

    let stdin_json = serde_json::json!({
        "cwd": "/tmp",
        "prompt": "ping",
        "model": "claude-sonnet-4-20250514"
    })
    .to_string();

    run_hook(&h, &stdin_json);

    let conn = h.open();
    let (removed, ab_group): (i64, Option<String>) = conn
        .query_row(
            "SELECT tools_removed, ab_group FROM hook_traces ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("hook trace row");
    assert_eq!(
        removed, 0,
        "control profile should report zero tools removed"
    );
    let group = ab_group.expect("ab_group should be set when experiment active");
    assert!(
        group.contains("P:C"),
        "expected profile control in ab_group, got {group}"
    );
}

#[test]
#[serial]
fn hook_profile_control_clears_runtime_deny() {
    let h = CtxHarness::new();
    std::env::set_var("HOME", h.tmp.path());
    let claude_dir = h.tmp.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("settings.json"),
        r#"{"permissions":{"deny":["Bash(rm *)"]}}"#,
    )
    .unwrap();

    h.write_config(
        r#"
active_profile = "data"
filter_mode = "soft"
inject_enabled = false
coaching_enabled = false
auto_profile_enabled = false
adaptive_prefix_enabled = false

[ab_test]
profile_pct = 0
inject_pct = 100
adaptive_pct = 100
coaching_pct = 100
"#,
    );

    ctx::profiles::apply_profile("data", true, true).expect("seed treatment deny");
    let before: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(claude_dir.join("settings.json")).unwrap())
            .unwrap();
    let deny_before = before["permissions"]["deny"].as_array().unwrap();
    assert!(
        deny_before.iter().any(|v| {
            v.as_str()
                .map(|s| s.starts_with("mcp__claude_ai_"))
                .unwrap_or(false)
        }),
        "expected ctx deny before control hook"
    );

    let stdin_json = serde_json::json!({
        "cwd": "/tmp",
        "prompt": "ping",
        "model": "claude-sonnet-4-20250514"
    })
    .to_string();
    run_hook(&h, &stdin_json);

    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(claude_dir.join("settings.json")).unwrap())
            .unwrap();
    let deny_after = after["permissions"]["deny"].as_array().unwrap();
    assert!(
        deny_after.iter().any(|v| v.as_str() == Some("Bash(rm *)")),
        "user deny rule should be preserved"
    );
    assert!(
        !deny_after.iter().any(|v| {
            v.as_str()
                .map(|s| ctx::profiles::is_ctx_managed_deny_pattern(s))
                .unwrap_or(false)
        }),
        "control hook should strip ctx-managed deny rules"
    );

    std::env::remove_var("HOME");
}

#[test]
#[serial]
fn hook_default_config_ab_group_null() {
    let h = CtxHarness::new();
    h.write_config(
        r#"
active_profile = "all"
inject_enabled = false
coaching_enabled = false
auto_profile_enabled = false
adaptive_prefix_enabled = false
"#,
    );

    let stdin_json = serde_json::json!({
        "cwd": "/tmp",
        "prompt": "ping"
    })
    .to_string();

    run_hook(&h, &stdin_json);

    let conn = h.open();
    let ab_group: Option<String> = conn
        .query_row(
            "SELECT ab_group FROM hook_traces ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("ab_group");
    assert!(
        ab_group.is_none(),
        "no experiment => ab_group should be NULL"
    );
}
