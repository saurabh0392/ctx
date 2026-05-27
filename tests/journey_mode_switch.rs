//! Context mode switch: config, hook trace mode column, list.

mod harness;

use harness::CtxHarness;
use serial_test::serial;
use std::io::Write;
use std::process::Command;

#[test]
#[serial]
fn journey_mode_switch() {
    let h = CtxHarness::new();
    h.write_config(
        r#"
active_profile = "all"
inject_enabled = true
coaching_enabled = true
adaptive_prefix_enabled = true
auto_profile_enabled = false

[modes.debug]
profile = "minimal"
inject_enabled = true
coaching_enabled = true
adaptive_prefix_enabled = false

[modes.review]
profile = "carrier"
inject_enabled = true
coaching_enabled = false
adaptive_prefix_enabled = true
"#,
    );

    let bin = option_env!("CARGO_BIN_EXE_ctx").expect("ctx binary");

    let out = Command::new(bin)
        .args(["mode", "debug"])
        .env("CTX_HOME", h.tmp.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let cfg = std::fs::read_to_string(h.tmp.path().join("config.toml")).unwrap();
    assert!(cfg.contains("active_mode = \"debug\""));
    assert!(cfg.contains("active_profile = \"minimal\""));
    assert!(cfg.contains("coaching_enabled = true"));

    let stdin_json = serde_json::json!({
        "cwd": "/tmp",
        "prompt": "mode test",
        "session_id": "mode-sess-1"
    })
    .to_string();
    let mut child = Command::new(bin)
        .args(["hook", "user-prompt-submit"])
        .env("CTX_HOME", h.tmp.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut sin = child.stdin.take().unwrap();
        sin.write_all(stdin_json.as_bytes()).unwrap();
    }
    assert!(child.wait().unwrap().success());

    let conn = h.open();
    let mode: String = conn
        .query_row(
            "SELECT mode FROM hook_traces ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mode, "debug");

    let out = Command::new(bin)
        .args(["mode", "review"])
        .env("CTX_HOME", h.tmp.path())
        .output()
        .unwrap();
    assert!(out.status.success());

    let stdin_json = serde_json::json!({
        "cwd": "/tmp",
        "prompt": "mode test 2",
        "session_id": "mode-sess-2"
    })
    .to_string();
    let mut child = Command::new(bin)
        .args(["hook", "user-prompt-submit"])
        .env("CTX_HOME", h.tmp.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut sin = child.stdin.take().unwrap();
        sin.write_all(stdin_json.as_bytes()).unwrap();
    }
    assert!(child.wait().unwrap().success());

    let mode2: String = conn
        .query_row(
            "SELECT mode FROM hook_traces ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mode2, "review");

    let list = Command::new(bin)
        .args(["mode", "list"])
        .env("CTX_HOME", h.tmp.path())
        .output()
        .unwrap();
    let list_out = String::from_utf8_lossy(&list.stdout);
    assert!(list_out.contains("debug"));
    assert!(list_out.contains("review"));
}
