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
    /// below this for BOTH corrections and re-reads. The claim is "not measurably worse by more
    /// than this margin", not "provably better".
    ///
    /// Twenty runs per side is the minimum at which CTX evaluates the randomized comparison, not a
    /// promise that the interval will already be decisive. A clean but still-wide result keeps
    /// collecting. 0.10 is the maximum meaningful increase: a clearly harmful tool stays closed,
    /// while the interval naturally tightens as more comparable runs land.
    pub max_harm_delta: f64,
}

impl Default for CausalThresholds {
    fn default() -> Self {
        Self {
            min_baseline: 20,
            min_trimmed: 20,
            max_harm_delta: 0.10,
        }
    }
}

/// Entry sanity fuse for automatic burn-in (ADR 0012). Do not start trimming a tool whose
/// left-alone (baseline) correction rate is already this high or higher: that is a tool where the
/// output clearly matters, so it should not be trimmed on autopilot without a deliberate trial.
/// Re-reads are intentionally not fused here: a high baseline re-read rate reflects task
/// difficulty, and the causal gate compares the trimmed arm against that same baseline directly.
pub const BURN_IN_MAX_BASELINE_CORR: f64 = 0.25;

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
    tool_activated_on_surface(cfg, tool_name, _kind_label, "claude-code")
}

/// Surface-isolated activation check. Evidence from a different agent transport can never open
/// this gate, even when both surfaces normalize to the same tool name.
pub fn tool_activated_on_surface(
    cfg: &Config,
    tool_name: &str,
    _kind_label: &str,
    surface: &str,
) -> bool {
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
    let outcomes =
        crate::db::explore_tool_outcomes_for_surface(&conn, Some(tool_name), Some(surface));
    let Some(o) = outcomes.into_iter().find(|o| o.tool_name == tool_name) else {
        return false;
    };
    randomized_clears_bar(&o, &th)
}

/// Whether a tool should be auto-trialed (in burn-in) right now: it has a solid clean baseline but
/// no full trimmed arm yet, so it starts trimming to build the "after" arm the causal gate needs.
/// This is the autopilot on-ramp (ADR 0012 / CTX-23) that replaces the hand-written
/// `compress_trial_tools` list. The caller has already confirmed the preset allows the kind, so
/// burn-in never trims when autopilot is off. Off when `compress_auto_trial` is disabled.
pub fn tool_in_burn_in(cfg: &Config, tool_name: &str) -> bool {
    tool_in_burn_in_on_surface(cfg, tool_name, "claude-code")
}

/// Surface-isolated burn-in check; see [`tool_activated_on_surface`].
pub fn tool_in_burn_in_on_surface(cfg: &Config, tool_name: &str, surface: &str) -> bool {
    if !cfg.compress_enabled || !cfg.compress_auto_trial {
        return false;
    }
    let Ok(conn) = crate::db::open_db() else {
        return false;
    };
    if crate::db::ensure_schema(&conn).is_err() {
        return false;
    }
    let observational =
        crate::db::causal_tool_outcomes_for_surface(&conn, Some(tool_name), Some(surface));
    let Some(o) = observational.into_iter().find(|o| o.tool_name == tool_name) else {
        return false;
    };
    let th = CausalThresholds::default();
    if o.baseline_n < th.min_baseline {
        return false;
    }
    let corr_rate = o.baseline_corrections as f64 / o.baseline_n as f64;
    if corr_rate > BURN_IN_MAX_BASELINE_CORR {
        return false;
    }
    // Once the observational on-ramp is clean, keep the bounded experiment running until both
    // randomized arms—not the rough all-runs tally—have enough scored samples for the live gate.
    let experiment =
        crate::db::explore_tool_outcomes_for_surface(&conn, Some(tool_name), Some(surface));
    match experiment.into_iter().find(|e| e.tool_name == tool_name) {
        Some(e) if e.control_n < th.min_baseline || e.treatment_n < th.min_trimmed => true,
        Some(e) if randomized_clears_bar(&e, &th) => false,
        Some(e) => {
            let correction_delta = e.treatment_corrections as f64 / e.treatment_n as f64
                - e.control_corrections as f64 / e.control_n as f64;
            let retouch_delta = e.treatment_rereads as f64 / e.treatment_n as f64
                - e.control_rereads as f64 / e.control_n as f64;
            // Stop promptly on an observed harmful effect. Otherwise keep gathering randomized
            // evidence until the interval becomes decisive, with a finite ceiling.
            correction_delta <= th.max_harm_delta
                && retouch_delta <= th.max_harm_delta
                && (e.control_n < 200 || e.treatment_n < 200)
        }
        None => true,
    }
}

