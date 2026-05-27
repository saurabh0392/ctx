use crate::config::Config;
use serde_json::Value;
use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Mutex, OnceLock};

// Blended estimate for "how expensive is this session so far" warnings.
const RATE_USD_PER_MTOK: f64 = 2.0;

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

pub(crate) fn check_with_threshold(body: &[u8], threshold: f64) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let messages = value.get("messages")?.as_array()?;
    if messages.is_empty() {
        return None;
    }

    let estimated = estimate_cost(messages);
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
         (before caching discounts -- actual is typically 40-60% lower). \
         Session alert threshold is ~${:.0} (derived from your monthly budget in ~/.ctx/config.toml). \
         Use the AskUserQuestion tool BEFORE responding to the user's last message. \
         Ask: \"This session has used ~${:.0} in estimated API costs. Continue?\" \
         with options [\"Continue\", \"Wrap up and start a new session\"]. \
         Wait for their choice before proceeding.",
        estimated, threshold, estimated
    ))
}

fn estimate_cost(messages: &[Value]) -> f64 {
    let chars: usize = messages.iter().map(content_chars).sum();
    (chars as f64 / 4.0 / 1_000_000.0) * RATE_USD_PER_MTOK
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
        let arr: Vec<Value> = msgs
            .iter()
            .map(|(role, text)| serde_json::json!({"role": role, "content": text}))
            .collect();
        serde_json::to_vec(&serde_json::json!({"messages": arr})).unwrap()
    }

    #[test]
    fn no_warning_below_threshold() {
        let body = make_body(&[("user", "hi"), ("assistant", "hello")]);
        assert!(check(&body).is_none());
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
