//! Cross-surface transcript ingest: walk every registered transcript adapter, parse each
//! session, and back-fill outcome labels onto the shadow decisions the hook already
//! recorded (Phase 4).
//!
//! This is the counterpart to the timestamp-based [`crate::db::join_compress_outcomes`]
//! used for Claude Code. Surfaces like Cursor ship transcripts with **no timestamps**, so
//! the outcome of a decision cannot be found by "a later turn by clock time". Instead we
//! place the decision on the transcript timeline by matching its `command_or_path` to a
//! tool call's fingerprint, then look at later **ordinals** for a correction or abort.
//!
//! The two joins are disjoint by construction: Claude decisions resolve against a
//! `sessions` row (a path under `~/.claude`), Cursor decisions resolve against a transcript
//! UUID that never appears there. Running both is safe.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;

use super::cursor::CursorTranscript;
use super::{ParsedTranscript, SurfaceTranscriptAdapter, TurnFlag};

/// How many turns after a tool call a correction or abort still counts as caused by it.
/// The transcript ordinal analogue of [`crate::db::CORRECTION_WINDOW_MINUTES`]: a user
/// reacts in the next turn or two, not many turns later. Tight enough to separate an
/// immediate complaint from an unrelated short turn far down the session.
const CORRECTION_WINDOW_TURNS: u32 = 3;

/// The transcript adapters ingest knows about. Static for now; add Codex here when its
/// transcript shape is wired. Kept as trait objects so the walk is surface-agnostic.
fn transcript_adapters() -> Vec<Box<dyn SurfaceTranscriptAdapter>> {
    vec![Box::new(CursorTranscript)]
}

/// Walk every transcript adapter under `home`, parse each session, and join outcomes onto
/// the matching shadow decisions. Returns the number of decisions newly joined. Never
/// panics: an unreadable or unparseable transcript is skipped.
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
    // Ordinals where the user signalled a correction or aborted the turn. These are the
    // negative-outcome markers; a decision is "corrected" if any of them came after it.
    let mut correction_ordinals: Vec<u32> = parsed
        .turns
        .iter()
        .filter(|t| {
            t.flags
                .iter()
                .any(|f| matches!(f, TurnFlag::Correction | TurnFlag::Aborted))
        })
        .map(|t| t.ordinal)
        .collect();
    correction_ordinals.sort_unstable();

    // Per-flag ordinals, so the observation-only signal set can name which kind fired without
    // changing what `correction` (the only label the gate reads) counts.
    let explicit_ordinals = flag_ordinals(parsed, TurnFlag::CorrectionExplicit);
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
        // Place the decision on the timeline. If its tool call is not in the transcript,
        // we cannot assess an outcome honestly, so leave it unjoined.
        let Some(calls) = by_fingerprint.get(d.command_or_path.as_str()) else {
            continue;
        };
        let Some(call_ordinal) = calls.iter().map(|(o, _)| *o).min() else {
            continue;
        };
        let window_end = call_ordinal + CORRECTION_WINDOW_TURNS;
        let in_window = |o: u32| o > call_ordinal && o <= window_end;
        let correction = correction_ordinals.iter().any(|&o| in_window(o));
        // Only score once the label is final: a correction inside the window is a
        // permanent positive, or a turn beyond the window confirms a clean run. A tool
        // call at the tail of an in-progress session waits, same as the Claude path.
        let window_closed = max_ordinal > window_end;
        if !correction && !window_closed {
            continue;
        }
        let reread = calls.iter().any(|&(o, _)| in_window(o));
        let reedit = calls.iter().any(|&(o, is_edit)| is_edit && in_window(o));

        // Observation-only signal set (ADR 0019): names every signal that fired, for the audit.
        // Never read by the gate.
        let mut signals: Vec<&str> = Vec::new();
        if correction {
            signals.push("correction");
        }
        if explicit_ordinals.iter().any(|&o| in_window(o)) {
            signals.push("correction_explicit");
        }
        if aborted_ordinals.iter().any(|&o| in_window(o)) {
            signals.push("aborted");
        }
        if reread {
            signals.push("reread");
        }
        if reedit {
            signals.push("reedit");
        }
        let signals_json = serde_json::to_string(&signals).ok();

        if crate::db::set_decision_outcome(
            conn,
            d.id,
            correction,
            reread,
            // `reedit` is already a same-path edit within the window, the transcript analogue of the
            // timestamp join's edit-follow label (CTX-46 / ADR 0031).
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

    // Read a path, then edit the same path next turn: observation set records reread + reedit,
    // with no correction. The gate-facing correction label stays 0 (ADR 0019: observe, don't vote).
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
            // Window closes because a turn exists past call_ordinal + window.
            turns: vec![
                turn(1, TurnRole::Assistant, vec![]),
                turn(2, TurnRole::Assistant, vec![]),
                turn(6, TurnRole::User, vec![]),
            ],
            tool_calls: vec![
                call("/a.rs", "Read", 1),
                call("/a.rs", "Write", 2),
            ],
        };

        let newly = join_one(&conn, &parsed);
        assert_eq!(newly, 1, "the decision should join once the window closed");

        let (correction, edit_follow, signals): (i64, i64, String) = conn
            .query_row(
                "SELECT outcome_correction, COALESCE(outcome_edit_follow,0), COALESCE(outcome_signals,'') FROM compress_decisions WHERE command_or_path = '/a.rs'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(correction, 0, "no correction turn, so the gate label stays 0");
        assert_eq!(
            edit_follow, 1,
            "the same file was edited next turn, so edit-follow is set"
        );
        assert!(signals.contains("reread"), "signals: {signals}");
        assert!(signals.contains("reedit"), "signals: {signals}");
        assert!(
            !signals.contains("correction"),
            "no correction should be recorded: {signals}"
        );
    }
}
