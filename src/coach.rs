/// Failure-signal detection for in-context coaching.
///
/// Reads the `messages` array from an Anthropic request body and returns
/// a coaching suggestion when a recoverable failure pattern is detected.
/// No LLM calls -- purely rule-based over conversation structure.

const CORRECTION_PHRASES: &[&str] = &[
    "no,",
    "no that",
    "no -",
    "nope",
    "that's wrong",
    "thats wrong",
    "not that",
    "actually,",
    "wait,",
    "stop,",
    "that is wrong",
    "incorrect",
    "you misunderstood",
    "wrong,",
    "thats not right",
    "that's not right",
    "not what i",
    "not what I",
    "that's not what",
    "thats not what",
    "you're wrong",
    "youre wrong",
];

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could", "should", "may", "might",
    "shall", "can", "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into",
    "through", "during", "before", "after", "above", "below", "i", "you", "it", "this", "that",
    "they", "we", "he", "she", "my", "your", "its", "their", "our", "me", "him", "her", "us",
    "what", "how", "why", "when", "where", "which", "who", "just", "also", "now", "up", "out",
    "so", "if", "then", "please", "ok", "okay", "yes", "no", "not", "don't", "cant",
];

pub struct CoachSignal {
    pub kind: SignalKind,
    /// One-line suggestion to inject into the system prompt for the next request.
    pub suggestion: String,
}

pub enum SignalKind {
    /// User has sent a correction phrase twice or more in recent turns.
    CorrectionCascade,
    /// User appears to be re-asking a question already posed 2-4 turns ago.
    ReAsk,
}

/// Inspect the messages array from a parsed Anthropic request body.
/// Returns Some(CoachSignal) when a failure pattern is detected, None otherwise.
pub fn detect(messages: &[serde_json::Value]) -> Option<CoachSignal> {
    let user_texts: Vec<String> = messages
        .iter()
        .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .map(|m| extract_text(m))
        .collect();

    detect_correction_cascade(&user_texts).or_else(|| detect_reask(&user_texts))
}

/// Parse the `messages` array from a raw Anthropic request body and call detect().
pub fn detect_from_body(body: &[u8]) -> Option<CoachSignal> {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    let messages = value.get("messages")?.as_array()?;
    detect(messages)
}

/// User turn texts in chronological order (for example from session JSONL plus the in-flight prompt).
pub fn detect_from_user_texts(user_texts: &[String]) -> Option<CoachSignal> {
    detect_correction_cascade(user_texts).or_else(|| detect_reask(user_texts))
}

/// Five or more correction-heavy user turns in the last six (same phrase detection as the cascade).
/// Used to block the hook with a visible `reason` so the user starts a fresh session.
pub fn severe_correction_fatigue_reason(user_texts: &[String]) -> Option<String> {
    let window: Vec<&String> = user_texts.iter().rev().take(6).collect();
    let n = window.iter().filter(|t| has_correction_phrase(t)).count();
    if n >= 5 {
        Some(format!(
            "ctx: {n} turns in your last 6 user messages look like corrections or rejections. Start a fresh session or narrow the request scope before continuing."
        ))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Correction cascade: 2+ correction phrases in the last 6 user turns
// ---------------------------------------------------------------------------

fn detect_correction_cascade(user_texts: &[String]) -> Option<CoachSignal> {
    let window: Vec<&String> = user_texts.iter().rev().take(6).collect();
    let count = window.iter().filter(|t| has_correction_phrase(t)).count();

    if count >= 2 {
        Some(CoachSignal {
            kind: SignalKind::CorrectionCascade,
            suggestion: format!(
                "Note: {count} correction turns detected in this session. \
                 When the next user message arrives, respond by first stating \
                 the specific constraint you will now honor, then proceed. \
                 Do not re-attempt without acknowledging what was wrong."
            ),
        })
    } else {
        None
    }
}

fn has_correction_phrase(text: &str) -> bool {
    let lower = text.to_lowercase();
    CORRECTION_PHRASES.iter().any(|p| lower.contains(p))
}

// ---------------------------------------------------------------------------
// Re-ask: current user message has high keyword overlap with a message 2-4 turns ago
// ---------------------------------------------------------------------------

fn detect_reask(user_texts: &[String]) -> Option<CoachSignal> {
    if user_texts.len() < 3 {
        return None;
    }

    let current = user_texts.last()?;
    let current_words = keywords(current);
    if current_words.len() < 4 {
        return None; // too short to be meaningful
    }

    // Compare against turns 2 and 3 positions back (skip current and the immediately
    // prior turn because back-and-forth clarification is normal). Mirrors filter.js
    // which does slice(-4, -1).reverse() then skips index 0.
    let lookback: Vec<&String> = user_texts
        .iter()
        .rev()
        .skip(2) // skip current (0) + immediately prior (1)
        .take(2) // check positions 2 and 3 back only
        .collect();

    for prior in lookback {
        let prior_words = keywords(prior);
        if prior_words.len() < 4 {
            continue;
        }
        let sim = jaccard(&current_words, &prior_words);
        if sim >= 0.40 {
            return Some(CoachSignal {
                kind: SignalKind::ReAsk,
                suggestion: format!(
                    "Note: the user appears to be rephrasing a question from earlier \
                     (keyword overlap {:.0}%). Their first attempt may not have been \
                     answered fully. Address the original intent directly before \
                     elaborating.",
                    sim * 100.0
                ),
            });
        }
    }

    None
}

fn keywords(text: &str) -> std::collections::HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !STOP_WORDS.contains(w))
        .map(|w| w.to_string())
        .collect()
}