/// The live safety check over randomly assigned unchanged and trimmed runs. This is the same math
/// the dashboard API explains, so there is one decision source instead of a rough tally plus a
/// contradictory "clean test".
pub fn randomized_clears_bar(o: &crate::db::ExploreToolOutcome, th: &CausalThresholds) -> bool {
    if o.control_n < th.min_baseline || o.treatment_n < th.min_trimmed {
        return false;
    }
    let (_, _, correction_hi) = crate::stats::newcombe_diff(
        o.treatment_corrections,
        o.treatment_n,
        o.control_corrections,
        o.control_n,
    );
    let (_, _, retouch_hi) = crate::stats::newcombe_diff(
        o.treatment_rereads,
        o.treatment_n,
        o.control_rereads,
        o.control_n,
    );
    correction_hi <= th.max_harm_delta && retouch_hi <= th.max_harm_delta
}

/// Pure burn-in entry decision, extracted for tests. Start a bounded trial when there is enough
/// baseline evidence to justify it, the trimmed arm is not yet full (so the causal gate cannot
/// judge it yet), and the baseline correction rate is not pathological. Once the trimmed arm fills
/// (`trimmed_n >= min_trimmed`) this returns false and `causal_clears_bar` takes over: clean tools
/// stay trimming as earned, harmful tools stop.
pub fn burn_in_clears(o: &crate::db::CausalToolOutcome, th: &CausalThresholds) -> bool {
    if o.baseline_n < th.min_baseline {
        return false;
    }
    if o.trimmed_n >= th.min_trimmed {
        return false;
    }
    let corr_rate = o.baseline_corrections as f64 / o.baseline_n as f64;
    corr_rate <= BURN_IN_MAX_BASELINE_CORR
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

/// Where a tool stands on the watching -> learning -> earned path, with the honest distance to
/// the next threshold. Derived from the same `CausalToolOutcome` and `CausalThresholds` the live
/// gate uses, so the loop-health view (CTX-26) can never drift from what trimming actually does.
/// `held` and `blocked` are deliberately distinct from `watching`/`learning`: a tool that will
/// never auto-trial (baseline correction too high) or that filled its trimmed arm and failed the
/// harm bar must not read as "still collecting".
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum ToolStage {
    /// Not enough left-alone (baseline) runs yet to start testing.
    Watching { baseline_to_go: i64 },
    /// Baseline met and the tool is eligible to trim now; building the trimmed arm a verdict needs.
    Learning { trimmed_to_go: i64 },
    /// Baseline met but its left-alone correction rate is above the burn-in fuse, so autopilot
    /// will not trial it. The output clearly matters here; trimming waits for a deliberate trial.
    Held,
    /// Trimmed arm full and trimming proved no measurably worse than leaving it alone. Earned.
    Earned,
    /// Trimmed arm full but the harm interval is too high. Trimming is held back, not earned.
    Blocked,
}

/// Classify a tool's evidence into a [`ToolStage`]. Pure and shared so the gate, the status
/// label, and the loop-health view agree on where a tool stands. This reports the stage the
/// evidence supports; whether autopilot is actually trialing is a separate config fact the
/// caller surfaces alongside it.
pub fn tool_stage(o: &crate::db::CausalToolOutcome, th: &CausalThresholds) -> ToolStage {
    if o.trimmed_n >= th.min_trimmed {
        return if causal_clears_bar(o, th) {
            ToolStage::Earned
        } else {
            ToolStage::Blocked
        };
    }
    if o.baseline_n < th.min_baseline {
        return ToolStage::Watching {
            baseline_to_go: th.min_baseline - o.baseline_n,
        };
    }
    let baseline_corr_rate = if o.baseline_n > 0 {
        o.baseline_corrections as f64 / o.baseline_n as f64
    } else {
        0.0
    };
    if baseline_corr_rate > BURN_IN_MAX_BASELINE_CORR {
        return ToolStage::Held;
    }
    ToolStage::Learning {
        trimmed_to_go: th.min_trimmed - o.trimmed_n,
    }
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
            trimmed_collected: trimmed_n,
            baseline_collected: baseline_n,
            is_edit_tool: false,
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

    #[test]
    fn causal_gate_earns_a_clean_tool_at_realistic_volume() {
        // The calibration target: a tool that trimmed 37 times with zero corrections and zero
        // re-reads, against a low-harm baseline, must actually earn. At the old 0.05 margin this
        // failed forever; at the calibrated 0.10 margin it clears.
        let o = outcome(83, 2, 0, 37, 0, 0);
        assert!(causal_clears_bar(&o, &CausalThresholds::default()));
    }

    #[test]
    fn causal_gate_still_blocks_real_harm_at_calibrated_margin() {
        // A 40-point jump in corrections stays far above the margin and remains closed.
        let o = outcome(200, 4, 4, 200, 84, 4);
        assert!(!causal_clears_bar(&o, &CausalThresholds::default()));
    }

    #[test]
    fn burn_in_starts_with_clean_baseline_and_empty_after_arm() {
        // Solid baseline, no trimmed runs yet: this is exactly the tool that should auto-trial so
        // it can build an "after" arm. The chicken-and-egg ADR 0012 fixes.
        let o = outcome(60, 1, 9, 0, 0, 0);
        assert!(burn_in_clears(&o, &CausalThresholds::default()));
    }

    #[test]
    fn burn_in_waits_for_enough_baseline() {
        // Too little baseline evidence to justify trimming on autopilot yet.
        let o = outcome(10, 0, 0, 0, 0, 0);
        assert!(!burn_in_clears(&o, &CausalThresholds::default()));
    }

    #[test]
    fn burn_in_stops_once_after_arm_is_full() {
        // Trimmed arm is full: burn-in hands off to the causal gate, so it must no longer report
        // "in burn-in" (otherwise it would trim forever regardless of the gate verdict).
        let o = outcome(60, 0, 0, 30, 0, 0);
        assert!(!burn_in_clears(&o, &CausalThresholds::default()));
    }

    #[test]
    fn burn_in_fuse_blocks_pathological_baseline_corrections() {
        // Baseline correction rate above the fuse (here 40%): do not auto-trim a tool whose
        // output clearly matters. A deliberate trial is still allowed via compress_trial_tools.
        let o = outcome(50, 20, 0, 0, 0, 0);
        assert!(!burn_in_clears(&o, &CausalThresholds::default()));
    }

    #[test]
    fn burn_in_then_gate_handoff_is_continuous() {
        // A clean tool mid-burn-in (some trimmed runs, not yet full) keeps trimming via burn-in,
        // and the causal gate is not yet satisfied because the arm is below min_trimmed.
        let mid = outcome(60, 1, 2, 10, 0, 0);
        assert!(burn_in_clears(&mid, &CausalThresholds::default()));
        assert!(!causal_clears_bar(&mid, &CausalThresholds::default()));
        // Once the arm fills cleanly, burn-in stops and the gate earns it.
        let full = outcome(80, 1, 2, 80, 0, 0);
        assert!(!burn_in_clears(&full, &CausalThresholds::default()));
        assert!(causal_clears_bar(&full, &CausalThresholds::default()));
    }

    #[test]
    fn stage_watching_reports_baseline_distance() {
        // Below the baseline bar: still watching, with the honest count of left-alone runs to go.
        let o = outcome(12, 0, 0, 0, 0, 0);
        assert_eq!(
            tool_stage(&o, &CausalThresholds::default()),
            ToolStage::Watching { baseline_to_go: 8 }
        );
    }

    #[test]
    fn stage_learning_reports_trimmed_distance() {
        // Baseline met, clean, trimmed arm building: learning, with trimmed runs left to a verdict.
        let o = outcome(60, 1, 2, 8, 0, 0);
        assert_eq!(
            tool_stage(&o, &CausalThresholds::default()),
            ToolStage::Learning { trimmed_to_go: 12 }
        );
    }

    #[test]
    fn stage_held_when_baseline_corrections_too_high() {
        // Baseline met but correction rate above the burn-in fuse: autopilot will not trial it.
        let o = outcome(50, 20, 0, 0, 0, 0);
        assert_eq!(
            tool_stage(&o, &CausalThresholds::default()),
            ToolStage::Held
        );
    }

    #[test]
    fn stage_earned_when_full_arm_clears() {
        let o = outcome(83, 2, 0, 37, 0, 0);
        assert_eq!(
            tool_stage(&o, &CausalThresholds::default()),
            ToolStage::Earned
        );
    }

    #[test]
    fn stage_blocked_when_full_arm_fails_harm_bar() {
        let o = outcome(200, 4, 4, 200, 84, 4);
        assert_eq!(
            tool_stage(&o, &CausalThresholds::default()),
            ToolStage::Blocked
        );
    }
}
