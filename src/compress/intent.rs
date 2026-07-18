//! Intent signal from the agent's narration (ADR 0004 / CTX-11).
//!
//! Claude Code persists the agent's turn in the session transcript
//! (`~/.claude/projects/**/*.jsonl`), and the PostToolUse hook payload hands us the path to that
//! transcript. The narration that precedes a tool call states the agent's *intent* for the call
//! ("Now update the references to MATCH_SPEED", "Let me add the transport handlers"), which is
//! exactly the signal trimming lacks: the static guard infers intent from the file path; the
//! narration states it.
//!
//! This module reads the tail of the transcript at decision time, lifts the most recent readable
//! narration, and reports whether it shows edit-intent for the file being read. The controller
//! uses this to protect a working read the static path guard would miss, and records the signal in
//! shadow features so we can measure how often intent is actually present before relying on it.
//!
//! Why narration and not "thinking" (CTX-11, measured 2026-06-09): we first tried the agent's
//! extended-thinking blocks. On `claude-opus-4-8` via Claude Code those are persisted
//! **signature-only**, the `thinking` text field is empty and only an encrypted `signature` is
//! stored (1294/1294 blocks across 8 recent sessions had no readable text). So the readable intent
//! on disk lives in assistant `text` blocks. This reader uses readable `text` blocks and also
//! readable `thinking` text when a model/surface does happen to store it, so it degrades gracefully
//! either way.
//!
//! Scope and honesty:
//! - Claude Code only. The Cursor adapter does not persist a transcript we extract intent from.
//! - Narration is noisier than raw reasoning: the agent narrates about many files, so
//!   `mentions_path` will over-fire. That is acceptable because the signal is purely *protective*,
//!   it only ever stops a trim, never causes one, so a false positive costs a little context, never
//!   correctness.
//! - We extract a small boolean signal, not the narration text itself. Reading is gated by the same
//!   privacy posture as the rest of the hook.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

/// How many bytes to read from the end of the transcript. We only need the last few turns;
/// reading the whole file on every PostToolUse would be wasteful on long sessions. Sized with
/// headroom because a single intervening tool result (an image read, a large file) can be tens of
/// KB, and the narration we want sits just before the current tool call.
const TAIL_BYTES: u64 = 256 * 1024;

/// How many of the most recent narration blocks to consider. The pre-call narration we care about
/// is usually the last block, but the agent sometimes splits it across a couple of messages.
const DEFAULT_MAX_BLOCKS: usize = 3;

/// Verbs that signal the agent intends to change a file rather than just consult it. Matched as
/// lowercase substrings; paired with a filename mention to keep precision reasonable.
const EDIT_VERBS: &[&str] = &[
    "edit",
    "modify",
    "change",
    "update",
    "rewrite",
    "refactor",
    "replace",
    "patch",
    "implement",
    "insert",
    "append",
    "fix the",
    "fix this",
    "add to",
    "remove from",
];

/// The edit-intent read of the agent's recent narration, relative to one file path. The three
/// components are recorded separately (not just the collapsed verdict) so prevalence is answerable:
/// `has_text` measures coverage (did the surface give us any readable narration), independent of
/// `mentions_path` and `has_edit_verb` which measure the intent rate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct IntentSignal {
    /// Readable narration was found in the recent transcript at all.
    pub has_text: bool,
    /// The recent narration names the file being read (by basename).
    pub mentions_path: bool,
    /// The recent narration uses an edit verb.
    pub has_edit_verb: bool,
}

impl IntentSignal {
    /// Build the intent read from a narration blob and the file under consideration.
    pub fn from_text(text: Option<&str>, file_path: Option<&str>) -> Self {
        let Some(text) = text else {
            return Self::default();
        };
        let lower = text.to_lowercase();
        let mentions_path = file_basename(file_path)
            .map(|base| lower.contains(&base))
            .unwrap_or(false);
        let has_edit_verb = EDIT_VERBS.iter().any(|v| lower.contains(v));
        Self {
            has_text: true,
            mentions_path,
            has_edit_verb,
        }
    }

    /// The agent's recent narration shows it is positioned to edit this file: it both names the
    /// file and expresses an intent to change something. This is the conservative signal the read
    /// guard acts on.
    pub fn edit_intent_for_path(&self) -> bool {
        self.has_text && self.mentions_path && self.has_edit_verb
    }
}

