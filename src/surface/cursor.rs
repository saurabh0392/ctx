//! Cursor agent-transcript adapter.
//!
//! Cursor stores each agent session as a JSONL transcript at
//! `~/.cursor/projects/<encoded-cwd>/agent-transcripts/<uuid>/<uuid>.jsonl`. Lines are
//! `{ "role": "user"|"assistant", "message": { "content": [ ... ] } }`, interleaved with
//! `{ "type": "turn_ended", "status": "success"|"error", "error": "..." }` markers.
//!
//! Unlike Claude Code JSONL, these transcripts carry **no timestamps and no per-turn cost
//! or tokens**, and tool *output* is not present (only the tool calls). So the canonical
//! timeline is built from append order (`ordinal`), and the only signals we can recover
//! are:
//!   - **Correction**: a short user turn following substantial assistant work. This is a
//!     deliberately low-confidence, fail-safe heuristic. Over-flagging a correction only
//!     keeps a tool in "watching" (activation fails closed), which is the safe direction;
//!     under-flagging would wrongly inflate "clean", so we bias toward flagging.
//!   - **Aborted**: a `turn_ended` error / "User aborted request", a strong dissatisfaction
//!     marker, attached to the turn it ended.
//!
//! The hook already records each tool result's features and the would-do decision keyed
//! by the Cursor session UUID; this adapter supplies the timeline that lets ingest join
//! an outcome to those decisions (Phase 4).

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{
    fingerprint_tool_input, CanonicalSession, CanonicalToolResult, CanonicalTurn, ParsedTranscript,
    SurfaceId, SurfaceTranscriptAdapter, TurnFlag, TurnRole,
};

/// The previous assistant turn must have produced at least this much text (or any tool
/// call) to count as "substantial" for the correction heuristic.
const SUBSTANTIAL_ASSISTANT_CHARS: usize = 120;

const TEXT_PREFIX_CAP: usize = 500;

pub struct CursorTranscript;

impl SurfaceTranscriptAdapter for CursorTranscript {
    fn surface_id(&self) -> SurfaceId {
        SurfaceId::Cursor
    }

    fn discover_sessions(&self, home: &Path) -> Vec<PathBuf> {
        let projects = home.join(".cursor").join("projects");
        let mut out = Vec::new();
        let Ok(project_dirs) = std::fs::read_dir(&projects) else {
            return out;
        };
        for proj in project_dirs.flatten() {
            let transcripts = proj.path().join("agent-transcripts");
            let Ok(session_dirs) = std::fs::read_dir(&transcripts) else {
                continue;
            };
            for sess in session_dirs.flatten() {
                let dir = sess.path();
                if !dir.is_dir() {
                    continue;
                }
                let Some(uuid) = dir.file_name().map(|n| n.to_string_lossy().to_string()) else {
                    continue;
                };
                // The main session transcript mirrors the directory name. Subagent
                // transcripts live under `subagents/` and are out of scope for now.
                let main = dir.join(format!("{uuid}.jsonl"));
                if main.is_file() {
                    out.push(main);
                }
            }
        }
        out
    }

    fn parse_session(&self, path: &Path) -> Option<ParsedTranscript> {
        let content = std::fs::read_to_string(path).ok()?;
        let uuid = path.file_stem()?.to_string_lossy().to_string();
        if uuid.is_empty() {
            return None;
        }

        let mut turns: Vec<CanonicalTurn> = Vec::new();
        let mut tool_calls: Vec<CanonicalToolResult> = Vec::new();
        let mut prev_assistant_substantial = false;
        let mut seen_user = false;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(row): Result<Value, _> = serde_json::from_str(line) else {
                // A single malformed line must not sink the whole session.
                continue;
            };

            // Marker rows: `turn_ended` with an abort/error annotates the last turn.
            if row.get("role").is_none() {
                if row.get("type").and_then(|v| v.as_str()) == Some("turn_ended") {
                    let status = row.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    let err = row.get("error").and_then(|v| v.as_str()).unwrap_or("");
                    let aborted = status == "error"
                        || err.to_lowercase().contains("abort")
                        || err.to_lowercase().contains("cancel");
                    if aborted {
                        if let Some(last) = turns.last_mut() {
                            if !last.flags.contains(&TurnFlag::Aborted) {
                                last.flags.push(TurnFlag::Aborted);
                            }
                        }
                    }
                }
                continue;
            }

            let role_str = row.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let content_blocks = row
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array());

