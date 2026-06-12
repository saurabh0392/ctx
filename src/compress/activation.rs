//! Evidence-gated activation (Act 1).
//!
//! A tool only turns from shadow to active once there is *causal* evidence that trimming it
//! does not make things worse for the user: enough baseline runs (ctx wanted to trim, left
//! it alone) AND enough trimmed runs (ctx actually cut it), with the trimmed correction and
//! re-read rates not measurably higher than baseline. This is the honest trust-rebuild after
//! compression was disabled. The preset (`compress_preset`) is the user's intent; this gate
//! is the evidence that intent is earned.
//!
//! The "after" arm (trimmed runs) only exists once a tool has been deliberately trialed
//! (`compress_trial_tools`). Before that the gate fails closed: with no trimmed data there is
//! no honest way to claim trimming is safe, so a tool can never silently auto-activate.

use crate::config::Config;

/// Causal bars a tool must clear before it earns auto-activation. Built on the same Wilson /
/// Newcombe math as `ctx context proof`, so the gate and the report always agree.
#[derive(Debug, Clone, Copy)]
pub struct CausalThresholds {
    /// Minimum would-trim runs left alone (baseline arm).
    pub min_baseline: i64,
    /// Minimum would-trim runs actually cut (trimmed arm). Without these the gate fails closed.
    pub min_trimmed: i64,
    /// The upper end of the 95% interval for (trimmed rate minus baseline rate) must sit at or
    /// below this for BOTH corrections and re-reads. A small positive slack absorbs noise; the
    /// claim is "not measurably worse", not "provably better".
    pub max_harm_delta: f64,
}

impl Default for CausalThresholds {
    fn default() -> Self {
        Self {
            min_baseline: 30,
            min_trimmed: 30,
            max_harm_delta: 0.05,
        }
    }
}

/// Legacy absolute bars, kept only for the training-readiness diagnostic in `learn.rs`
/// (PerToolGate). This is NOT the live trimming gate and never was the honest causal one.
#[derive(Debug, Clone, Copy)]
pub struct ActivationThresholds {
    pub min_joined: i64,
    pub max_correction_rate: f64,
    pub max_reread_rate: f64,
}

impl Default for ActivationThresholds {
    fn default() -> Self {
        Self {
            min_joined: 50,
            max_correction_rate: 0.02,
            max_reread_rate: 0.05,
        }
    }
}

/// Whether a tool has earned user-facing activation. The caller has already confirmed the
/// preset allows the kind. Deliberate trials (`compress_trial_tools`) trim through a separate
/// path in `agent::decide`; this gate is only about *auto*-activation from earned evidence.
///
/// `compress_force_active` bypasses the evidence bar for power users who explicitly opt into
/// aggressive trimming; otherwise the gate is causal and fails closed.
pub fn tool_activated(cfg: &Config, tool_name: &str, _kind_label: &str) -> bool {
    if cfg.compress_force_active {
        return true;
    }
    let Ok(conn) = crate::db::open_db() else {
        return false;
    };
    if crate::db::ensure_schema(&conn).is_err() {
        return false;
    }
    let th = CausalThresholds::default();
    let outcomes = crate::db::causal_tool_outcomes(&conn, Some(tool_name));
    let Some(o) = outcomes.into_iter().find(|o| o.tool_name == tool_name) else {
        return false;
    };
    causal_clears_bar(&o, &th)
}

/// Pure causal decision over a tool's before/after outcome. Extracted so the live gate, the
/// `ctx context status` label, and tests share one definition of "earned". Fails closed
/// whenever either arm is too small to make an honest claim.
pub fn causal_clears_bar(o: &crate::db::CausalToolOutcome, th: &CausalThresholds) -> bool {
    if o.baseline_n < th.min_baseline || o.trimmed_n < th.min_trimmed {
        return false;
    }
    let (_, _, corr_hi) = crate::stats::newcombe_diff(
        o.trimmed_corrections,
        o.trimmed_n,
        o.baseline_corrections,
        o.baseline_n,
    );
    let (_, _, rr_hi) = crate::stats::newcombe_diff(
        o.trimmed_rereads,
        o.trimmed_n,
        o.baseline_rereads,
        o.baseline_n,
    );
    corr_hi <= th.max_harm_delta && rr_hi <= th.max_harm_delta
}

/// Pure decision over a tool's collected progress. Extracted for tests and the Earning
/// dashboard view, which both need the same definition of "earned".
pub fn activation_clears_bar(
    p: &crate::db::CompressToolProgress,
    th: &ActivationThresholds,
) -> bool {
    if p.joined < th.min_joined {
        return false;
    }
    let joined = p.joined as f64;
    let correction_rate = p.corrections as f64 / joined;
    let reread_rate = p.rereads as f64 / joined;
    correction_rate <= th.max_correction_rate && reread_rate <= th.max_reread_rate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::CompressToolProgress;

    fn prog(joined: i64, corrections: i64, rereads: i64) -> CompressToolProgress {
        CompressToolProgress {
            tool_name: "Bash".into(),
            decisions: joined,
            joined,
            clean_runs: joined - corrections - rereads,
            corrections,
            rereads,
            active: false,
        }
    }

    #[test]
    fn not_enough_evidence_fails_closed() {
        assert!(!activation_clears_bar(
            &prog(10, 0, 0),
            &ActivationThresholds::default()
        ));
    }

    #[test]
    fn clean_high_volume_activates() {
        assert!(activation_clears_bar(
            &prog(200, 1, 2),
            &ActivationThresholds::default()
        ));
    }

    #[test]
    fn too_many_corrections_blocks() {
        assert!(!activation_clears_bar(
            &prog(200, 50, 0),
            &ActivationThresholds::default()
        ));
    }

    fn outcome(
        baseline_n: i64,
        baseline_corr: i64,
        baseline_rr: i64,
        trimmed_n: i64,
        trimmed_corr: i64,
        trimmed_rr: i64,
    ) -> crate::db::CausalToolOutcome {
        crate::db::CausalToolOutcome {
            tool_name: "Read".into(),
            baseline_n,
            baseline_corrections: baseline_corr,
            baseline_rereads: baseline_rr,
            trimmed_n,
            trimmed_corrections: trimmed_corr,
            trimmed_rereads: trimmed_rr,
        }
    }

    #[test]
    fn causal_gate_fails_closed_without_trimmed_data() {
        // Plenty of clean baseline, zero trimmed runs: no honest "after" exists yet.
        let o = outcome(500, 0, 0, 0, 0, 0);
        assert!(!causal_clears_bar(&o, &CausalThresholds::default()));
    }

    #[test]
    fn causal_gate_blocks_when_trimming_clearly_worse() {
        // Baseline near zero harm, trimmed badly worse: delta upper bound well above slack.
        let o = outcome(200, 2, 2, 200, 80, 80);
        assert!(!causal_clears_bar(&o, &CausalThresholds::default()));
    }

    #[test]
    fn causal_gate_activates_when_trimming_not_worse() {
        // Both arms large and similarly low: trimmed is not measurably worse than baseline.
        let o = outcome(400, 4, 4, 400, 4, 4);
        assert!(causal_clears_bar(&o, &CausalThresholds::default()));
    }

    #[test]
    fn causal_gate_needs_minimum_trimmed_volume() {
        // Clean but tiny trimmed arm stays closed: too few runs to claim safety.
        let o = outcome(400, 0, 0, 5, 0, 0);
        assert!(!causal_clears_bar(&o, &CausalThresholds::default()));
    }
}
