//! Cross-turn dedup: skip re-sending identical tool output blocks.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn output_fingerprint(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.trim().hash(&mut h);
    h.finish()
}

/// If this exact output was compressed before in the session, return a short pointer.
pub fn duplicate_output_pointer(
    conn: &rusqlite::Connection,
    session_id: Option<&str>,
    text: &str,
    chars_in: usize,
) -> Option<String> {
    let sid = session_id.filter(|s| !s.is_empty())?;
    let fp = output_fingerprint(text);
    let prior_turn: Option<i64> = conn
        .query_row(
            "SELECT COUNT(*) FROM compress_output_fingerprints
             WHERE session_id = ?1 AND fingerprint = ?2",
            rusqlite::params![sid, fp as i64],
            |r| r.get(0),
        )
        .ok();
    if prior_turn.unwrap_or(0) > 0 {
        Some(format!(
            "[same tool output as a prior turn in this session, {chars_in} chars omitted; re-run with a narrower scope if needed]"
        ))
    } else {
        None
    }
}

pub fn record_output_fingerprint(
    conn: &rusqlite::Connection,
    session_id: Option<&str>,
    text: &str,
) {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return;
    };
    let fp = output_fingerprint(text) as i64;
    let ts = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "INSERT OR IGNORE INTO compress_output_fingerprints (session_id, fingerprint, first_ts)
         VALUES (?1, ?2, ?3)",
        rusqlite::params![sid, fp, ts],
    );
}

pub fn load_prior_line_hashes(
    conn: &rusqlite::Connection,
    session_id: Option<&str>,
) -> Vec<u64> {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT line_hash FROM compress_line_fingerprints WHERE session_id = ?1 LIMIT 500",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map(rusqlite::params![sid], |r| r.get::<_, i64>(0)) else {
        return Vec::new();
    };
    rows.filter_map(|r| r.ok().map(|h| h as u64)).collect()
}

pub fn record_line_fingerprints(
    conn: &rusqlite::Connection,
    session_id: Option<&str>,
    text: &str,
) {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return;
    };
    let ts = chrono::Utc::now().to_rfc3339();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let hash = super::retain::line_hash(line) as i64;
        let _ = conn.execute(
            "INSERT OR IGNORE INTO compress_line_fingerprints (session_id, line_hash, first_ts)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![sid, hash, ts],
        );
    }
}
