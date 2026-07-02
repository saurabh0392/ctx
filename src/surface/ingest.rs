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

/// What ctx has observed of one agent purely from its on-disk transcripts, independent of any
/// hook decision (CTX-53). This is how ctx shows a real second agent side by side with Claude
/// Code even when it has never trimmed that agent: the transcripts are genuine machine data, so
/// the surface card can report sessions and tool calls seen instead of a fake zero. Cursor
/// transcripts carry no timestamps, so `last_activity` is the newest transcript's file mtime, the
/// only honest "when" we have.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TranscriptSurfaceStats {
    pub surface: String,
    /// Transcripts ctx could parse for this surface.
    pub sessions: i64,
    /// Tool calls observed across those transcripts (calls, not results: Cursor omits output).
    pub tool_calls: i64,
    /// User turns flagged as a correction by the shared lexical guard.
    pub corrections: i64,
    /// Distinct project labels seen, so a card can say "across N repos".
    pub projects: i64,
    /// Newest transcript file mtime (RFC3339), or `None` when nothing parsed.
    pub last_activity: Option<String>,
}

/// Summarize the transcript corpus per surface, read-only over the files under `home`. Every
/// registered transcript adapter contributes one entry, even one that parses nothing, so callers
/// can distinguish "seen zero" from "adapter absent". Never touches the database.
pub fn transcript_corpus_summary(home: &Path) -> Vec<TranscriptSurfaceStats> {
    let mut out = Vec::new();
    for adapter in transcript_adapters() {
        let mut stats = TranscriptSurfaceStats {
            surface: adapter.surface_id().as_str().to_string(),
            ..Default::default()
        };
        let mut projects: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut newest: Option<std::time::SystemTime> = None;
        for path in adapter.discover_sessions(home) {
            if let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) {
                newest = Some(newest.map_or(modified, |n: std::time::SystemTime| n.max(modified)));
            }
            let Some(parsed) = adapter.parse_session(&path) else {
                continue;
            };
            stats.sessions += 1;
            stats.tool_calls += parsed.tool_calls.len() as i64;
            projects.insert(parsed.session.project_label.clone());
            for t in &parsed.turns {
                if t.flags.contains(&TurnFlag::Correction) {
                    stats.corrections += 1;
                }
            }
        }
        stats.projects = projects.len() as i64;
        stats.last_activity = newest
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
        out.push(stats);
    }
    out
}

