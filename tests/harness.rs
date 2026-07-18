//! Shared helpers for integration tests: isolated `CTX_HOME`, schema, seed rows.

#![allow(dead_code)]

use ctx::db;
use rusqlite::Connection;
use std::sync::MutexGuard;
use tempfile::TempDir;

pub fn ctx_env_lock() -> MutexGuard<'static, ()> {
    ctx::test_lock::CTX_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub struct CtxHarness {
    pub tmp: TempDir,
    prev_home: Option<String>,
    prev_ctx_home: Option<String>,
    prev_ctx_test_home: Option<String>,
    _guard: MutexGuard<'static, ()>,
}

impl Default for CtxHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl CtxHarness {
    pub fn new() -> Self {
        let _guard = ctx_env_lock();
        let tmp = tempfile::tempdir().expect("tempdir");
        let prev_home = std::env::var("HOME").ok();
        let prev_ctx_home = std::env::var("CTX_HOME").ok();
        let prev_ctx_test_home = std::env::var("CTX_TEST_HOME").ok();
        std::env::set_var("CTX_HOME", tmp.path());
        // Isolate HOME too: several code paths (experiment tick, friction recovery) write
        // ~/.claude/settings.json, which is resolved from HOME, not CTX_HOME. Without this a
        // test run clobbers the live PostToolUse collection hook in the real settings file.
        std::env::set_var("HOME", tmp.path());
        // dirs::home_dir() ignores HOME on Windows, so also set the cross-platform test-home
        // override that config::home_dir_for_paths() honors, isolating ~/.claude and ~/.cursor.
        std::env::set_var("CTX_TEST_HOME", tmp.path());
        let _ = std::fs::create_dir_all(tmp.path());
        let _ = std::fs::create_dir_all(tmp.path().join(".claude"));
        let conn = db::open_db().expect("open_db");
        db::ensure_schema(&conn).expect("ensure_schema");
        drop(conn);
        Self {
            tmp,
            prev_home,
            prev_ctx_home,
            prev_ctx_test_home,
            _guard,
        }
    }

    pub fn open(&self) -> Connection {
        db::open_db().expect("open_db")
    }

    pub fn write_config(&self, toml: &str) {
        std::fs::write(self.tmp.path().join("config.toml"), toml).expect("write config");
    }

    /// Minimal session + one user turn + one tool row for adaptive prefix queries.
    pub fn seed_session_tool_and_correction(&self) {
        let conn = self.open();
        conn.execute(
            "INSERT INTO sessions (external_key, project, started_at, profile, working_directory, turn_count, first_user_message, embed_text)
             VALUES ('ext-journey', 'p1', datetime('now'), 'all', '/tmp', 1, 'fix the python bug', 'fix the python bug')",
            [],
        )
        .expect("insert session");
        let sid: i64 = conn
            .query_row(
                "SELECT id FROM sessions WHERE external_key='ext-journey'",
                [],
                |r| r.get(0),
            )
            .expect("session id");
        conn.execute(
            "INSERT INTO turns (session_id, turn_index, role, human_text_prefix, flags, ts)
             VALUES (?1, 0, 'user', 'please use typescript for the fix', 'correction', datetime('now'))",
            [sid],
        )
        .expect("insert turn");
        let tid: i64 = conn
            .query_row(
                "SELECT id FROM turns WHERE session_id=?1 ORDER BY id DESC LIMIT 1",
                [sid],
                |r| r.get(0),
            )
            .expect("turn id");
        conn.execute(
            "INSERT INTO tool_invocations (session_id, turn_id, tool_name, server_prefix, ts)
             VALUES (?1, ?2, 'x', 'mcp__claude_ai_Slack__', datetime('now'))",
            rusqlite::params![sid, tid],
        )
        .expect("insert tool_invocation");
    }

    /// Insert a hook_traces row (for subagent / mode journey tests).
    pub fn seed_hook_trace(
        &self,
        session_id: &str,
        parent_session_id: Option<&str>,
        mode: Option<&str>,
        cost_usd: f64,
        enriched: bool,
    ) {
        let conn = self.open();
        conn.execute(
            r#"INSERT INTO hook_traces (
                ts, session_id, parent_session_id, working_directory, profile, mode,
                tools_kept, tools_removed, tokens_saved, cost_usd, enriched
            ) VALUES (datetime('now'), ?1, ?2, '/tmp/project', 'carrier', ?3, 5, 10, 42000, ?4, ?5)"#,
            rusqlite::params![
                session_id,
                parent_session_id,
                mode,
                cost_usd,
                enriched as i64
            ],
        )
        .expect("insert hook_trace");
    }
}

impl Drop for CtxHarness {
    fn drop(&mut self) {
        match &self.prev_home {
            Some(p) => std::env::set_var("HOME", p),
            None => std::env::remove_var("HOME"),
        }
        match &self.prev_ctx_home {
            Some(p) => std::env::set_var("CTX_HOME", p),
            None => std::env::remove_var("CTX_HOME"),
        }
        match &self.prev_ctx_test_home {
            Some(p) => std::env::set_var("CTX_TEST_HOME", p),
            None => std::env::remove_var("CTX_TEST_HOME"),
        }
    }
}