fn jaccard(a: &std::collections::HashSet<String>, b: &std::collections::HashSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let intersection = a.iter().filter(|w| b.contains(*w)).count();
    let union = a.len() + b.len() - intersection;
    intersection as f32 / union as f32
}

fn extract_text(message: &serde_json::Value) -> String {
    match message.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user(text: &str) -> serde_json::Value {
        json!({ "role": "user", "content": text })
    }

    fn assistant(text: &str) -> serde_json::Value {
        json!({ "role": "assistant", "content": text })
    }

    // --- correction cascade ---

    #[test]
    fn detect_from_user_texts_matches_cascade() {
        let texts = vec![
            "Refactor the auth module".to_string(),
            "No that's wrong, I meant the handler".to_string(),
            "Actually, still not right — I said auth, not session".to_string(),
        ];
        assert!(detect_from_user_texts(&texts).is_some());
    }

    #[test]
    fn severe_fatigue_fires_at_five_correction_turns_in_window() {
        let texts = vec![
            "please add a test".to_string(),
            "no that's wrong".to_string(),
            "actually, still wrong".to_string(),
            "wait, not what I asked".to_string(),
            "incorrect approach".to_string(),
            "you misunderstood the requirement".to_string(),
        ];
        assert!(severe_correction_fatigue_reason(&texts).is_some());
    }

    #[test]
    fn severe_fatigue_absent_below_five() {
        let texts = vec![
            "no that's wrong".to_string(),
            "actually wrong".to_string(),
            "wait no".to_string(),
            "looks good".to_string(),
        ];
        assert!(severe_correction_fatigue_reason(&texts).is_none());
    }

    #[test]
    fn correction_cascade_fires_at_two() {
        let messages = vec![
            user("Can you refactor the auth module?"),
            assistant("Here's the refactored version..."),
            user("No that's wrong, I meant the handler not the middleware"),
            assistant("Apologies, here is the handler..."),
            user("Actually, still not right — I said auth, not session"),
            assistant("Let me fix that..."),
            user("Now fix the tests too"),
        ];
        let texts: Vec<String> = messages
            .iter()
            .filter(|m| m["role"] == "user")
            .map(|m| m["content"].as_str().unwrap().to_string())
            .collect();
        assert!(detect_correction_cascade(&texts).is_some());
    }

    #[test]
    fn correction_cascade_does_not_fire_on_one() {
        let messages = vec![
            user("Refactor the auth module"),
            assistant("Here you go"),
            user("No that's wrong — I meant the handler"),
            assistant("Here is the handler"),
            user("Looks good, now add tests"),
        ];
        let texts: Vec<String> = messages
            .iter()
            .filter(|m| m["role"] == "user")
            .map(|m| m["content"].as_str().unwrap().to_string())
            .collect();
        assert!(detect_correction_cascade(&texts).is_none());
    }

    #[test]
    fn correction_cascade_does_not_fire_on_normal_conversation() {
        let texts = vec![
            "How do I implement rate limiting in the carrier adapter?".to_string(),
            "Can you show me the retry logic too?".to_string(),
            "What about the timeout configuration?".to_string(),
        ];
        assert!(detect_correction_cascade(&texts).is_none());
    }

    // --- re-ask detection ---

    #[test]
    fn reask_fires_on_high_overlap() {
        let texts = vec![
            "How does the carrier integration factory handle label generation errors?".to_string(),
            "Got it, what about retry logic?".to_string(),
            "Okay, and how about error handling in general?".to_string(),
            "Can you explain how the carrier integration factory handles errors for label generation?".to_string(),
        ];
        let result = detect_reask(&texts);
        assert!(
            result.is_some(),
            "expected reask signal for high-overlap rephrasing"
        );
    }

    #[test]
    fn reask_does_not_fire_on_normal_followup() {
        let texts = vec![
            "How do I configure the Deutsche Post adapter?".to_string(),
            "What environment variables does it need?".to_string(),
            "Where do I set those in staging?".to_string(),
        ];
        assert!(detect_reask(&texts).is_none());
    }

    #[test]
    fn reask_does_not_fire_with_too_few_turns() {
        let texts = vec![
            "What is the carrier name?".to_string(),
            "Where is it configured?".to_string(),
        ];
        assert!(detect_reask(&texts).is_none());
    }

    #[test]
    fn reask_skips_immediately_prior_turn() {
        // Turn 2 is very similar to turn 3 (normal follow-up), should not fire
        let texts = vec![
            "How does the label API work?".to_string(),
            "What about the label API response format?".to_string(),
            "What is the structure of the label API response object?".to_string(),
        ];
        // Current is turn 3, prior (skipped) is turn 2 -- only turns before that count
        // turn 1 has low overlap, so should not fire
        assert!(detect_reask(&texts).is_none());
    }

    // --- full body parsing ---

    #[test]
    fn detect_from_body_parses_correctly() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                user("Refactor the carrier adapter"),
                assistant("Done."),
                user("No that's wrong"),
                assistant("Sorry."),
                user("Actually still wrong — not what I asked"),
                assistant("My apologies."),
                user("Try again"),
            ]
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        assert!(detect_from_body(&bytes).is_some());
    }

    #[test]
    fn detect_from_body_returns_none_for_no_messages() {
        let body = json!({ "model": "claude-sonnet-4-6", "messages": [] });
        let bytes = serde_json::to_vec(&body).unwrap();
        assert!(detect_from_body(&bytes).is_none());
    }

    // --- jaccard helper ---

    #[test]
    fn jaccard_identical_sets() {
        let a: std::collections::HashSet<String> = ["carrier", "integration", "factory"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let b = a.clone();
        assert!((jaccard(&a, &b) - 1.0).abs() < 0.001);
    }

    #[test]
    fn jaccard_disjoint_sets() {
        let a: std::collections::HashSet<String> =
            ["foo", "bar"].iter().map(|s| s.to_string()).collect();
        let b: std::collections::HashSet<String> =
            ["baz", "qux"].iter().map(|s| s.to_string()).collect();
        assert!((jaccard(&a, &b) - 0.0).abs() < 0.001);
    }

    #[test]
    fn jaccard_partial_overlap() {
        let a: std::collections::HashSet<String> = ["carrier", "label", "error"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let b: std::collections::HashSet<String> = ["carrier", "label", "retry"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // intersection=2, union=4 => 0.5
        let sim = jaccard(&a, &b);
        assert!(sim > 0.49 && sim < 0.51, "expected ~0.5, got {sim}");
    }

    // --- content block format ---

    #[test]
    fn extract_text_handles_content_blocks() {
        let msg = json!({
            "role": "user",
            "content": [
                { "type": "text", "text": "Hello" },
                { "type": "tool_result", "content": "ignored" },
                { "type": "text", "text": " world" }
            ]
        });
        assert_eq!(extract_text(&msg), "Hello  world");
    }
}
