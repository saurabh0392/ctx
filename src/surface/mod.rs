//! Canonical, surface-agnostic corpus types.
//!
//! Every agent surface (Claude Code, Cursor, later Codex) normalizes its native wire
//! format into these types before anything touches the controller, the learned model,
//! or the outcome join. The brain never sees a surface-specific shape; it sees one
//! corpus keyed by repo and task, not by agent. This is the platform boundary: thin,
//! compile-time adapters in, one canonical model out.
//!
//! Phase 1 defines the vocabulary only. No schema migration and no behavior change ride
//! along here. Adapters (Phase 2 hook, Phase 3 Cursor transcript) and the shared ingest
//! join (Phase 4) build on these types.
//!
//! Timestamps are kept as RFC3339 strings, matching the SQLite `TEXT` columns
//! (`turns.ts`, `compress_decisions.ts`). Some surfaces (Cursor transcripts) have no
//! timestamps at all, so `ts` is optional and the join falls back to transcript order.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod cursor;
pub mod ingest;

/// Which agent produced a session. The learned model is keyed by repo and task, not by
/// this id; `surface` is provenance, used for honest per-surface reporting and for
/// picking the right adapter, never to fork the controller logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceId {
    ClaudeCode,
    Cursor,
    Codex,
}

impl SurfaceId {
    pub fn as_str(self) -> &'static str {
        match self {
            SurfaceId::ClaudeCode => "claude-code",
            SurfaceId::Cursor => "cursor",
            SurfaceId::Codex => "codex",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claude-code" | "claude_code" | "claudecode" => Some(SurfaceId::ClaudeCode),
            "cursor" => Some(SurfaceId::Cursor),
            "codex" => Some(SurfaceId::Codex),
            _ => None,
        }
    }
}

/// A turn's role, normalized across surfaces. Existing Claude ingest collapses an
/// exchange into a single stored row; new adapters should emit the real role so the
/// canonical timeline is faithful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TurnRole {
    User,
    Assistant,
    System,
}

impl TurnRole {
    pub fn as_str(self) -> &'static str {
        match self {
            TurnRole::User => "user",
            TurnRole::Assistant => "assistant",
            TurnRole::System => "system",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "user" => Some(TurnRole::User),
            "assistant" => Some(TurnRole::Assistant),
            "system" => Some(TurnRole::System),
            _ => None,
        }
    }
}

/// A normalized signal on a turn. These map 1:1 to the free-string flags already stored
/// in `turns.flags` so the canonical layer round-trips with existing data. `Aborted` is
/// new: it carries the Cursor `turn_ended` error / "User aborted request" signal, which
/// is the closest thing Cursor gives us to a dissatisfaction marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnFlag {
    Correction,
    /// High-confidence correction: the turn carried explicit complaint language. A subset
    /// of `Correction` (both are emitted together) so the join still matches on
    /// `%correction%` while the confidence tier stays recoverable from the flags column.
    CorrectionExplicit,
    Clarification,
    LongDump,
    PreCompact,
    Aborted,
}

impl TurnFlag {
    pub fn as_str(self) -> &'static str {
        match self {
            TurnFlag::Correction => "correction",
            TurnFlag::CorrectionExplicit => "correction_explicit",
            TurnFlag::Clarification => "clarification",
            TurnFlag::LongDump => "long_dump",
            TurnFlag::PreCompact => "pre_compact",
            TurnFlag::Aborted => "aborted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "correction" => Some(TurnFlag::Correction),
            "correction_explicit" => Some(TurnFlag::CorrectionExplicit),
            "clarification" => Some(TurnFlag::Clarification),
            "long_dump" => Some(TurnFlag::LongDump),
            "pre_compact" => Some(TurnFlag::PreCompact),
            "aborted" => Some(TurnFlag::Aborted),
            _ => None,
        }
    }
}

/// One agent session, normalized. `session_key` is the surface's own id (a UUID for both
/// Claude Code and Cursor) and is what `compress_decisions.session_id` records at hook
/// time. `external_key` is the durable storage path used as the `sessions.external_key`
/// (the join matches `external_key LIKE '%session_key%'`), so both are carried.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalSession {
    pub surface: SurfaceId,
    pub session_key: String,
    pub external_key: String,
    pub project_label: String,
    pub repo_root: Option<String>,
}

/// One turn on the canonical timeline. `ordinal` is the append order within the session;
/// it is the only temporal ground truth some surfaces give us, so the outcome join keys
/// off ordinal first and uses `ts` only as a refinement when present.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalTurn {
    pub ordinal: u32,
    pub role: TurnRole,
    pub text_prefix: String,
    pub flags: Vec<TurnFlag>,
    pub ts: Option<String>,
}

impl CanonicalTurn {
    pub fn has_flag(&self, flag: TurnFlag) -> bool {
        self.flags.contains(&flag)
    }

