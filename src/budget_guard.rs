use crate::config::Config;
use serde_json::Value;
use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Mutex, OnceLock};

/// Per-model input pricing in USD/MTok (Anthropic list rates as of 2025).
/// Used for budget-warning estimates only — intentionally conservative (no cache discount).
fn rate_for_model(body: &Value) -> f64 {
    let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("");
    if model.starts_with("claude-opus") {
        15.0
    } else if model.starts_with("claude-haiku") {
        0.80
    } else {
        // Sonnet and anything unrecognised: use Sonnet input rate as the safe default.
        3.0
    }
}

static WARNED: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();

fn warned() -> &'static Mutex<HashSet<u64>> {
    WARNED.get_or_init(|| Mutex::new(HashSet::new()))
}

#[cfg(test)]
pub fn reset_warned_for_tests() {
    if let Ok(mut w) = warned().lock() {
        w.clear();
    }
}

/// Session-level alert threshold derived from `monthly_budget_usd` (rough pacing).
pub fn session_threshold_usd() -> f64 {
    let cfg = Config::load();
    cfg.monthly_budget_usd
        .filter(|m| *m > 0.0)
        .map(|m| (m / 20.0).clamp(5.0, 100.0))
        .unwrap_or(25.0)
}

/// Returns Some(injection text) the first time a session crosses the threshold.
pub fn check(body: &[u8]) -> Option<String> {
    check_with_threshold(body, session_threshold_usd())
}

/// Soft budget hint from recent user texts (hook path — no full API body).
pub fn soft_warning_for_user_texts(texts: &[String], model: Option<&str>) -> Option<String> {
    if texts.is_empty() {
        return None;
    }
    let model = model.unwrap_or("claude-sonnet-4-20250514");
    let messages: Vec<Value> = texts
        .iter()
        .map(|t| serde_json::json!({"role": "user", "content": t.as_str()}))
        .collect();
    let body = serde_json::to_vec(&serde_json::json!({"model": model, "messages": messages})).ok()?;
    check(&body)
}

/// Soft budget hint for UserPromptSubmit — prefers transcript messages when present.
pub fn soft_warning_for_hook_input(
    input: &Value,
    prompt: &str,
    fallback_texts: &[String],
    model: Option<&str>,
) -> Option<String> {
    let model = model.unwrap_or("claude-sonnet-4-20250514");
    if let Some(body) = hook_budget_body(input, prompt, model) {
        if let Some(w) = check(&body) {
            return Some(w);
        }
    }
    soft_warning_for_user_texts(fallback_texts, Some(model))
}

fn hook_budget_body(input: &Value, prompt: &str, model: &str) -> Option<Vec<u8>> {
    let mut messages: Vec<Value> = Vec::new();
    if let Some(transcript) = input.get("transcript") {
        if let Some(msgs) = transcript.get("messages").and_then(|m| m.as_array()) {
            messages.extend(msgs.iter().cloned());
        }
    }
    if messages.is_empty() {
        if let Some(msgs) = input.get("messages").and_then(|m| m.as_array()) {
            messages.extend(msgs.iter().cloned());
        }
    }
    if messages.is_empty() {
        return None;
    }
    let prompt_trim = prompt.trim();
    let last_user = messages.iter().rev().find(|m| {
        m.get("role").and_then(|r| r.as_str()) == Some("user")
    });
    let needs_append = match last_user {
        None => true,
        Some(m) => message_text(m).trim() != prompt_trim,
    };
    if needs_append && !prompt_trim.is_empty() {
        messages.push(serde_json::json!({"role": "user", "content": prompt}));
    }
    serde_json::to_vec(&serde_json::json!({"model": model, "messages": messages})).ok()
}

fn message_text(msg: &Value) -> String {
    match msg.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

pub(crate) fn check_with_threshold(body: &[u8], threshold: f64) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let messages = value.get("messages")?.as_array()?;
    if messages.is_empty() {
        return None;
    }

    let estimated = estimate_cost(&value);
    if estimated < threshold {
        return None;
    }

    let key = session_key(messages)?;

    let mut warned = warned().lock().ok()?;
    if warned.contains(&key) {
        return None;
    }
    warned.insert(key);

    Some(format!(
        "[ctx budget alert] This session has consumed an estimated ${:.0} in API costs \
         (before caching discounts -- actual is typically 40-60% lower; estimated at ${:.2}/MTok input). \
         Session alert threshold is ~${:.0} (derived from your monthly budget in ~/.ctx/config.toml). \
         Use the AskUserQuestion tool BEFORE responding to the user's last message. \
         Ask: \"This session has used ~${:.0} in estimated API costs. Continue?\" \
         with options [\"Continue\", \"Wrap up and start a new session\"]. \
         Wait for their choice before proceeding.",
        estimated, rate_for_model(&value), threshold, estimated
    ))
}

/// Conservative USD estimate for submitted prompt only (Sonnet-class rate).
pub fn estimated_usd_from_prompt_only(prompt: &str) -> f64 {
    let v = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": prompt}]
    });
    estimate_cost(&v)
}

/// When the prompt alone clears the session gate, return a user-facing block reason.
pub fn hard_block_reason_for_prompt(prompt: &str) -> Option<String> {
    let th = session_threshold_usd();
    let est = estimated_usd_from_prompt_only(prompt);
    if est < th {
        return None;
    }
    Some(format!(
        "Estimated input cost for this prompt (~${:.0}) meets the session gate (~${:.0}). Start a new session or raise monthly_budget_usd in ~/.ctx/config.toml.",
        est, th
    ))
}

