//! Journey: ctx simulate dry-run.

mod harness;

use harness::CtxHarness;
use serial_test::serial;
use std::process::Command;

#[test]
#[serial]
fn journey_simulate_single_profile() {
    let h = CtxHarness::new();
    h.write_config(
        r#"
active_profile = "carrier"
inject_enabled = true
coaching_enabled = false
auto_profile_enabled = false
adaptive_prefix_enabled = true
"#,
    );
    std::fs::write(
        h.tmp.path().join("system_prefix.md"),
        "You are a helpful assistant.\n",
    )
    .unwrap();
    std::fs::write(
        h.tmp.path().join("adaptive_prefix.md"),
        "Use typescript.\n",
    )
    .unwrap();

    let bin = option_env!("CARGO_BIN_EXE_ctx").expect("ctx binary");
    let out = Command::new(bin)
        .args(["simulate", "--prompt", "fix the bug", "--cwd", "/tmp", "--json"])
        .env("CTX_HOME", h.tmp.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["profile_slug"], "carrier");
    assert!(v["tools_removed"].as_u64().unwrap() > 0);
    assert!(v["inject_fired"].as_bool().unwrap());
    assert!(v["adaptive_fired"].as_bool().unwrap());
    assert!(v["estimated_cost_with_ctx"].as_f64().unwrap() > 0.0);
    assert!(v["savings_pct"].as_f64().unwrap() > 0.0);
}

#[test]
#[serial]
fn journey_simulate_all_profiles() {
    let h = CtxHarness::new();
    h.write_config("active_profile = \"all\"\nauto_profile_enabled = false\n");

    let bin = option_env!("CARGO_BIN_EXE_ctx").expect("ctx binary");
    let out = Command::new(bin)
        .args([
            "simulate",
            "--prompt",
            "test prompt",
            "--all-profiles",
            "--json",
        ])
        .env("CTX_HOME", h.tmp.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().expect("array of profiles");
    assert!(arr.len() >= 2);
    let all_entry = arr.iter().find(|r| r["profile_slug"] == "all").expect("all profile");
    assert_eq!(all_entry["tools_removed"].as_u64().unwrap(), 0);
}

#[test]
#[serial]
fn journey_simulate_replay() {
    let h = CtxHarness::new();
    h.write_config("active_profile = \"carrier\"\nauto_profile_enabled = false\n");
    for i in 0..3 {
        h.seed_hook_trace(&format!("replay-{i}"), None, None, 0.03, true);
    }

    let bin = option_env!("CARGO_BIN_EXE_ctx").expect("ctx binary");
    let out = Command::new(bin)
        .args(["simulate", "--replay-last", "3", "--json"])
        .env("CTX_HOME", h.tmp.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().expect("replay array");
    assert_eq!(arr.len(), 3);
    assert!(arr[0].get("simulated").is_some());
    assert!(arr[0].get("trace_id").is_some());
}