    /// Serialize flags to the JSON-array string shape stored in `turns.flags`
    /// (for example `["correction","aborted"]`), so adapters write compatible rows.
    pub fn flags_json(&self) -> String {
        let names: Vec<&str> = self.flags.iter().map(|f| f.as_str()).collect();
        serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string())
    }
}

/// One tool result lifted out of an agent's payload or transcript. `input_fingerprint`
/// is the stable join key (the command, the file path, or the tool name) and matches
/// what the hook stores in `compress_decisions.command_or_path`. `turn_ordinal` is filled
/// by ingest alignment when the result can be placed on the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalToolResult {
    pub surface: SurfaceId,
    pub session_key: String,
    pub tool_name: String,
    pub input_fingerprint: String,
    pub raw_text: String,
    pub observed_at: Option<String>,
    pub turn_ordinal: Option<u32>,
}

/// A whole transcript normalized: the session, its ordered turns, and the tool calls
/// placed on that timeline. Adapters return this; ingest persists what it needs and the
/// outcome join (Phase 4) walks the timeline by ordinal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedTranscript {
    pub session: CanonicalSession,
    pub turns: Vec<CanonicalTurn>,
    pub tool_calls: Vec<CanonicalToolResult>,
}

/// One agent surface that ships a durable transcript on disk (Cursor, later Codex).
/// This is the ingest-side counterpart to `crate::agent::AgentTransport` (the hook-side
/// adapter). Both feed the same canonical corpus; neither forks controller logic.
pub trait SurfaceTranscriptAdapter {
    fn surface_id(&self) -> SurfaceId;
    /// Locate every parseable session transcript under `home` for this surface.
    fn discover_sessions(&self, home: &Path) -> Vec<PathBuf>;
    /// Parse one transcript. Returns `None` (never panics) when the file is missing,
    /// unreadable, or not in a shape this adapter understands, so a bad file can never
    /// crash ingest.
    fn parse_session(&self, path: &Path) -> Option<ParsedTranscript>;
}

/// Build the stable join fingerprint for a tool call. This is the single source of truth
/// for how a tool result is keyed; both the hook adapter and the transcript adapters use
/// it so a hook-recorded decision and a transcript tool call line up. Mirrors the legacy
/// inline logic in the PostToolUse hook: prefer the shell command, then the file path,
/// then fall back to the tool name.
pub fn fingerprint_tool_input(tool_name: &str, tool_input: &Value) -> String {
    tool_input
        .get("command")
        .and_then(|v| v.as_str())
        .or_else(|| tool_input.get("file_path").and_then(|v| v.as_str()))
        .or_else(|| tool_input.get("path").and_then(|v| v.as_str()))
        .filter(|s| !s.is_empty())
        .unwrap_or(tool_name)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn surface_id_round_trips() {
        for s in [SurfaceId::ClaudeCode, SurfaceId::Cursor, SurfaceId::Codex] {
            assert_eq!(SurfaceId::parse(s.as_str()), Some(s));
        }
        assert_eq!(SurfaceId::parse("claude_code"), Some(SurfaceId::ClaudeCode));
        assert_eq!(SurfaceId::parse("nope"), None);
    }

    #[test]
    fn turn_flag_round_trips_with_stored_strings() {
        for f in [
            TurnFlag::Correction,
            TurnFlag::Clarification,
            TurnFlag::LongDump,
            TurnFlag::PreCompact,
            TurnFlag::Aborted,
        ] {
            assert_eq!(TurnFlag::parse(f.as_str()), Some(f));
        }
        // The "opus" model marker is not a normalized signal; it must not parse.
        assert_eq!(TurnFlag::parse("opus"), None);
    }

    #[test]
    fn flags_json_matches_stored_shape() {
        let t = CanonicalTurn {
            ordinal: 3,
            role: TurnRole::User,
            text_prefix: "do the thing".into(),
            flags: vec![TurnFlag::Correction, TurnFlag::Aborted],
            ts: None,
        };
        assert_eq!(t.flags_json(), r#"["correction","aborted"]"#);
        assert!(t.has_flag(TurnFlag::Correction));
        assert!(!t.has_flag(TurnFlag::LongDump));
    }

    #[test]
    fn fingerprint_prefers_command_then_path_then_tool() {
        assert_eq!(
            fingerprint_tool_input("Bash", &json!({"command": "git status"})),
            "git status"
        );
        assert_eq!(
            fingerprint_tool_input("Read", &json!({"file_path": "/a/b.rs"})),
            "/a/b.rs"
        );
        assert_eq!(fingerprint_tool_input("Grep", &json!({"path": "/c"})), "/c");
        assert_eq!(
            fingerprint_tool_input("ListMcpResources", &json!({})),
            "ListMcpResources"
        );
        // Empty command falls through to the tool name, never an empty key.
        assert_eq!(
            fingerprint_tool_input("Bash", &json!({"command": ""})),
            "Bash"
        );
    }
}
