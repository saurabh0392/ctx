//! Cross-surface compaction hook normalization.
//!
//! A pre-compaction hook proves only that a platform started (or intended to start) compaction.
//! A post-compaction hook proves completion.  Keep those facts separate: Cursor currently exposes
//! only the former, while Claude Code and Codex expose both.  Hook payload text is never persisted;
//! it is used only to derive a retry-stable SHA-256 event key.

use std::io::Read;

use anyhow::Result;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionPhase {
    Attempted,
    Completed,
}

impl CompactionPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Attempted => "attempted",
            Self::Completed => "completed",
        }
    }

    pub fn hook_name(self) -> &'static str {
        match self {
            Self::Attempted => "PreCompact",
            Self::Completed => "PostCompact",
        }
    }
}

/// Command-hook entry point. Invalid JSON and unavailable local storage both fail open, because a
/// metrics hook must never interrupt the agent's own compaction.
pub fn record_stdin(surface: &str, phase: CompactionPhase) -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let payload = serde_json::from_str(input.trim()).unwrap_or_else(|_| json!({}));
    if let Ok(conn) = crate::db::open_db() {
        let _ = crate::db::ensure_schema(&conn);
        let _ = record_payload(&conn, surface, phase, &payload);
    }
    print!("{{}}");
    Ok(())
}

/// Persist a normalized metadata-only event. `false` means an identical hook delivery was already
/// seen. This is public within the crate so Cursor can retain its extra pressure metrics alongside
/// the shared ledger without inventing a second identity scheme.
pub(crate) fn record_payload(
    conn: &rusqlite::Connection,
    surface: &str,
    phase: CompactionPhase,
    payload: &Value,
) -> Result<bool> {
    let key = event_key(surface, phase, payload);
    let session_id = string_field(payload, &["session_id", "sessionId", "conversation_id"]);
    let turn_id = string_field(
        payload,
        &["turn_id", "turnId", "generation_id", "generationId"],
    );
    let trigger = string_field(payload, &["trigger", "compact_trigger", "source"]);
    let transaction = conn.unchecked_transaction()?;
    let _ = crate::db::claim_surface_hook_event(
        &transaction,
        &key,
        surface,
        phase.hook_name(),
        session_id,
        turn_id,
        None,
    )?;
    let inserted = crate::db::insert_native_compaction(
        &transaction,
        &key,
        surface,
        phase.as_str(),
        session_id,
        turn_id,
        trigger,
    )?;
    transaction.commit()?;
    Ok(inserted)
}

fn string_field<'a>(payload: &'a Value, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| payload.get(*name).and_then(Value::as_str))
        .filter(|value| !value.is_empty())
}

/// Build a deterministic delivery key without retaining prompt text or compact summaries. Known
/// sequence fields distinguish multiple compactions in one session. When a platform omits them,
/// transcript size/mtime provides a local monotonic discriminator; the final payload digest is a
/// last-resort retry key and is never reversible into the payload.
pub(crate) fn event_key(surface: &str, phase: CompactionPhase, payload: &Value) -> String {
    let mut identity = Map::new();
    for name in [
        "session_id",
        "sessionId",
        "conversation_id",
        "turn_id",
        "turnId",
        "generation_id",
        "generationId",
        "trigger",
        "compact_trigger",
        "source",
        "message_count",
        "messages_to_compact",
        "context_tokens",
        "is_first_compaction",
    ] {
        if let Some(value) = payload.get(name) {
            identity.insert(name.to_string(), value.clone());
        }
    }

    if let Some(path) = string_field(payload, &["transcript_path", "transcriptPath"]) {
        // The path itself is private and is therefore hashed with the rest rather than stored.
        identity.insert("transcript_path".into(), Value::String(path.to_string()));
        if let Ok(metadata) = std::fs::metadata(path) {
            identity.insert("transcript_len".into(), Value::from(metadata.len()));
        }
    }

    // PostCompact summaries and manual instructions can distinguish otherwise identical events.
    // Include them only inside the one-way digest.
    for name in ["compact_summary", "custom_instructions"] {
        if let Some(value) = payload.get(name) {
            identity.insert(name.to_string(), value.clone());
        }
    }
    if identity.is_empty() {
        identity.insert("payload".into(), payload.clone());
    }

    let mut hash = Sha256::new();
    hash.update(b"ctx-compaction-event-v2\0");
    hash.update(surface.as_bytes());
    hash.update([0]);
    hash.update(phase.as_str().as_bytes());
    hash.update([0]);
    hash.update(serde_json::to_vec(&Value::Object(identity)).unwrap_or_default());
    let digest = format!("{:x}", hash.finalize());
    format!("{surface}-compact-{}-{}", phase.as_str(), &digest[..24])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_have_the_same_key_but_phases_do_not() {
        let payload = json!({
            "session_id": "s1",
            "turn_id": "t9",
            "trigger": "auto",
            "compact_summary": "private summary"
        });
        let first = event_key("codex", CompactionPhase::Completed, &payload);
        assert_eq!(
            first,
            event_key("codex", CompactionPhase::Completed, &payload)
        );
        assert_ne!(
            first,
            event_key("codex", CompactionPhase::Attempted, &payload)
        );
        assert!(!first.contains("private"));
    }

    #[test]
    fn distinct_cursor_message_counts_are_distinct_attempts() {
        let a = json!({"conversation_id":"c1","trigger":"auto","message_count":100});
        let b = json!({"conversation_id":"c1","trigger":"auto","message_count":140});
        assert_ne!(
            event_key("cursor", CompactionPhase::Attempted, &a),
            event_key("cursor", CompactionPhase::Attempted, &b)
        );
    }

    #[test]
    fn one_attempt_and_completion_are_counted_once_each() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::ensure_schema(&conn).unwrap();
        let pre = json!({"session_id":"s1","turn_id":"t1","trigger":"manual"});
        let post =
            json!({"session_id":"s1","turn_id":"t1","trigger":"manual","compact_summary":"done"});
        assert!(record_payload(&conn, "claude-code", CompactionPhase::Attempted, &pre).unwrap());
        assert!(!record_payload(&conn, "claude-code", CompactionPhase::Attempted, &pre).unwrap());
        assert!(record_payload(&conn, "claude-code", CompactionPhase::Completed, &post).unwrap());
        assert!(!record_payload(&conn, "claude-code", CompactionPhase::Completed, &post).unwrap());
        let counts: (i64, i64) = conn
            .query_row(
                "SELECT SUM(phase='attempted'), SUM(phase='completed') FROM native_compactions",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (1, 1));
    }
}
