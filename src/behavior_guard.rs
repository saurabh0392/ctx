/// Behavior-pattern guard: fires a proactive system-prompt hint early in a session
/// when the user has a chronic pattern (high correction rate, frequent context resets).
///
/// Unlike coach.rs (which reacts to patterns already forming in the current request),
/// this module looks at the user's historical sessions from ~/.claude/projects/ and
/// injects a forward-looking hint at the start of a new session (turns 1-3).
/// No LLM calls -- rule-based only.
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

// Fire at most once per session (keyed on first-message hash)
static WARNED: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();

fn warned() -> &'static Mutex<HashSet<u64>> {
    WARNED.get_or_init(|| Mutex::new(HashSet::new()))
}

struct BehaviorProfile {
    correction_rate: f64, // fraction of turns that are corrections (0.0-1.0)
    compact_rate: f64,    // fraction of sessions that hit context limit
    opus_rate: f64,       // fraction of sessions using Opus (expensive)
    total_sessions: usize,
}

fn compute_behavior_profile() -> Option<BehaviorProfile> {
    let sessions = crate::conversations::all_sessions();
    if sessions.len() < 3 {
        return None; // not enough history to draw conclusions
    }

    let total = sessions.len();
    let total_turns: usize = sessions.iter().map(|s| s.turn_count).sum();
    if total_turns == 0 {
        return None;
    }

    let total_corrections: usize = sessions.iter().map(|s| s.correction_turns).sum();
    let compact_count = sessions.iter().filter(|s| s.hit_compact).count();
    let opus_count = sessions
        .iter()
        .filter(|s| s.models_used.iter().any(|m| m == "opus"))
        .count();

    Some(BehaviorProfile {
        correction_rate: total_corrections as f64 / total_turns as f64,
        compact_rate: compact_count as f64 / total as f64,
        opus_rate: opus_count as f64 / total as f64,
        total_sessions: total,
    })
}

fn session_key(messages: &[Value]) -> Option<u64> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let first = messages.first()?;
    let text = match first.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    };
    let mut h = DefaultHasher::new();
    text[..text.len().min(256)].hash(&mut h);
    Some(h.finish())
}

fn is_early_in_session(messages: &[Value]) -> bool {
    // Count user turns -- fire only on turns 1-3
    let user_turns = messages
        .iter()
        .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .count();
    user_turns <= 3
}

/// Check historical behavior patterns and return an injection hint if warranted.
/// Returns None if no pattern is strong enough or if we already warned this session.
pub fn check(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let messages = value.get("messages")?.as_array()?;
    if messages.is_empty() {
        return None;
    }

    if !is_early_in_session(messages) {
        return None;
    }

    let key = session_key(messages)?;
    {
        let mut warned = warned().lock().ok()?;
        if warned.contains(&key) {
            return None;
        }
        warned.insert(key);
    }

    let profile_hint = compute_behavior_profile()
        .as_ref()
        .and_then(hint_from_profile);
    let sim_hint = similar_history_hint(messages);
    match (profile_hint, sim_hint) {
        (None, None) => None,
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (Some(a), Some(b)) => Some(format!("{a}\n{b}")),
    }
}

fn first_user_text(messages: &[Value]) -> Option<String> {
    let first = messages.first()?;
    let text = match first.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    };
    Some(text)
}

fn similar_history_hint(messages: &[Value]) -> Option<String> {
    let text = first_user_text(messages)?;
    let Ok(conn) = crate::db::open_db() else {
        return None;
    };
    crate::db::ensure_schema(&conn).ok()?;
    let emb = crate::embedder::embed_text(&crate::embedder::compose_embed_text(
        &text.chars().take(1500).collect::<String>(),
        "",
        "",
        &[],
    ))
    .ok()?;
    let sims = crate::embedder::similar_sessions_by_query(&conn, &emb, 5, None).ok()?;
    if sims.is_empty() {
        return None;
    }
    let mut acc = 0f64;
    let mut n = 0usize;
    for (sid, _) in &sims {
        if let Ok((ct, tc)) = conn.query_row(
            "SELECT correction_turns, turn_count FROM sessions WHERE id = ?1",
            rusqlite::params![sid],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        ) {
            if tc > 0 {
                acc += ct as f64 / tc as f64;
                n += 1;
            }
        }
    }
    if n == 0 {
        return None;
    }
    let avg = acc / n as f64;
    if avg < 0.20 {
        return None;
    }
    Some(format!(
        "[ctx behavior insight] Past sessions that looked like this one averaged about {:.0}% correction rounds. Stating the output shape and constraints up front usually lowers rework.",
        avg * 100.0
    ))
}

