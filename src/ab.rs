//! Per-feature A/B assignment for UserPromptSubmit gates.

use crate::config::AbTestConfig;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const AB_SALT: &str = "ctx-ab-v1";

/// Stable per-request key for coin flips (survives hook subprocess restarts).
pub fn request_key(session_id: Option<&str>, cwd: &str, prompt: &str) -> String {
    format!(
        "{}|{}|{}",
        session_id.unwrap_or(""),
        cwd,
        prompt
    )
}

/// Returns `true` for treatment (feature active), `false` for control (feature skipped).
pub fn ab_assign(pct: u8, feature: &str, request_key: &str) -> bool {
    if pct >= 100 {
        return true;
    }
    if pct == 0 {
        return false;
    }
    let mut h = DefaultHasher::new();
    AB_SALT.hash(&mut h);
    request_key.hash(&mut h);
    feature.hash(&mut h);
    (h.finish() % 100) < pct as u64
}

#[derive(Debug, Clone, Copy)]
pub struct AbAssignments {
    pub profile: bool,
    pub inject: bool,
    pub adaptive: bool,
    pub coaching: bool,
}

impl AbAssignments {
    pub fn from_config(ab: &AbTestConfig, request_key: &str) -> Self {
        Self {
            profile: ab_assign(ab.profile_pct, "profile", request_key),
            inject: ab_assign(ab.inject_pct, "inject", request_key),
            adaptive: ab_assign(ab.adaptive_pct, "adaptive", request_key),
            coaching: ab_assign(ab.coaching_pct, "coaching", request_key),
        }
    }

    /// All features at 100% — no experiment row metadata.
    pub fn experiment_active(ab: &AbTestConfig) -> bool {
        ab.profile_pct < 100
            || ab.inject_pct < 100
            || ab.adaptive_pct < 100
            || ab.coaching_pct < 100
    }

    /// Compact cohort label, e.g. `P:T I:C A:T C:T`. None when no experiment is running.
    pub fn format_group(ab: &AbTestConfig, a: &AbAssignments) -> Option<String> {
        if !Self::experiment_active(ab) {
            return None;
        }
        fn tc(on: bool) -> &'static str {
            if on {
                "T"
            } else {
                "C"
            }
        }
        Some(format!(
            "P:{} I:{} A:{} C:{}",
            tc(a.profile),
            tc(a.inject),
            tc(a.adaptive),
            tc(a.coaching)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ab_assign_zero_always_control() {
        for i in 0..50 {
            let key = format!("req-{i}");
            assert!(!ab_assign(0, "profile", &key));
        }
    }

    #[test]
    fn ab_assign_hundred_always_treatment() {
        for i in 0..50 {
            let key = format!("req-{i}");
            assert!(ab_assign(100, "profile", &key));
        }
    }

    #[test]
    fn ab_assign_fifty_roughly_balanced() {
        let n = 1000;
        let mut treatment = 0usize;
        for i in 0..n {
            let key = format!("req-{i}");
            if ab_assign(50, "profile", &key) {
                treatment += 1;
            }
        }
        assert!(
            treatment > 350 && treatment < 650,
            "expected ~50% treatment, got {treatment}/{n}"
        );
    }
}
