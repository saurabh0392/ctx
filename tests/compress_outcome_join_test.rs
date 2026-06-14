//! Regression guard for the shadow-decision outcome join.
//!
//! Two properties are pinned here:
//!   1. Corrections are read from the `correction` flag, not a `role = 'user'` filter.
//!      Real ingest stores turns with role `"turn"`; an earlier join filtered on
//!      `role = 'user'` and so labeled every decision clean.
//!   2. The correction is windowed. A short user turn an hour after a tool call is not
//!      caused by it, so only corrections within `CORRECTION_WINDOW_MINUTES` count, and a
//!      decision is only scored once that window has closed (or a correction landed in
//!      it), making the label independent of when ingest happened to run.

mod harness;

use harness::CtxHarness;
use serial_test::serial;

fn seed_session(conn: &rusqlite::Connection, external_key: &str) -> i64 {
    ctx::db::upsert_claude_session(
        conn,
        external_key,
        "proj",
        "2026-06-06T09:00:00+00:00",
        Some("2026-06-06T12:00:00+00:00"),
        2,
        0.0,
        0,
        0,
        0,
        0,
        "[]",
        0,
        0,
        1,
        "first message",
        "",
        "/tmp/proj",
        "[]",
    )
    .expect("upsert session")
}

fn seed_decision(conn: &rusqlite::Connection, session_id: &str, ts: &str, command: &str) {
    let d = ctx::db::CompressDecision {
        ts,
        session_id: Some(session_id),
        tool_name: "Bash",
        server_prefix: None,
        kind: "generic",
        task_mode: "normal",
        lines_total: 10,
        lines_keep: 8,
        lines_drop: 2,
        chars_in: 500,
        would_chars_out: 400,
        features_json: "{}",
        command_or_path: command,
        applied: false,
        explore_arm: None,
        surface: None,
    };
    ctx::db::insert_compress_decision(conn, &d).expect("insert decision");
}

fn seed_turn(
    conn: &rusqlite::Connection,
    session_id: i64,
    turn_index: i64,
    flags_json: &str,
    ts: &str,
) {
    // Mirror real ingest: role is "turn", corrections live in the flags column.
    ctx::db::insert_turn(
        conn,
        session_id,
        turn_index,
        "turn",
        0.0,
        0,
        0,
        0,
        0,
        "claude-opus",
        flags_json,
        "",
        Some(ts),
    )
    .expect("insert turn");
}

