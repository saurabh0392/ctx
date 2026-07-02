//! Cross-surface transcript ingest: walk every registered transcript adapter, parse each
//! session, and back-fill outcome labels onto the shadow decisions the hook already
//! recorded (Phase 4).

use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;

use super::cursor::CursorTranscript;
use super::{ParsedTranscript, SurfaceTranscriptAdapter, TurnFlag};

/// How many turns after a tool call a correction or abort still counts as caused by it.
const CORRECTION_WINDOW_TURNS: u32 = 3;

fn transcript_adapters() -> Vec<Box<dyn SurfaceTranscriptAdapter>> {
    vec![Box::new(CursorTranscript)]
}

/// Join outcomes for every transcript session discovered under `home`.
pub fn join_transcript_outcomes(conn: &Connection, home: &Path) -> usize {
    let mut newly = 0;
    for adapter in transcript_adapters() {
        for path in adapter.discover_sessions(home) {
            if let Some(parsed) = adapter.parse_session(&path) {
                newly += join_one(conn, &parsed);
            }
        }
    }
    newly
}

/// Ordinal/fingerprint outcome join for one parsed transcript. Surface-agnostic: it works
/// for any `ParsedTranscript` regardless of which adapter produced it.
fn join_one(conn: &Connection, parsed: &ParsedTranscript) -> usize {
    let explicit_ordinals = flag_ordinals(parsed, TurnFlag::CorrectionExplicit);
    let terse_ordinals = flag_ordinals(parsed, TurnFlag::CorrectionTerse);
    let aborted_ordinals = flag_ordinals(parsed, TurnFlag::Aborted);

    let Some(max_ordinal) = parsed.turns.iter().map(|t| t.ordinal).max() else {
        return 0;
    };

    // fingerprint -> every (ordinal, is_edit) it was called at. The earliest ordinal is the
    // decision's position; a later call inside the window is a re-read, and a later *edit* of
    // the same path is an immediate re-edit (ADR 0019).
    let mut by_fingerprint: HashMap<&str, Vec<(u32, bool)>> = HashMap::new();
    for call in &parsed.tool_calls {
        by_fingerprint
            .entry(call.input_fingerprint.as_str())
            .or_default()
            .push((
                call.turn_ordinal.unwrap_or(0),
                crate::outcome_signals::is_edit_tool(&call.tool_name),
            ));
    }

    let decisions = crate::db::unjoined_decisions_for_session(conn, &parsed.session.session_key);
    let mut newly = 0;
    for d in decisions {
        let Some(calls) = by_fingerprint.get(d.command_or_path.as_str()) else {
            continue;
        };
        let Some(call_ordinal) = calls.iter().map(|(o, _)| *o).min() else {
            continue;
        };
        let window_end = call_ordinal + CORRECTION_WINDOW_TURNS;
        let in_window = |o: u32| o > call_ordinal && o <= window_end;

        let explicit_in_window = explicit_gate_ordinals(parsed)
            .iter()
            .any(|&o| in_window(o));
        let terse_in_window = terse_ordinals.iter().any(|&o| in_window(o));
        let aborted_in_window = aborted_ordinals.iter().any(|&o| in_window(o));
        let observed_user_signal =
            explicit_in_window || terse_in_window || aborted_in_window;

        let window_closed = max_ordinal > window_end;
        let reread = calls.iter().any(|&(o, _)| in_window(o));
        let reedit = calls.iter().any(|&(o, is_edit)| is_edit && in_window(o));

        if !observed_user_signal && !window_closed && !reread {
            continue;
        }

        let gate_correction = crate::outcome_signals::gate_correction_label(
            explicit_in_window,
            d.applied,
            d.lines_drop,
        );

        let mut signals: Vec<&str> = Vec::new();
        if explicit_in_window {
            signals.push("correction_explicit");
        }
        if terse_in_window {
            signals.push("correction_terse");
        }
        if aborted_in_window {
            signals.push("aborted");
        }
        if gate_correction {
            signals.push("correction_gate");
        }
        if reread {
            signals.push("reread");
        }
        if reedit {
            signals.push("reedit");
        }
        if d.applied && d.lines_drop > 0 {
            signals.push("trimmed");
        }
        let signals_json = serde_json::to_string(&signals).ok();

        if crate::db::set_decision_outcome(
            conn,
            d.id,
            gate_correction,
            reread,
            reedit,
            parsed.session.surface.as_str(),
            signals_json.as_deref(),
        )
        .is_ok()
        {
            newly += 1;
        }
    }
    newly
}

/// Sorted ordinals of every turn carrying `flag`. Small helper so the signal set can report
/// each flag kind independently of the merged correction window.
fn flag_ordinals(parsed: &ParsedTranscript, flag: TurnFlag) -> Vec<u32> {
    let mut v: Vec<u32> = parsed
        .turns
        .iter()
        .filter(|t| t.flags.contains(&flag))
        .map(|t| t.ordinal)
        .collect();
    v.sort_unstable();
    v
}