fn hint_from_profile(profile: &BehaviorProfile) -> Option<String> {
    let mut hints: Vec<(f64, String)> = Vec::new();

    if profile.correction_rate > 0.20 {
        let pct = (profile.correction_rate * 100.0).round() as u32;
        hints.push((
            profile.correction_rate,
            format!(
                "[ctx behavior insight] Across your last {} sessions, about {}% of your messages \
             were corrections to Claude's previous response. To reduce this: start each task \
             with the desired output format and key constraints before describing the problem. \
             Fewer corrections mean faster results and lower token cost.",
                profile.total_sessions, pct
            ),
        ));
    }

    if profile.compact_rate > 0.30 {
        let pct = (profile.compact_rate * 100.0).round() as u32;
        hints.push((
            profile.compact_rate,
            format!(
                "[ctx behavior insight] {}% of your recent sessions ran out of working memory \
             mid-session (context reset). This forces Claude to lose prior context and \
             costs extra tokens to recover. Keep this session focused on one clear goal. \
             Use a new session for each distinct task.",
                pct
            ),
        ));
    }

    if profile.opus_rate > 0.40 && profile.total_sessions >= 5 {
        let pct = (profile.opus_rate * 100.0).round() as u32;
        hints.push((
            profile.opus_rate * 0.5,
            format!(
                "[ctx behavior insight] {}% of your recent sessions used Opus, which costs 5x \
             more than Sonnet. For standard engineering, writing, and analysis tasks, \
             Sonnet delivers equivalent results at a fraction of the cost.",
                pct
            ),
        ));
    }

    hints.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    hints.into_iter().next().map(|(_, hint)| hint)
}

/// Refresh `~/.ctx/behavior-hints.json` for the in-process filter (no per-session dedupe).
pub fn write_behavior_hints_file() -> anyhow::Result<()> {
    crate::config::ensure_dir()?;
    let path = crate::config::behavior_hints_path();
    let hint = compute_behavior_profile()
        .as_ref()
        .and_then(hint_from_profile);
    let v = serde_json::json!({
        "hint": hint,
        "generated_at": chrono::Utc::now().to_rfc3339(),
    });
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(&v)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msgs(pairs: &[(&str, &str)]) -> Vec<Value> {
        pairs
            .iter()
            .map(|(role, content)| serde_json::json!({"role": role, "content": content}))
            .collect()
    }

    fn body_bytes(messages: &[Value]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"messages": messages})).unwrap()
    }

    #[test]
    fn early_in_session_for_one_user_turn() {
        assert!(is_early_in_session(&msgs(&[("user", "hello")])));
    }

    #[test]
    fn early_in_session_for_three_user_turns() {
        let m = msgs(&[
            ("user", "q1"),
            ("assistant", "a1"),
            ("user", "q2"),
            ("assistant", "a2"),
            ("user", "q3"),
        ]);
        assert!(is_early_in_session(&m));
    }

    #[test]
    fn not_early_after_four_user_turns() {
        let m = msgs(&[
            ("user", "q1"),
            ("assistant", "a1"),
            ("user", "q2"),
            ("assistant", "a2"),
            ("user", "q3"),
            ("assistant", "a3"),
            ("user", "q4"),
        ]);
        assert!(!is_early_in_session(&m));
    }

    #[test]
    fn session_key_returns_some_for_valid_message() {
        let m = msgs(&[("user", "hello world")]);
        assert!(session_key(&m).is_some());
    }

    #[test]
    fn session_key_returns_none_for_empty_slice() {
        assert!(session_key(&[]).is_none());
    }

    #[test]
    fn session_key_is_stable_for_same_content() {
        let m1 = msgs(&[("user", "same message")]);
        let m2 = msgs(&[("user", "same message")]);
        assert_eq!(session_key(&m1), session_key(&m2));
    }

    #[test]
    fn check_returns_none_for_invalid_json() {
        assert!(check(b"not json").is_none());
    }

    #[test]
    fn check_returns_none_for_missing_messages_field() {
        let body = serde_json::to_vec(&serde_json::json!({"model": "test"})).unwrap();
        assert!(check(&body).is_none());
    }

    #[test]
    fn check_returns_none_for_empty_messages_array() {
        let body = serde_json::to_vec(&serde_json::json!({"messages": []})).unwrap();
        assert!(check(&body).is_none());
    }

    #[test]
    fn check_suppressed_after_turn_3() {
        // 4 user turns: is_early_in_session returns false, so check exits before all_sessions
        let m = msgs(&[
            ("user", "q1-bg-late-test"),
            ("assistant", "a1"),
            ("user", "q2-bg-late-test"),
            ("assistant", "a2"),
            ("user", "q3-bg-late-test"),
            ("assistant", "a3"),
            ("user", "q4-bg-late-test"),
        ]);
        assert!(check(&body_bytes(&m)).is_none());
    }

    #[test]
    fn check_deduplicates_same_session_key() {
        // Use a unique first message so we don't collide with WARNED state from other tests
        let m = msgs(&[("user", "unique-guard-dedup-xq9z7k-do-not-reuse")]);
        let body = body_bytes(&m);
        // First call inserts key into WARNED (may return None if no behavior profile)
        let _first = check(&body);
        // Second call must return None regardless of what first returned
        assert!(
            check(&body).is_none(),
            "dedup: second call must always return None"
        );
    }
}
