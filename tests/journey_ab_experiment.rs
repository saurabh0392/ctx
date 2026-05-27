//! End-to-end A/B experiment: hooks, enrich, cohort assignment.

mod harness;

use harness::CtxHarness;
use serial_test::serial;
use std::io::Write;
use std::process::Command;

#[test]
#[serial]
fn journey_ab_experiment_hooks_and_enrich() {
    let h = CtxHarness::new();
    h.write_config(
        r#"
active_profile = "all"
inject_enabled = false
coaching_enabled = false
auto_profile_enabled = false
adaptive_prefix_enabled = false

[ab_test]
profile_pct = 50
inject_pct = 100
adaptive_pct = 50
coaching_pct = 100
"#,
    );

    let conn = h.open();
    for i in 0..10 {
        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count, first_user_message, embed_text)
             VALUES (?1, 'p', datetime('now'), 'all', '/tmp', 1, 'hello', 'hello')",
            [format!("sess-{i}")],
        )
        .unwrap();
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key = ?1",
                [format!("sess-{i}")],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, human_text_prefix, input_tokens, output_tokens, cost_usd, model, ts)
             VALUES (?1, 0, 'user', ?2, 1000, 500, 0.05, 'claude', datetime('now'))",
            rusqlite::params![sid, format!("prompt {i}")],
        )
        .unwrap();
    }
    drop(conn);

    let bin = option_env!("CARGO_BIN_EXE_ctx").expect("ctx binary");
    for i in 0..20 {
        let stdin_json = serde_json::json!({
            "cwd": "/tmp",
            "prompt": format!("journey prompt {i}"),
            "session_id": format!("sess-{}", i % 10)
        })
        .to_string();
        let mut child = Command::new(bin)
            .args(["hook", "user-prompt-submit"])
            .current_dir(h.tmp.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        {
            let mut sin = child.stdin.take().unwrap();
            sin.write_all(stdin_json.as_bytes()).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "hook failed: {}", String::from_utf8_lossy(&out.stderr));
    }

    let conn = h.open();
    let p_t: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM hook_traces WHERE ab_group LIKE '%P:T%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let p_c: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM hook_traces WHERE ab_group LIKE '%P:C%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(p_t > 0 && p_c > 0, "expected both profile treatment and control, got T={p_t} C={p_c}");

    ctx::db::enrich_hook_traces(&conn).unwrap();
    let enriched: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM hook_traces WHERE enriched = 1 AND ab_group IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(enriched > 0);

    let with_prompt: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM hook_traces WHERE human_text_prefix IS NOT NULL AND LENGTH(human_text_prefix) > 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(with_prompt > 0, "enrich should copy human_text_prefix");

    h.write_config(
        r#"
active_profile = "all"
inject_enabled = false
coaching_enabled = false
auto_profile_enabled = false
adaptive_prefix_enabled = false

[ab_test]
profile_pct = 100
inject_pct = 100
adaptive_pct = 100
coaching_pct = 100
"#,
    );
    for i in 0..5 {
        let stdin_json = serde_json::json!({
            "cwd": "/tmp",
            "prompt": format!("after stop {i}")
        })
        .to_string();
        let mut child = Command::new(bin)
            .args(["hook", "user-prompt-submit"])
            .current_dir(h.tmp.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        {
            let mut sin = child.stdin.take().unwrap();
            sin.write_all(stdin_json.as_bytes()).unwrap();
        }
        child.wait().unwrap();
    }

    let recent_null: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT ab_group FROM hook_traces ORDER BY id DESC LIMIT 5) WHERE ab_group IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(recent_null, 5, "last 5 hooks should have null ab_group");
}