/// Lowercased basename of a path, e.g. `web/components/Foo.tsx` -> `foo.tsx`. Used so a relative
/// read path and an absolute mention in the narration still line up.
fn file_basename(file_path: Option<&str>) -> Option<String> {
    let p = file_path.map(str::trim).filter(|p| !p.is_empty())?;
    let normalized = p.replace('\\', "/");
    let base = normalized.rsplit('/').next().unwrap_or(&normalized);
    if base.is_empty() {
        return None;
    }
    Some(base.to_lowercase())
}

/// Read the most recent readable narration from a parsed transcript string. Collects assistant
/// `text` blocks and any readable `thinking` text (empty signature-only thinking is skipped),
/// returns the last `max_blocks` (oldest first) joined with newlines, or `None` when there is no
/// readable narration (a non-Claude surface, or a turn that emitted only tool calls).
pub fn recent_intent_text_from_transcript(content: &str, max_blocks: usize) -> Option<String> {
    let mut blocks: Vec<String> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // Claude Code rows are tagged `{"type":"assistant",...}`; Cursor rows are
        // `{"role":"assistant","message":{...}}`. Accept either so one reader serves both.
        let is_assistant = v.get("type").and_then(|t| t.as_str()) == Some("assistant")
            || v.get("role").and_then(|t| t.as_str()) == Some("assistant");
        if !is_assistant {
            continue;
        }
        let Some(arr) = v
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };
        for item in arr {
            let text = match item.get("type").and_then(|t| t.as_str()) {
                // Plain narration, always readable.
                Some("text") => item.get("text").and_then(|t| t.as_str()),
                // Reasoning: readable on some models, signature-only (empty) on others.
                Some("thinking") => item.get("thinking").and_then(|t| t.as_str()),
                _ => None,
            }
            .unwrap_or("")
            .trim();
            if !text.is_empty() {
                blocks.push(text.to_string());
            }
        }
    }
    if blocks.is_empty() {
        return None;
    }
    let start = blocks.len().saturating_sub(max_blocks.max(1));
    Some(blocks[start..].join("\n"))
}