/// Ordinal/fingerprint outcome join for one parsed transcript. Surface-agnostic: it works
/// for any `ParsedTranscript` regardless of which adapter produced it.
fn join_one(conn: &Connection, parsed: &ParsedTranscript) -> usize {
    let _explicit_ordinals = flag_ordinals(parsed, TurnFlag::CorrectionExplicit);
    let terse_ordinals = flag_ordinals(parsed, TurnFlag::CorrectionTerse);
    let aborted_ordinals = flag_ordinals(parsed, TurnFlag::Aborted);
    let steer_ordinals = flag_ordinals(parsed, TurnFlag::SessionSteer);

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
        let steer_in_window = steer_ordinals.iter().any(|&o| in_window(o));
        let observed_user_signal = explicit_in_window
            || terse_in_window
            || aborted_in_window
            || steer_in_window;

        let window_closed = max_ordinal > window_end;
        let reread = calls.iter().any(|&(o, _)| in_window(o));
        let reedit = calls.iter().any(|&(o, is_edit)| is_edit && in_window(o));

        let assistant_turns: Vec<(u32, &str)> = parsed
            .turns
            .iter()
            .filter(|t| matches!(t.role, crate::surface::TurnRole::Assistant))
            .map(|t| (t.ordinal, t.text_prefix.as_str()))
            .collect();
        let all_calls: Vec<(u32, &str, &str)> = parsed
            .tool_calls
            .iter()
            .filter_map(|c| c.turn_ordinal.map(|o| (o, c.tool_name.as_str(), c.input_fingerprint.as_str())))
            .collect();
        let compression_workaround = crate::outcome_signals::is_compression_workaround(
            d.applied,
            d.lines_drop,
            call_ordinal,
            &assistant_turns,
            &all_calls,
            CORRECTION_WINDOW_TURNS,
        );

        if !observed_user_signal && !window_closed && !reread && !compression_workaround {
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
        if steer_in_window {
            signals.push("session_steer");
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
        if compression_workaround {
            signals.push("compression_workaround");
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
                && !t.flags.contains(&TurnFlag::SessionSteer)
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

    #[test]
    fn transcript_corpus_summary_reports_real_cursor_sessions() {
        // A watched-only agent (Cursor here) must surface real sessions and tool calls from its
        // transcripts so it can render side by side with Claude Code (CTX-53), never a fake zero.
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp
            .path()
            .join(".cursor/projects/Users-me-Projects-ctx/agent-transcripts/sess-a");
        std::fs::create_dir_all(&proj).unwrap();
        let file = proj.join("sess-a.jsonl");
        let lines = [
            r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>build a cursor adapter and walk me through the tradeoffs carefully</user_query>"}]}}"#,
            r#"{"role":"assistant","message":{"content":[{"type":"text","text":"Reading the adapter boundary before proposing anything, so the plan lines up with the real ingest path."},{"type":"tool_use","name":"Read","input":{"path":"/Users/me/Projects/ctx/src/agent.rs"}}]}}"#,
            r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>no that's wrong, revert it</user_query>"}]}}"#,
        ];
        std::fs::write(&file, lines.join("\n")).unwrap();

        let out = transcript_corpus_summary(tmp.path());
        let cursor = out.iter().find(|s| s.surface == "cursor").expect("cursor entry");
        assert_eq!(cursor.sessions, 1);
        assert_eq!(cursor.tool_calls, 1);
        assert_eq!(cursor.corrections, 1);
        assert_eq!(cursor.projects, 1);
        assert!(cursor.last_activity.is_some());
    }

    #[test]
    fn transcript_corpus_summary_empty_home_is_zero_not_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let out = transcript_corpus_summary(tmp.path());
        // Every registered adapter still contributes an entry, so callers can tell "seen zero"
        // (adapter ran, found nothing) from "adapter absent".
        let cursor = out.iter().find(|s| s.surface == "cursor").expect("cursor entry");
        assert_eq!(cursor.sessions, 0);
        assert!(cursor.last_activity.is_none());
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

    #[test]
    fn session_steer_after_trim_does_not_set_gate_correction() {
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
                command_or_path: "figma metadata",
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
                turn(2, TurnRole::User, vec![TurnFlag::SessionSteer]),
                turn(6, TurnRole::User, vec![]),
            ],
            tool_calls: vec![call("figma metadata", "Bash", 1)],
        };

        assert_eq!(join_one(&conn, &parsed), 1);
        let (corr, sigs): (i64, String) = conn
            .query_row(
                "SELECT outcome_correction, COALESCE(outcome_signals,'') FROM compress_decisions WHERE command_or_path = 'figma metadata'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(corr, 0);
        assert!(sigs.contains("session_steer"));
        assert!(!sigs.contains("correction_gate"));
    }

    #[test]
    fn compression_workaround_records_on_trimmed_read_not_bash() {
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
                lines_total: 200,
                lines_keep: 40,
                lines_drop: 160,
                chars_in: 8000,
                would_chars_out: 2000,
                features_json: "{}",
                command_or_path: "/atlas.json",
                applied: true,
                explore_arm: None,
                surface: None,
            },
        )
        .unwrap();

        let mut narrate = turn(2, TurnRole::Assistant, vec![]);
        narrate.text_prefix = "Read output was trimmed; writing json via shell".into();
        let parsed = ParsedTranscript {
            session: CanonicalSession {
                surface: SurfaceId::Cursor,
                session_key: "sess-1".into(),
                external_key: "sess-1".into(),
                project_label: "p".into(),
                repo_root: None,
            },
            turns: vec![turn(1, TurnRole::Assistant, vec![]), narrate, turn(6, TurnRole::User, vec![])],
            tool_calls: vec![
                call("/atlas.json", "Read", 1),
                call("python3 -c 'open(\"x.json\")'", "Bash", 2),
            ],
        };

        assert_eq!(join_one(&conn, &parsed), 1);
        let sigs: String = conn
            .query_row(
                "SELECT COALESCE(outcome_signals,'') FROM compress_decisions WHERE command_or_path = '/atlas.json'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(sigs.contains("compression_workaround"));
    }
}