            match role_str {
                "user" => {
                    let raw_text = concat_text(content_blocks);
                    let human = extract_human_text(&raw_text);
                    let ordinal = turns.len() as u32;
                    let mut flags = Vec::new();
                    // Correction: a short follow-up after the assistant did real work,
                    // passed through the shared lexical guard so approvals and continuations
                    // ("proceed", "phase 4", "looks good") are not mislabeled. The explicit
                    // tier (complaint language) is carried alongside for the confidence join.
                    if seen_user && prev_assistant_substantial {
                        match crate::outcome_signals::classify_correction(
                            &human,
                            crate::outcome_signals::DEFAULT_TERSE_MAX_CHARS,
                        ) {
                            crate::outcome_signals::CorrectionClass::Explicit => {
                                flags.push(TurnFlag::Correction);
                                flags.push(TurnFlag::CorrectionExplicit);
                            }
                            crate::outcome_signals::CorrectionClass::Terse => {
                                flags.push(TurnFlag::Correction);
                                flags.push(TurnFlag::CorrectionTerse);
                            }
                            crate::outcome_signals::CorrectionClass::Steer => {
                                flags.push(TurnFlag::SessionSteer);
                            }
                            crate::outcome_signals::CorrectionClass::None => {}
                        }
                    }
                    seen_user = true;
                    prev_assistant_substantial = false;
                    turns.push(CanonicalTurn {
                        ordinal,
                        role: TurnRole::User,
                        text_prefix: cap(&human),
                        flags,
                        ts: None,
                    });
                }
                "assistant" => {
                    let ordinal = turns.len() as u32;
                    let text = concat_text(content_blocks);
                    let mut had_tool = false;
                    if let Some(blocks) = content_blocks {
                        for b in blocks {
                            if b.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                                had_tool = true;
                                let name = b
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if name.is_empty() {
                                    continue;
                                }
                                let empty = Value::Object(Default::default());
                                let input = b.get("input").unwrap_or(&empty);
                                tool_calls.push(CanonicalToolResult {
                                    surface: SurfaceId::Cursor,
                                    session_key: uuid.clone(),
                                    tool_name: name.clone(),
                                    input_fingerprint: fingerprint_tool_input(&name, input),
                                    raw_text: String::new(),
                                    observed_at: None,
                                    turn_ordinal: Some(ordinal),
                                });
                            }
                        }
                    }
                    prev_assistant_substantial =
                        had_tool || text.trim().chars().count() >= SUBSTANTIAL_ASSISTANT_CHARS;
                    turns.push(CanonicalTurn {
                        ordinal,
                        role: TurnRole::Assistant,
                        text_prefix: cap(&text),
                        flags: Vec::new(),
                        ts: None,
                    });
                }
                _ => continue,
            }
        }

        if turns.is_empty() {
            return None;
        }

        let session = CanonicalSession {
            surface: SurfaceId::Cursor,
            session_key: uuid,
            external_key: path.to_string_lossy().to_string(),
            project_label: project_label_from_path(path),
            repo_root: None,
        };
        Some(ParsedTranscript {
            session,
            turns,
            tool_calls,
        })
    }
}

/// Concatenate the `text` of every text block in a content array.
fn concat_text(blocks: Option<&Vec<Value>>) -> String {
    let Some(blocks) = blocks else {
        return String::new();
    };
    let mut out = String::new();
    for b in blocks {
        if b.get("type").and_then(|v| v.as_str()) == Some("text") {
            if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(t);
            }
        }
    }
    out
}

/// Pull the human instruction out of a user message. Cursor wraps the real prompt in
/// `<user_query>...</user_query>` alongside system reminders and attachment notes; when
/// present we keep only that, otherwise we fall back to the whole text.
fn extract_human_text(raw: &str) -> String {
    if let Some(start) = raw.find("<user_query>") {
        let after = &raw[start + "<user_query>".len()..];
        if let Some(end) = after.find("</user_query>") {
            return after[..end].trim().to_string();
        }
        return after.trim().to_string();
    }
    raw.trim().to_string()
}

fn cap(s: &str) -> String {
    s.chars().take(TEXT_PREFIX_CAP).collect()
}