fn estimate_cost(body: &Value) -> f64 {
    let messages = match body.get("messages").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => return 0.0,
    };
    let chars: usize = messages.iter().map(content_chars).sum();
    let rate = rate_for_model(body);
    (chars as f64 / 4.0 / 1_000_000.0) * rate
}

fn content_chars(msg: &Value) -> usize {
    match msg.get("content") {
        Some(Value::String(s)) => s.len(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .map(|b| b.get("text").and_then(|t| t.as_str()).map_or(0, str::len))
            .sum(),
        _ => 0,
    }
}

fn session_key(messages: &[Value]) -> Option<u64> {
    for msg in messages {
        if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }
        let text = match msg.get("content") {
            Some(Value::String(s)) => s.chars().take(500).collect::<String>(),
            Some(Value::Array(blocks)) => blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
                .chars()
                .take(500)
                .collect::<String>(),
            _ => continue,
        };
        if text.len() < 10 {
            continue;
        }
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        return Some(h.finish());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_body(msgs: &[(&str, &str)]) -> Vec<u8> {
        make_body_with_model(msgs, "claude-sonnet-4-6")
    }

    fn make_body_with_model(msgs: &[(&str, &str)], model: &str) -> Vec<u8> {
        let arr: Vec<Value> = msgs
            .iter()
            .map(|(role, text)| serde_json::json!({"role": role, "content": text}))
            .collect();
        serde_json::to_vec(&serde_json::json!({"model": model, "messages": arr})).unwrap()
    }

    #[test]
    fn no_warning_below_threshold() {
        let body = make_body(&[("user", "hi"), ("assistant", "hello")]);
        assert!(check(&body).is_none());
    }

    #[test]
    fn hard_block_on_massive_prompt() {
        let _g = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("CTX_HOME").ok();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let _ = crate::config::ensure_dir();
        let mut cfg = Config::load();
        cfg.monthly_budget_usd = Some(100.0);
        cfg.save().unwrap();
        let big = "z".repeat(35_000_000);
        assert!(hard_block_reason_for_prompt(&big).is_some());
        match prev {
            Some(v) => std::env::set_var("CTX_HOME", v),
            None => std::env::remove_var("CTX_HOME"),
        }
    }

    #[test]
    fn warning_fires_above_default_threshold() {
        let _g = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_warned_for_tests();
        let body = make_body(&[("user", "hello there x")]);
        let result = check_with_threshold(&body, -1.0);
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("AskUserQuestion"));
        assert!(text.contains("before caching discounts"));
    }

    #[test]
    fn opus_costs_more_than_haiku_for_same_content() {
        let content = "z".repeat(4_000_000); // 4M chars = ~1M tokens
        let opus_body = make_body_with_model(&[("user", &content)], "claude-opus-4-7");
        let haiku_body = make_body_with_model(&[("user", &content)], "claude-haiku-4-5");
        let opus_val: Value = serde_json::from_slice(&opus_body).unwrap();
        let haiku_val: Value = serde_json::from_slice(&haiku_body).unwrap();
        let opus_cost = estimate_cost(&opus_val);
        let haiku_cost = estimate_cost(&haiku_val);
        assert!(opus_cost > haiku_cost, "Opus ({opus_cost}) should cost more than Haiku ({haiku_cost})");
        // 15.0 / 0.80 = 18.75x ratio
        assert!(opus_cost / haiku_cost > 10.0, "Expected >10x ratio, got {}", opus_cost / haiku_cost);
    }

    #[test]
    fn unknown_model_defaults_to_sonnet_rate() {
        let body = make_body_with_model(&[("user", &"z".repeat(4_000_000))], "claude-unknown-model");
        let val: Value = serde_json::from_slice(&body).unwrap();
        let sonnet_body = make_body_with_model(&[("user", &"z".repeat(4_000_000))], "claude-sonnet-4-6");
        let sonnet_val: Value = serde_json::from_slice(&sonnet_body).unwrap();
        assert!((estimate_cost(&val) - estimate_cost(&sonnet_val)).abs() < 0.001);
    }

    #[test]
    fn hook_input_uses_transcript_messages_for_estimate() {
        let _g = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_warned_for_tests();
        let input = serde_json::json!({
            "prompt": "follow up",
            "transcript": {
                "messages": [
                    {"role": "user", "content": "z".repeat(6_000_000)},
                    {"role": "assistant", "content": "ok"}
                ]
            }
        });
        let body = super::hook_budget_body(&input, "follow up", "claude-sonnet-4-20250514")
            .expect("transcript should produce a budget body");
        let w = super::check_with_threshold(&body, 1.0);
        assert!(w.is_some(), "transcript-sized session should cross a low threshold");
    }

    #[test]
    fn lower_monthly_budget_lowers_threshold() {
        let _g = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_warned_for_tests();
        let prev = std::env::var("CTX_HOME").ok();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        let _ = crate::config::ensure_dir();
        let mut cfg = Config::load();
        cfg.monthly_budget_usd = Some(100.0);
        cfg.save().unwrap();
        let body = make_body(&[("user", "hello budget test body")]);
        let th = session_threshold_usd();
        assert!((th - 5.0).abs() < 0.01);
        let result = check_with_threshold(&body, th);
        assert!(result.is_none());
        let big = make_body(&[("user", &"z".repeat(11_000_000))]);
        let r2 = check_with_threshold(&big, th);
        assert!(r2.is_some());
        let text = r2.unwrap();
        assert!(text.contains("Session alert threshold"));
        match prev {
            Some(v) => std::env::set_var("CTX_HOME", v),
            None => std::env::remove_var("CTX_HOME"),
        }
    }
}