fn read_outcome(conn: &rusqlite::Connection, sid: &str) -> (i64, i64) {
    conn.query_row(
        "SELECT outcome_joined, COALESCE(outcome_correction,-1) FROM compress_decisions WHERE session_id = ?1",
        [sid],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .expect("read back row")
}

#[test]
#[serial]
fn correction_within_window_joins_as_correction() {
    let h = CtxHarness::new();
    let conn = h.open();

    let sid = "abc12345-corr";
    let session_row = seed_session(&conn, &format!("/path/{sid}.jsonl"));
    seed_decision(&conn, sid, "2026-06-06T10:00:00+00:00", "git status");
    // A correction five minutes later, well inside the window, stored with role "turn".
    seed_turn(
        &conn,
        session_row,
        1,
        r#"["correction","opus"]"#,
        "2026-06-06T10:05:00+00:00",
    );

    let joined = ctx::db::join_compress_outcomes(&conn).expect("join");
    assert_eq!(joined, 1, "the one eligible decision should join");

    let (joined_flag, correction) = read_outcome(&conn, sid);
    assert_eq!(
        joined_flag, 1,
        "a correction inside the window joins immediately"
    );
    assert_eq!(
        correction, 1,
        "a correction-flagged turn inside the window is a correction"
    );
}

#[test]
#[serial]
fn clean_turn_after_window_joins_without_correction() {
    let h = CtxHarness::new();
    let conn = h.open();

    let sid = "def67890-clean";
    let session_row = seed_session(&conn, &format!("/path/{sid}.jsonl"));
    seed_decision(&conn, sid, "2026-06-06T10:00:00+00:00", "ls -la");
    // A clean turn past the window closes it and confirms a clean run.
    seed_turn(
        &conn,
        session_row,
        1,
        r#"["opus"]"#,
        "2026-06-06T10:30:00+00:00",
    );

    let joined = ctx::db::join_compress_outcomes(&conn).expect("join");
    assert_eq!(joined, 1);

    let (joined_flag, correction) = read_outcome(&conn, sid);
    assert_eq!(joined_flag, 1, "the closed window lets the row join");
    assert_eq!(
        correction, 0,
        "a clean run inside the window is not a correction"
    );
}

#[test]
#[serial]
fn correction_outside_window_does_not_count() {
    let h = CtxHarness::new();
    let conn = h.open();

    let sid = "jkl24680-late";
    let session_row = seed_session(&conn, &format!("/path/{sid}.jsonl"));
    seed_decision(&conn, sid, "2026-06-06T10:00:00+00:00", "cargo test");
    // A correction a full hour later: the user moved on, this is not caused by the tool.
    // The turn is past the window, so it closes the window and confirms a clean run.
    seed_turn(
        &conn,
        session_row,
        1,
        r#"["correction","opus"]"#,
        "2026-06-06T11:00:00+00:00",
    );

    let joined = ctx::db::join_compress_outcomes(&conn).expect("join");
    assert_eq!(
        joined, 1,
        "a turn past the window closes it, so the row joins"
    );

    let (joined_flag, correction) = read_outcome(&conn, sid);
    assert_eq!(joined_flag, 1);
    assert_eq!(
        correction, 0,
        "a correction an hour later is unrelated and must not be labeled a correction"
    );
}

#[test]
#[serial]
fn correction_attributes_only_to_nearest_preceding_decision() {
    // Two decisions, then one correction turn. The fan-out bug labeled BOTH a correction;
    // the nearest-preceding rule must attribute it only to the later (closer) decision.
    let h = CtxHarness::new();
    let conn = h.open();

    let sid = "nearest-attr-01";
    let session_row = seed_session(&conn, &format!("/path/{sid}.jsonl"));
    seed_decision(&conn, sid, "2026-06-06T10:00:00+00:00", "first cmd");
    seed_decision(&conn, sid, "2026-06-06T10:03:00+00:00", "second cmd");
    // One correction four minutes after the second decision (in window for both).
    seed_turn(
        &conn,
        session_row,
        1,
        r#"["correction"]"#,
        "2026-06-06T10:07:00+00:00",
    );

    let joined = ctx::db::join_compress_outcomes(&conn).expect("join");
    assert_eq!(
        joined, 2,
        "both decisions join (the in-window correction closes them)"
    );

    let read_by_cmd = |cmd: &str| -> i64 {
        conn.query_row(
            "SELECT COALESCE(outcome_correction,-1) FROM compress_decisions WHERE command_or_path = ?1",
            [cmd],
            |r| r.get(0),
        )
        .expect("read row")
    };
    assert_eq!(
        read_by_cmd("second cmd"),
        1,
        "the nearest decision owns the correction"
    );
    assert_eq!(
        read_by_cmd("first cmd"),
        0,
        "the earlier decision must not be fanned the same correction"
    );
}

#[test]
#[serial]
fn decision_without_later_evidence_stays_unjoined() {
    let h = CtxHarness::new();
    let conn = h.open();

    let sid = "ghi13579-pending";
    let session_row = seed_session(&conn, &format!("/path/{sid}.jsonl"));
    seed_decision(&conn, sid, "2026-06-06T11:00:00+00:00", "cargo build");
    // Only an earlier turn exists; the window has not closed and no correction is in it.
    seed_turn(
        &conn,
        session_row,
        1,
        r#"["correction"]"#,
        "2026-06-06T10:00:00+00:00",
    );

    let joined = ctx::db::join_compress_outcomes(&conn).expect("join");
    assert_eq!(
        joined, 0,
        "an open window with no in-window correction stays unjoined"
    );

    let (joined_flag, _) = read_outcome(&conn, sid);
    assert_eq!(
        joined_flag, 0,
        "a decision is never scored before its window closes"
    );
}
