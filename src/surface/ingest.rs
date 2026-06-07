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

    let Some(max_ordinal) = parsed.turns.iter().map(|t| t.ordinal).max() else {
        return 0;
    };

    // fingerprint -> every ordinal it was called at. The earliest is the decision's
    // position on the timeline; a second call inside the window is a re-read.
    let mut by_fingerprint: HashMap<&str, Vec<u32>> = HashMap::new();
    for call in &parsed.tool_calls {
        by_fingerprint
            .entry(call.input_fingerprint.as_str())
            .or_default()
            .push(call.turn_ordinal.unwrap_or(0));
    }

    let decisions = crate::db::unjoined_decisions_for_session(conn, &parsed.session.session_key);
    let mut newly = 0;
    for d in decisions {
        // Place the decision on the timeline. If its tool call is not in the transcript,
        // we cannot assess an outcome honestly, so leave it unjoined.
        let Some(ordinals) = by_fingerprint.get(d.command_or_path.as_str()) else {
            continue;
        };
        let Some(&call_ordinal) = ordinals.iter().min() else {
            continue;
        };
        let window_end = call_ordinal + CORRECTION_WINDOW_TURNS;
        let correction = correction_ordinals
            .iter()
            .any(|&o| o > call_ordinal && o <= window_end);
        // Only score once the label is final: a correction inside the window is a
        // permanent positive, or a turn beyond the window confirms a clean run. A tool
        // call at the tail of an in-progress session waits, same as the Claude path.
        let window_closed = max_ordinal > window_end;
        if !correction && !window_closed {
            continue;
        }
        let reread = ordinals
            .iter()
            .any(|&o| o > call_ordinal && o <= window_end);
        if crate::db::set_decision_outcome(conn, d.id, correction, reread, parsed.session.surface.as_str())
            .is_ok()
        {
            newly += 1;
        }
    }
    newly
}
