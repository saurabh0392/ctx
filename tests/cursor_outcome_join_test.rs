//! Phase 4: the ordinal/fingerprint outcome join makes Cursor shadow decisions joinable.
//!
//! Cursor transcripts have no timestamps, so this join places a decision on the timeline
//! by matching its `command_or_path` to a transcript tool call, then looks at later
//! ordinals for a correction or abort. This drives the real `join_transcript_outcomes`
//! against a fixture transcript plus seeded decisions.

mod harness;

use harness::CtxHarness;
use serial_test::serial;
use std::path::Path;

fn write_cursor_transcript(home: &Path, uuid: &str) {
    let dir = home
        .join(".cursor/projects/Users-me-Projects-ctx/agent-transcripts")
        .join(uuid);
    std::fs::create_dir_all(&dir).unwrap();
    let lines = [
        r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>add the cursor outcome join and walk me through the timeline alignment in detail please</user_query>"}]}}"#,
        r#"{"role":"assistant","message":{"content":[{"type":"text","text":"Reading the agent module and checking git state before making the change so I understand the alignment."},{"type":"tool_use","name":"Read","input":{"path":"/x/a.rs"}},{"type":"tool_use","name":"Shell","input":{"command":"git status"}}]}}"#,
        r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>no, that's broken, revert</user_query>"}]}}"#,
        r#"{"role":"assistant","message":{"content":[{"type":"text","text":"Reverting and re-checking the tree to confirm it is clean again."},{"type":"tool_use","name":"Shell","input":{"command":"git status"}}]}}"#,
        r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>ok thanks that works now</user_query>"}]}}"#,
    ];
    std::fs::write(dir.join(format!("{uuid}.jsonl")), lines.join("\n")).unwrap();
}

fn seed_decision(conn: &rusqlite::Connection, session_id: &str, command: &str, tool: &str) {
    let d = ctx::db::CompressDecision {
        ts: "2026-06-06T10:00:00+00:00",
        session_id: Some(session_id),
        tool_name: tool,
        server_prefix: None,
        kind: "generic",
        task_mode: "normal",
        lines_total: 10,
        lines_keep: 8,
        lines_drop: 2,
        chars_in: 600,
        would_chars_out: 400,
        features_json: "{}",
        command_or_path: command,
        applied: false,
        explore_arm: None,
        surface: None,
    };
    ctx::db::insert_compress_decision(conn, &d).expect("insert decision");
}

fn outcome(conn: &rusqlite::Connection, command: &str) -> (i64, i64, i64) {
    conn.query_row(
        "SELECT outcome_joined, COALESCE(outcome_correction,-1), COALESCE(outcome_reread,-1)
         FROM compress_decisions WHERE command_or_path = ?1",
        [command],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .expect("read decision")
}

#[test]
#[serial]
fn cursor_decisions_join_via_ordinal_timeline() {
    let h = CtxHarness::new();
    let cursor_home = tempfile::tempdir().unwrap();
    let uuid = "aaaa1111-bbbb-cccc-dddd-eeee22223333";
    write_cursor_transcript(cursor_home.path(), uuid);

    let conn = h.open();
    // A path read, a command run twice (re-read), and a command not in the transcript.
    seed_decision(&conn, uuid, "/x/a.rs", "Read");
    seed_decision(&conn, uuid, "git status", "Shell");
    seed_decision(&conn, uuid, "command that never appears", "Shell");

    let joined = ctx::surface::ingest::join_transcript_outcomes(&conn, cursor_home.path());
    assert_eq!(
        joined, 2,
        "only the two decisions present in the transcript join"
    );

    // The read happened before the "no, that's broken, revert" correction turn.
    let (j_read, corr_read, rr_read) = outcome(&conn, "/x/a.rs");
    assert_eq!(j_read, 1);
    assert_eq!(corr_read, 1, "a correction turn followed the read");
    assert_eq!(rr_read, 0, "the path was only read once");

    // git status was run twice: correction after the first, and a re-read.
    let (j_git, corr_git, rr_git) = outcome(&conn, "git status");
    assert_eq!(j_git, 1);
    assert_eq!(corr_git, 1);
    assert_eq!(rr_git, 1, "git status recurred later, so it is a re-read");

    // A command with no transcript tool call cannot be placed, so it stays unjoined.
    let j_missing: i64 = conn
        .query_row(
            "SELECT outcome_joined FROM compress_decisions WHERE command_or_path = ?1",
            ["command that never appears"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(j_missing, 0, "an unplaceable decision is never scored");
}

fn write_late_correction_transcript(home: &Path, uuid: &str) {
    let dir = home
        .join(".cursor/projects/Users-me-Projects-ctx/agent-transcripts")
        .join(uuid);
    std::fs::create_dir_all(&dir).unwrap();
    let big = "Working through the build and explaining each step in enough detail that this \
               assistant turn counts as substantial real work rather than a quick reply.";
    let lines = [
        r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>run the build and walk me through what each step is doing in detail</user_query>"}]}}"#.to_string(),
        r#"{"role":"assistant","message":{"content":[{"type":"text","text":"Running the build now."},{"type":"tool_use","name":"Shell","input":{"command":"make build"}}]}}"#.to_string(),
        r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>next</user_query>"}]}}"#.to_string(),
        format!(r#"{{"role":"assistant","message":{{"content":[{{"type":"text","text":"{big}"}}]}}}}"#),
        r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>ok</user_query>"}]}}"#.to_string(),
        format!(r#"{{"role":"assistant","message":{{"content":[{{"type":"text","text":"{big}"}}]}}}}"#),
        r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>no that's broken, revert</user_query>"}]}}"#.to_string(),
    ];
    std::fs::write(dir.join(format!("{uuid}.jsonl")), lines.join("\n")).unwrap();
}

#[test]
#[serial]
fn cursor_correction_beyond_turn_window_does_not_count() {
    let h = CtxHarness::new();
    let cursor_home = tempfile::tempdir().unwrap();
    let uuid = "bbbb2222-cccc-dddd-eeee-ffff33334444";
    write_late_correction_transcript(cursor_home.path(), uuid);

    let conn = h.open();
    // The tool call is at ordinal 1; the only correction is at ordinal 6, well past the
    // 3-turn window. Turns exist beyond the window, so the run joins as clean.
    seed_decision(&conn, uuid, "make build", "Shell");

    let joined = ctx::surface::ingest::join_transcript_outcomes(&conn, cursor_home.path());
    assert_eq!(joined, 1);

    let (j, corr, _rr) = outcome(&conn, "make build");
    assert_eq!(j, 1, "a turn past the window closes it, so the run joins");
    assert_eq!(
        corr, 0,
        "a correction many turns later is unrelated and must not count"
    );
}

#[test]
#[serial]
fn no_cursor_transcripts_is_a_noop() {
    let h = CtxHarness::new();
    let empty_home = tempfile::tempdir().unwrap();
    let conn = h.open();
    seed_decision(&conn, "some-uuid", "git status", "Shell");
    let joined = ctx::surface::ingest::join_transcript_outcomes(&conn, empty_home.path());
    assert_eq!(joined, 0, "no transcripts means nothing to join");
}