/// Read the recent narration a PostToolUse payload points at. Tries the explicit
/// `transcript_path` first (Claude Code always supplies it; Cursor when present), then falls back
/// to deriving the Cursor transcript path from the session UUID and cwd. Returns `None` (never
/// errors out the hook) when nothing readable is found.
pub fn recent_intent_text_for_payload(payload: &Value) -> Option<String> {
    if let Some(text) = payload
        .get("transcript_path")
        .or_else(|| payload.get("transcriptPath"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .and_then(|p| read_tail(Path::new(p), TAIL_BYTES).ok())
        .and_then(|c| recent_intent_text_from_transcript(&c, DEFAULT_MAX_BLOCKS))
    {
        return Some(text);
    }
    // Cursor surface: no transcript_path in the payload, but the session UUID plus cwd locate the
    // transcript on disk the same way `surface::cursor` discovers sessions.
    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("sessionId"))
        .and_then(|v| v.as_str())?;
    let cwd = payload.get("cwd").and_then(|v| v.as_str())?;
    let path = cursor_transcript_path(session_id, cwd)?;
    let content = read_tail(&path, TAIL_BYTES).ok()?;
    recent_intent_text_from_transcript(&content, DEFAULT_MAX_BLOCKS)
}

/// Locate a Cursor agent transcript from its session UUID and cwd. Cursor encodes the workspace
/// path by dropping the leading slash and replacing `/` with `-`, then stores the main session
/// transcript at `~/.cursor/projects/<encoded>/agent-transcripts/<uuid>/<uuid>.jsonl`. Returns the
/// path only when the file exists.
fn cursor_transcript_path(session_id: &str, cwd: &str) -> Option<std::path::PathBuf> {
    if session_id.is_empty() || cwd.is_empty() {
        return None;
    }
    let encoded = cwd
        .trim_start_matches('/')
        .trim_end_matches('/')
        .replace('/', "-");
    if encoded.is_empty() {
        return None;
    }
    let path = crate::config::home_dir_for_paths()?
        .join(".cursor")
        .join("projects")
        .join(encoded)
        .join("agent-transcripts")
        .join(session_id)
        .join(format!("{session_id}.jsonl"));
    path.is_file().then_some(path)
}

/// Read up to `max_bytes` from the end of a file as lossy UTF-8. When the read started mid file,
/// the first (partial) line is dropped so JSONL parsing stays line-aligned.
fn read_tail(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        if let Some(nl) = text.find('\n') {
            return Ok(text[nl + 1..].to_string());
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: &str, body: &str) -> String {
        let field = if kind == "thinking" {
            "thinking"
        } else {
            "text"
        };
        format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"{kind}","{field}":{body}}}]}}}}"#
        )
    }

    #[test]
    fn extracts_last_narration_block_from_text() {
        let transcript = [
            line("text", r#""First I will read the helper.""#),
            line(
                "text",
                r#""Now update the render method in FullConversation.tsx.""#,
            ),
        ]
        .join("\n");
        let got = recent_intent_text_from_transcript(&transcript, 1).unwrap();
        assert!(got.contains("FullConversation.tsx"));
        assert!(!got.contains("First I will read"));
    }

    #[test]
    fn signature_only_thinking_is_skipped_but_text_is_kept() {
        // Mirrors Opus on Claude Code: thinking text is empty (signature-only), narration is in text.
        let transcript = [
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"","signature":"abc123"}]}}"#.to_string(),
            line("text", r#""Let me modify foo.rs now.""#),
        ]
        .join("\n");
        let got = recent_intent_text_from_transcript(&transcript, 3).unwrap();
        assert!(got.contains("modify foo.rs"));
    }

    #[test]
    fn no_readable_narration_returns_none() {
        let transcript = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"","signature":"sig"}]}}"#;
        assert!(recent_intent_text_from_transcript(transcript, 3).is_none());
    }

    #[test]
    fn parses_cursor_role_tagged_assistant_rows() {
        // Cursor rows use `role` not `type`, and interleave text with tool_use blocks.
        let transcript = [
            r#"{"role":"user","message":{"content":[{"type":"text","text":"fix the bug"}]}}"#.to_string(),
            r#"{"role":"assistant","message":{"content":[{"type":"text","text":"Let me edit src/agent.rs to add the guard."},{"type":"tool_use","name":"Read","input":{"path":"src/agent.rs"}}]}}"#.to_string(),
        ]
        .join("\n");
        let got =
            recent_intent_text_from_transcript(&transcript, 3).expect("cursor narration parsed");
        assert!(got.contains("edit src/agent.rs"));
        // The same narration drives the protective intent signal, surface-agnostic.
        let intent = IntentSignal::from_text(Some(&got), Some("src/agent.rs"));
        assert!(intent.edit_intent_for_path());
    }

    #[test]
    fn edit_intent_detected_when_file_named_with_edit_verb() {
        let intent = IntentSignal::from_text(
            Some("I need to modify the render method in FullConversation.tsx next."),
            Some("web/components/FullConversation.tsx"),
        );
        assert!(intent.has_text);
        assert!(intent.mentions_path);
        assert!(intent.has_edit_verb);
        assert!(intent.edit_intent_for_path());
    }

    #[test]
    fn reference_read_without_edit_verb_is_not_edit_intent() {
        let intent = IntentSignal::from_text(
            Some("Let me read FullConversation.tsx to understand how it renders."),
            Some("web/components/FullConversation.tsx"),
        );
        assert!(intent.mentions_path);
        assert!(!intent.has_edit_verb);
        assert!(!intent.edit_intent_for_path());
    }

    #[test]
    fn edit_verb_about_a_different_file_is_not_intent_for_this_path() {
        let intent = IntentSignal::from_text(
            Some("I will edit main.rs after I check this dependency."),
            Some("/Users/me/proj/vendor/dep/lib.rs"),
        );
        assert!(intent.has_edit_verb);
        assert!(!intent.mentions_path);
        assert!(!intent.edit_intent_for_path());
    }

    #[test]
    fn missing_narration_yields_empty_intent() {
        let intent = IntentSignal::from_text(None, Some("src/foo.rs"));
        assert_eq!(intent, IntentSignal::default());
        assert!(!intent.edit_intent_for_path());
    }

    #[test]
    fn tail_reader_drops_partial_first_line() {
        let dir = std::env::temp_dir().join(format!("ctx-intent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let mut content = String::from("PARTIAL-LINE-SHOULD-BE-DROPPED\n");
        content.push_str(&line("text", r#""edit foo.rs now.""#));
        content.push('\n');
        std::fs::write(&path, &content).unwrap();
        // Force a mid-file start so the partial first line is dropped.
        let tail = read_tail(&path, 40).unwrap();
        assert!(!tail.contains("PARTIAL-LINE"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
