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
active_profile = "minimal"
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
    std::fs::write(h.tmp.path().join("adaptive_prefix.md"), "Use typescript.\n").unwrap();
    {
        let conn = h.open();
        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count)
             VALUES ('sim-seed', 'p1', datetime('now'), 'carrier', '/tmp', 1)",
            [],
        )
        .unwrap();
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key='sim-seed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, human_text_prefix, ts)
             VALUES (?1, 0, 'user', 'fix the bug', datetime('now'))",
            [sid],
        )
        .unwrap();
        let tid: i64 = conn
            .query_row("SELECT id FROM turns WHERE session_id=?1", [sid], |r| {
                r.get(0)
            })
            .unwrap();
        for i in 0..20 {
            conn.execute(
                "INSERT INTO tool_invocations (session_id, turn_id, tool_name, server_prefix, ts)
                 VALUES (?1, ?2, ?3, 'mcp__claude_ai_Slack__', datetime('now'))",
                rusqlite::params![sid, tid, format!("slack_tool_{i}")],
            )
            .unwrap();
        }
        for i in 0..20 {
            conn.execute(
                "INSERT INTO tool_invocations (session_id, turn_id, tool_name, server_prefix, ts)
                 VALUES (?1, ?2, ?3, 'mcp__claude_ai_Atlassian__', datetime('now'))",
                rusqlite::params![sid, tid, format!("jira_tool_{i}")],
            )
            .unwrap();
        }
    }

    let bin = env!("CARGO_BIN_EXE_ctx");
    let out = Command::new(bin)
        .args([
            "simulate",
            "--prompt",
            "fix the bug",
            "--cwd",
            "/tmp",
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
    assert_eq!(v["profile_slug"], "minimal");
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

    let bin = env!("CARGO_BIN_EXE_ctx");
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
    let all_entry = arr
        .iter()
        .find(|r| r["profile_slug"] == "all")
        .expect("all profile");
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

    let bin = env!("CARGO_BIN_EXE_ctx");
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
