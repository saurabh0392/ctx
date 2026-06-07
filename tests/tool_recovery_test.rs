//! Stop-hook access friction recovery and proactive expansion.

use ctx::config::{Config, FilterMode};
use ctx::profiles::Profile;
use ctx::semantic_tools::{self, ExpansionReason, ToolExpansionEntry};
use ctx::test_lock::CTX_ENV_LOCK;
use serde_json::json;

fn with_ctx_home<F: FnOnce()>(f: F) {
    let _guard = CTX_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let prev_home = std::env::var("HOME").ok();
    // Isolate BOTH homes. CTX_HOME relocates ~/.ctx, but the friction-recovery path writes
    // ~/.claude/settings.json (write_native_ctx_to_user_settings via current_exe()). Without
    // an isolated HOME that write lands on the real settings file and clobbers the live
    // PostToolUse collection hook with this test binary's path.
    std::env::set_var("CTX_HOME", tmp.path());
    std::env::set_var("HOME", tmp.path());
    std::fs::create_dir_all(tmp.path().join(".claude")).ok();
    f();
    std::env::remove_var("CTX_HOME");
    match prev_home {
        Some(p) => std::env::set_var("HOME", p),
        None => std::env::remove_var("HOME"),
    }
}

fn seed_notion_tool(conn: &rusqlite::Connection) -> i64 {
    let sid = conn
        .execute(
            "INSERT INTO sessions (external_key, project, started_at, profile) VALUES ('t', 'p', '2026-01-01', 'test')",
            [],
        )
        .unwrap();
    let pk = conn.last_insert_rowid();
    let ts = chrono::Utc::now().to_rfc3339();
    ctx::db::insert_tool_invocation(
        conn,
        pk,
        None,
        "mcp__claude_ai_Notion__notion-search",
        "mcp__claude_ai_Notion__",
        &ts,
    )
    .unwrap();
    let _ = sid;
    pk
}

fn write_test_profile() {
    use std::io::Write;
    let path = ctx::config::ctx_dir().join("profiles.toml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let mut f = std::fs::File::create(path).unwrap();
    write!(
        f,
        r#"[test]
display = "Test"
description = "test"
keep = []
keep_tools = ["mcp__claude_ai_Figma__use_figma"]
"#
    )
    .unwrap();
}

#[test]
fn stop_hook_recovery_expands_and_merges_trace() {
    with_ctx_home(|| {
        write_test_profile();
        let mut cfg = Config::load();
        cfg.filter_mode = FilterMode::Soft;
        cfg.active_profile = Some("test".into());
        cfg.save().unwrap();

        let conn = ctx::db::open_db().unwrap();
        ctx::db::ensure_schema(&conn).unwrap();
        seed_notion_tool(&conn);

        let sid = "abc-session-123";
        ctx::db::insert_hook_trace(
            &conn,
            Some(sid),
            None,
            "/tmp/project",
            "test",
            None,
            false,
            None,
            false,
            None,
            false,
            10,
            5,
            1000,
            false,
            None,
            0,
            0,
            false,
            None,
            None,
            Some("find notion page"),
            Some("[]"),
        )
        .unwrap();

        let payload = json!({
            "session_id": sid,
            "transcript": [
                {"role": "user", "content": "find notion page"},
                {"role": "assistant", "content": [{"type": "text", "text": "I don't have access to Notion for this."}]}
            ]
        });

        let added = semantic_tools::process_stop_hook_recovery(&payload).unwrap();
        assert!(!added.is_empty(), "expected friction recovery, got {added:?}");
        assert_eq!(added[0].reason, ExpansionReason::AccessFriction);

        let traces = ctx::db::load_hook_traces(&conn, 5, 0, None).unwrap();
        assert_eq!(traces.len(), 1);
        assert!(!traces[0].tools_expanded.is_empty());
    });
}

#[test]
fn proactive_keyword_expansion_records_reason() {
    with_ctx_home(|| {
        write_test_profile();
        let mut cfg = Config::load();
        cfg.filter_mode = FilterMode::Soft;
        cfg.active_profile = Some("test".into());
        cfg.save().unwrap();

        let conn = ctx::db::open_db().unwrap();
        ctx::db::ensure_schema(&conn).unwrap();
        seed_notion_tool(&conn);

        let profile = Profile {
            display: "Test".into(),
            description: "test".into(),
            keep: vec![],
            keep_tools: vec!["mcp__claude_ai_Figma__use_figma".into()],
            ..Default::default()
        };

        let added = semantic_tools::expand_from_prompt_keywords(
            "Please search Notion for the roadmap doc",
            "/tmp/design",
            &profile,
        )
        .unwrap();
        assert!(
            added.iter().any(|e| e.reason == ExpansionReason::Keyword),
            "got {added:?}"
        );
    });
}

#[test]
fn append_hook_trace_expansions_dedupes() {
    with_ctx_home(|| {
        let conn = ctx::db::open_db().unwrap();
        ctx::db::ensure_schema(&conn).unwrap();
        ctx::db::insert_hook_trace(
            &conn,
            Some("sess-1"),
            None,
            ".",
            "personal",
            None,
            false,
            None,
            false,
            None,
            false,
            0,
            0,
            0,
            false,
            None,
            0,
            0,
            false,
            None,
            None,
            None,
            Some(r#"[{"target":"mcp__x","reason":"keyword","display":"x"}]"#),
        )
        .unwrap();

        let entry = ToolExpansionEntry::new("mcp__y", ExpansionReason::AccessFriction);
        ctx::db::append_hook_trace_expansions(&conn, "sess-1", &[entry]).unwrap();
        let traces = ctx::db::load_hook_traces(&conn, 1, 0, None).unwrap();
        assert_eq!(traces[0].tools_expanded.len(), 2);
    });
}