/// Derive a human project label from the transcript path. The project directory encodes
/// the workspace path with dashes (for example `Users-me-Projects-ctx`); take the segment
/// after `Projects`/`Documents`, else the last segment.
fn project_label_from_path(path: &Path) -> String {
    // .../projects/<encoded>/agent-transcripts/<uuid>/<uuid>.jsonl
    let encoded = path
        .ancestors()
        .nth(3)
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let parts: Vec<&str> = encoded.split('-').filter(|s| !s.is_empty()).collect();
    if let Some(idx) = parts
        .iter()
        .rposition(|&s| s == "Projects" || s == "Documents")
    {
        if idx + 1 < parts.len() {
            return parts[idx + 1..].join(" ");
        }
    }
    parts
        .last()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "cursor".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fixture(dir: &Path) -> PathBuf {
        let proj = dir.join(".cursor/projects/Users-me-Projects-ctx/agent-transcripts/sess-uuid-1");
        std::fs::create_dir_all(&proj).unwrap();
        let file = proj.join("sess-uuid-1.jsonl");
        let lines = [
            r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>\nbuild a plan to add a cursor adapter and explain the tradeoffs in detail so I understand the mechanics\n</user_query>"}]}}"#,
            r#"{"role":"assistant","message":{"content":[{"type":"text","text":"Exploring the codebase to understand the adapter boundary before proposing a plan, looking at agent.rs and the ingest path."},{"type":"tool_use","name":"Read","input":{"path":"/Users/me/Projects/ctx/src/agent.rs"}},{"type":"tool_use","name":"Shell","input":{"command":"git status"}}]}}"#,
            r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>no that's wrong, revert it</user_query>"}]}}"#,
            r#"{"role":"assistant","message":{"content":[{"type":"text","text":"Reverting the change and re-running the build to confirm a clean tree."},{"type":"tool_use","name":"Shell","input":{"command":"git status"}}]}}"#,
            r#"{"type":"turn_ended","status":"error","error":"User aborted request"}"#,
            r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>SGR v2</user_query>"}]}}"#,
        ];
        std::fs::write(&file, lines.join("\n")).unwrap();
        file
    }

    #[test]
    fn discovers_main_session_transcript() {
        let tmp = tempfile::tempdir().unwrap();
        let file = write_fixture(tmp.path());
        let found = CursorTranscript.discover_sessions(tmp.path());
        assert_eq!(found, vec![file]);
    }

    #[test]
    fn parses_turns_tool_calls_and_correction_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let file = write_fixture(tmp.path());
        let parsed = CursorTranscript.parse_session(&file).expect("parse");

        assert_eq!(parsed.session.session_key, "sess-uuid-1");
        assert_eq!(parsed.session.surface, SurfaceId::Cursor);
        assert_eq!(parsed.session.project_label, "ctx");

        // 3 user + 2 assistant turns.
        assert_eq!(parsed.turns.len(), 5);

        // The short "no that's wrong, revert it" after substantial assistant work flags,
        // and carries the high-confidence explicit tier (complaint language present).
        let correction = &parsed.turns[2];
        assert_eq!(correction.role, TurnRole::User);
        assert!(correction.has_flag(TurnFlag::Correction));
        assert!(correction.has_flag(TurnFlag::CorrectionExplicit));

        // The first user turn (the long task) must not be a correction.
        assert!(!parsed.turns[0].has_flag(TurnFlag::Correction));

        // "SGR v2" is short but follows an aborted assistant turn, not substantial work
        // (the abort marker does not make the preceding turn substantial); it still
        // follows real assistant work, so we accept it may flag. Assert the abort landed.
        let aborted_turn = &parsed.turns[3];
        assert!(aborted_turn.has_flag(TurnFlag::Aborted));

        // Tool calls fingerprinted and placed on the timeline.
        assert_eq!(parsed.tool_calls.len(), 3);
        let reads: Vec<&str> = parsed
            .tool_calls
            .iter()
            .map(|t| t.input_fingerprint.as_str())
            .collect();
        assert!(reads.contains(&"/Users/me/Projects/ctx/src/agent.rs"));
        assert!(reads.contains(&"git status"));
        // The first assistant turn is ordinal 1; its tool calls reference it.
        assert_eq!(parsed.tool_calls[0].turn_ordinal, Some(1));
    }

    #[test]
    fn malformed_lines_are_skipped_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join(".cursor/projects/p/agent-transcripts/s");
        std::fs::create_dir_all(&proj).unwrap();
        let file = proj.join("s.jsonl");
        std::fs::write(
            &file,
            "not json\n{\"role\":\"user\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n{bad",
        )
        .unwrap();
        let parsed = CursorTranscript
            .parse_session(&file)
            .expect("parse survives junk");
        assert_eq!(parsed.turns.len(), 1);
    }

    #[test]
    fn empty_or_missing_transcript_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(CursorTranscript
            .parse_session(&tmp.path().join("nope.jsonl"))
            .is_none());
        let empty = tmp.path().join("empty.jsonl");
        std::fs::write(&empty, "").unwrap();
        assert!(CursorTranscript.parse_session(&empty).is_none());
    }
}