/// Explicit complaint ordinals that can feed the causal gate. Agent log dumps can carry
/// complaint-like substrings ("sits wrong") without being user pushback, so long_dump is out.
fn explicit_gate_ordinals(parsed: &ParsedTranscript) -> Vec<u32> {
    let mut v: Vec<u32> = parsed
        .turns
        .iter()
        .filter(|t| {
            t.flags.contains(&TurnFlag::CorrectionExplicit)
                && !t.flags.contains(&TurnFlag::LongDump)
        })
        .map(|t| t.ordinal)
        .collect();
    v.sort_unstable();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::{CanonicalSession, CanonicalToolResult, CanonicalTurn, SurfaceId, TurnRole};

    fn turn(ordinal: u32, role: TurnRole, flags: Vec<TurnFlag>) -> CanonicalTurn {
        CanonicalTurn {
            ordinal,
            role,
            text_prefix: String::new(),
            flags,
            ts: None,
        }
    }

    fn call(fingerprint: &str, tool: &str, ordinal: u32) -> CanonicalToolResult {
        CanonicalToolResult {
            surface: SurfaceId::Cursor,
            session_key: "sess-1".into(),
            tool_name: tool.into(),
            input_fingerprint: fingerprint.into(),
            raw_text: String::new(),
            observed_at: None,
            turn_ordinal: Some(ordinal),
        }
    }

    #[test]
    fn join_records_reedit_signal_without_correction() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = crate::db::open_db().unwrap();
        crate::db::ensure_schema(&conn).unwrap();

        crate::db::insert_compress_decision(
            &conn,
            &crate::db::CompressDecision {
                ts: "2026-06-14T00:00:00+00:00",
                session_id: Some("sess-1"),
                tool_name: "Read",
                server_prefix: None,
                kind: "read",
                task_mode: "scan",
                lines_total: 100,
                lines_keep: 60,
                lines_drop: 40,
                chars_in: 5000,
                would_chars_out: 2000,
                features_json: "{}",
                command_or_path: "/a.rs",
                applied: false,
                explore_arm: None,
                surface: None,
            },
        )
        .unwrap();

        let parsed = ParsedTranscript {
            session: CanonicalSession {
                surface: SurfaceId::Cursor,
                session_key: "sess-1".into(),
                external_key: "sess-1".into(),
                project_label: "p".into(),
                repo_root: None,
            },
            turns: vec![
                turn(1, TurnRole::Assistant, vec![]),
                turn(2, TurnRole::Assistant, vec![]),
                turn(6, TurnRole::User, vec![]),
            ],
            tool_calls: vec![call("/a.rs", "Read", 1), call("/a.rs", "Write", 2)],
        };

        assert_eq!(join_one(&conn, &parsed), 1);
        let (corr, ef, sigs): (i64, i64, String) = conn
            .query_row(
                "SELECT outcome_correction, COALESCE(outcome_edit_follow,0), COALESCE(outcome_signals,'') FROM compress_decisions WHERE command_or_path = '/a.rs'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(corr, 0);
        assert_eq!(ef, 1);
        assert!(sigs.contains("reedit"));
    }

    #[test]
    fn aborted_turn_does_not_set_gate_correction_even_when_trimmed() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = crate::db::open_db().unwrap();
        crate::db::ensure_schema(&conn).unwrap();

        crate::db::insert_compress_decision(
            &conn,
            &crate::db::CompressDecision {
                ts: "2026-06-14T00:00:00+00:00",
                session_id: Some("sess-1"),
                tool_name: "Bash",
                server_prefix: None,
                kind: "generic",
                task_mode: "scan",
                lines_total: 100,
                lines_keep: 40,
                lines_drop: 60,
                chars_in: 5000,
                would_chars_out: 2000,
                features_json: "{}",
                command_or_path: "npm test",
                applied: true,
                explore_arm: None,
                surface: None,
            },
        )
        .unwrap();

        let parsed = ParsedTranscript {
            session: CanonicalSession {
                surface: SurfaceId::Cursor,
                session_key: "sess-1".into(),
                external_key: "sess-1".into(),
                project_label: "p".into(),
                repo_root: None,
            },
            turns: vec![
                turn(1, TurnRole::Assistant, vec![]),
                turn(2, TurnRole::User, vec![TurnFlag::Aborted]),
                turn(6, TurnRole::User, vec![]),
            ],
            tool_calls: vec![call("npm test", "Bash", 1)],
        };

        assert_eq!(join_one(&conn, &parsed), 1);
        let (corr, sigs): (i64, String) = conn
            .query_row(
                "SELECT outcome_correction, COALESCE(outcome_signals,'') FROM compress_decisions WHERE command_or_path = 'npm test'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(corr, 0);
        assert!(sigs.contains("aborted"));
        assert!(!sigs.contains("correction_gate"));
    }

    #[test]
    fn explicit_complaint_after_trim_sets_gate_correction() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let conn = crate::db::open_db().unwrap();
        crate::db::ensure_schema(&conn).unwrap();

        crate::db::insert_compress_decision(
            &conn,
            &crate::db::CompressDecision {
                ts: "2026-06-14T00:00:00+00:00",
                session_id: Some("sess-1"),
                tool_name: "Read",
                server_prefix: None,
                kind: "read",
                task_mode: "scan",
                lines_total: 100,
                lines_keep: 40,
                lines_drop: 60,
                chars_in: 5000,
                would_chars_out: 2000,
                features_json: "{}",
                command_or_path: "/a.rs",
                applied: true,
                explore_arm: None,
                surface: None,
            },
        )
        .unwrap();

        let parsed = ParsedTranscript {
            session: CanonicalSession {
                surface: SurfaceId::Cursor,
                session_key: "sess-1".into(),
                external_key: "sess-1".into(),
                project_label: "p".into(),
                repo_root: None,
            },
            turns: vec![
                turn(1, TurnRole::Assistant, vec![]),
                turn(
                    2,
                    TurnRole::User,
                    vec![TurnFlag::Correction, TurnFlag::CorrectionExplicit],
                ),
                turn(6, TurnRole::User, vec![]),
            ],
            tool_calls: vec![call("/a.rs", "Read", 1)],
        };

        assert_eq!(join_one(&conn, &parsed), 1);
        let corr: i64 = conn
            .query_row(
                "SELECT outcome_correction FROM compress_decisions WHERE command_or_path = '/a.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(corr, 1);
    }
}
