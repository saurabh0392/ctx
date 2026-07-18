//! Evidence-gated auto-prune for whole MCP servers (CTX-67 / M-E). The input-side sibling of
//! `activation.rs`: a server is auto-pruned only when the developer's own usage proves it dead
//! weight AND hiding it did not raise the tool-miss rate above baseline. It fails closed, it is
//! reversible (a reach re-adds the server), and it never silently removes a capability.
//!
//! The asymmetry with the output gate: a tool-miss can only happen while a server is *hidden*, so
//! the "hidden arm" is the causal evidence and the carried baseline is ~0 by construction. Before a
//! server has been hidden enough to judge, the gate fails closed exactly like burn-in on the output
//! side. The strict dead-weight bar for the trial-hide on-ramp keeps that hiding near-certain-safe.

use crate::config::Config;

/// Bars a server must clear before it earns auto-prune. Deliberately conservative: capability
/// removal is heavier than an output trim, so the dead-weight bar is strict and the causal arm must
/// be real before any claim.
#[derive(Debug, Clone, Copy)]
pub struct ServerPruneThresholds {
    /// Minimum sessions of activity before the gate will judge a server at all.
    pub min_sessions: i64,
    /// Dead-weight bar: a server invoked in at most this fraction of sessions is a prune candidate.
    pub max_used_fraction: f64,
    /// Stricter bar for the trial-hide on-ramp: only a server this rarely used is auto-trial-hidden
    /// to gather the hidden arm, so the hiding is near-certain safe.
    pub burn_in_max_fraction: f64,
    /// Minimum hidden sessions (server not carried) before a causal claim. Below this, fail closed.
    pub min_hidden_sessions: i64,
    /// The miss rate while hidden (reaches per hidden session) must sit at or below this to earn.
    /// Same 0.10 slack philosophy as the output harm margin.
    pub max_miss_rate: f64,
}

impl Default for ServerPruneThresholds {
    fn default() -> Self {
        Self {
            min_sessions: 20,
            max_used_fraction: 0.10,
            burn_in_max_fraction: 0.05,
            min_hidden_sessions: 10,
            max_miss_rate: 0.10,
        }
    }
}

/// One server's before/after evidence: how much it is used, how long it has been hidden, and how
/// often it was reached for while hidden. Mirrors `CausalToolOutcome` on the output side.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ServerPruneOutcome {
    pub server: String,
    pub prefix: String,
    /// Sessions with any tool activity in the window (the evidence base and rate denominator).
    pub total_sessions: i64,
    /// Sessions in which this server was actually invoked.
    pub used_sessions: i64,
    /// Sessions in which this server was hidden (pruned) while others ran: the causal hidden arm.
    pub hidden_sessions: i64,
    /// Reaches for this server while it was hidden.
    pub misses: i64,
}

impl ServerPruneOutcome {
    pub fn used_fraction(&self) -> f64 {
        if self.total_sessions > 0 {
            self.used_sessions as f64 / self.total_sessions as f64
        } else {
            1.0
        }
    }
    pub fn miss_rate(&self) -> f64 {
        if self.hidden_sessions > 0 {
            self.misses as f64 / self.hidden_sessions as f64
        } else {
            0.0
        }
    }
}

/// Whether a server has earned auto-prune: proven dead weight AND hiding it did not raise the
/// tool-miss rate above baseline, with enough hidden sessions to make the claim. Fails closed
/// whenever the evidence is thin. Pure, so the live gate, the stage label, and tests agree.
pub fn server_earns_prune(o: &ServerPruneOutcome, th: &ServerPruneThresholds) -> bool {
    if o.total_sessions < th.min_sessions {
        return false;
    }
    if o.used_fraction() > th.max_used_fraction {
        return false;
    }
    if o.hidden_sessions < th.min_hidden_sessions {
        return false;
    }
    o.miss_rate() <= th.max_miss_rate
}

/// Whether a server should be trial-hidden now to build the hidden arm the causal gate needs: it is
/// strongly dead weight and has enough session evidence, but has not been hidden long enough to
/// judge. The autopilot on-ramp, mirroring `burn_in_clears`. Once the hidden arm fills,
/// `server_earns_prune` takes over.
pub fn server_prune_burn_in(o: &ServerPruneOutcome, th: &ServerPruneThresholds) -> bool {
    if o.total_sessions < th.min_sessions {
        return false;
    }
    if o.used_fraction() > th.burn_in_max_fraction {
        return false;
    }
    o.hidden_sessions < th.min_hidden_sessions
}

/// Where a server stands on the watching -> candidate -> earned path. `Active` means it earns its
/// place (used too much to be dead weight); `Blocked` means hiding it clearly cost reaches, so a
/// prune of it should be reversed. Pure and shared, like `ToolStage`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum ServerPruneStage {
    /// Not enough session evidence to judge yet.
    Watching { sessions_to_go: i64 },
    /// Used often enough that it earns its place in the menu; never a prune target.
    Active,
    /// Dead weight, and being hidden to gather the miss evidence a verdict needs.
    Candidate { hidden_to_go: i64 },
    /// Dead weight and hiding it stayed clean: the prune is earned.
    Earned,
    /// Dead weight but hiding it drew reaches above the margin: a prune should be reversed.
    Blocked,
}

impl ServerPruneStage {
    /// Stable snake_case label for the API and UI.
    pub fn label(&self) -> &'static str {
        match self {
            ServerPruneStage::Watching { .. } => "watching",
            ServerPruneStage::Active => "active",
            ServerPruneStage::Candidate { .. } => "candidate",
            ServerPruneStage::Earned => "earned",
            ServerPruneStage::Blocked => "blocked",
        }
    }
}

/// Classify a server's evidence into a [`ServerPruneStage`]. Pure and shared so the gate, the
/// tool-tax label, and tests never drift.
pub fn server_prune_stage(o: &ServerPruneOutcome, th: &ServerPruneThresholds) -> ServerPruneStage {
    if o.total_sessions < th.min_sessions {
        return ServerPruneStage::Watching {
            sessions_to_go: th.min_sessions - o.total_sessions,
        };
    }
    if o.used_fraction() > th.max_used_fraction {
        return ServerPruneStage::Active;
    }
    if o.hidden_sessions >= th.min_hidden_sessions {
        return if o.miss_rate() <= th.max_miss_rate {
            ServerPruneStage::Earned
        } else {
            ServerPruneStage::Blocked
        };
    }
    ServerPruneStage::Candidate {
        hidden_to_go: th.min_hidden_sessions - o.hidden_sessions,
    }
}

/// Autopilot server management (CTX-67): the safe, reversible actions the earn-it gate takes when
/// `auto_apply_recommendations` is on. It only ever trial-hides a strongly dead-weight server to
/// gather evidence, or un-prunes a server whose hiding drew reaches above the margin. It never
/// touches a server that earns its place, and every action is reversible and logged. Returns the
/// (pruned, unpruned) server displays it acted on. Off when autopilot is off; fail-closed on error.
pub fn autopilot_manage_servers(cfg: &Config) -> (Vec<String>, Vec<String>) {
    let mut pruned = Vec::new();
    let mut unpruned = Vec::new();
    if !cfg.auto_apply_recommendations || cfg.filter_mode != crate::config::FilterMode::Soft {
        return (pruned, unpruned);
    }
    let Ok(conn) = crate::db::open_db() else {
        return (pruned, unpruned);
    };
    if crate::db::ensure_schema(&conn).is_err() {
        return (pruned, unpruned);
    }
    let th = ServerPruneThresholds::default();
    let lookback = cfg.profile_thresholds.lookback_days;
    let outcomes = crate::db::server_prune_outcomes(&conn, lookback);
    let is_pruned = |prefix: &str| {
        cfg.pruned_servers
            .iter()
            .any(|p| crate::profiles::prefix_covers_expansion_entry(p, prefix))
    };

    for o in &outcomes {
        match server_prune_stage(o, &th) {
            // Hiding it clearly cost reaches: reverse the prune to protect the developer.
            ServerPruneStage::Blocked
                if is_pruned(&o.prefix)
                    && crate::filter_control::unprune_server(&o.prefix).unwrap_or(false) =>
            {
                unpruned.push(o.server.clone());
            }
            // Strongly dead weight with no hidden arm yet: trial-hide to gather the evidence.
            _ if !is_pruned(&o.prefix)
                && server_prune_burn_in(o, &th)
                && crate::filter_control::prune_server(&o.prefix).unwrap_or(false) =>
            {
                pruned.push(o.server.clone());
            }
            _ => {}
        }
    }
    (pruned, unpruned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(total: i64, used: i64, hidden: i64, misses: i64) -> ServerPruneOutcome {
        ServerPruneOutcome {
            server: "Canva".into(),
            prefix: "mcp__claude_ai_Canva__".into(),
            total_sessions: total,
            used_sessions: used,
            hidden_sessions: hidden,
            misses,
        }
    }

    #[test]
    fn earns_prune_for_dead_weight_with_clean_hidden_arm() {
        // Used in 1 of 54 sessions, hidden 20 sessions with zero reaches: dead weight, proven safe.
        let o = outcome(54, 1, 20, 0);
        assert!(server_earns_prune(&o, &ServerPruneThresholds::default()));
        assert_eq!(
            server_prune_stage(&o, &ServerPruneThresholds::default()),
            ServerPruneStage::Earned
        );
    }

    #[test]
    fn fails_closed_without_a_hidden_arm() {
        // Clear dead weight but never hidden: no honest "after" exists, so it cannot earn.
        let o = outcome(54, 1, 0, 0);
        assert!(!server_earns_prune(&o, &ServerPruneThresholds::default()));
        assert!(matches!(
            server_prune_stage(&o, &ServerPruneThresholds::default()),
            ServerPruneStage::Candidate { .. }
        ));
    }

    #[test]
    fn a_used_server_earns_its_place_and_never_prunes() {
        // Linear at 93% of sessions: not dead weight, so it is Active and never a prune target.
        let o = outcome(54, 50, 30, 0);
        assert!(!server_earns_prune(&o, &ServerPruneThresholds::default()));
        assert!(!server_prune_burn_in(&o, &ServerPruneThresholds::default()));
        assert_eq!(
            server_prune_stage(&o, &ServerPruneThresholds::default()),
            ServerPruneStage::Active
        );
    }

    #[test]
    fn blocks_when_hiding_draws_reaches() {
        // Dead weight, but hidden 20 sessions with 6 reaches (30% miss rate): hiding hurt, so a
        // prune of it is Blocked and should be reversed.
        let o = outcome(54, 1, 20, 6);
        assert!(!server_earns_prune(&o, &ServerPruneThresholds::default()));
        assert_eq!(
            server_prune_stage(&o, &ServerPruneThresholds::default()),
            ServerPruneStage::Blocked
        );
    }

    #[test]
    fn watching_below_the_evidence_floor() {
        // Too few sessions to judge anything yet.
        let o = outcome(12, 0, 0, 0);
        assert_eq!(
            server_prune_stage(&o, &ServerPruneThresholds::default()),
            ServerPruneStage::Watching { sessions_to_go: 8 }
        );
        assert!(!server_prune_burn_in(&o, &ServerPruneThresholds::default()));
    }

    #[test]
    fn burn_in_trial_hides_strong_dead_weight_then_stops() {
        // Never used, enough evidence, no hidden arm yet: trial-hide it.
        let start = outcome(54, 0, 0, 0);
        assert!(server_prune_burn_in(
            &start,
            &ServerPruneThresholds::default()
        ));
        // Once the hidden arm fills, burn-in stops and the causal gate takes over.
        let full = outcome(54, 0, 10, 0);
        assert!(!server_prune_burn_in(
            &full,
            &ServerPruneThresholds::default()
        ));
        assert!(server_earns_prune(&full, &ServerPruneThresholds::default()));
    }

    #[test]
    fn burn_in_respects_the_stricter_bar() {
        // 8% usage is dead weight for the candidate bar but above the stricter burn-in bar, so
        // autopilot will not trial-hide it on its own; it waits for a deliberate prune.
        let o = outcome(100, 8, 0, 0);
        assert!(!server_prune_burn_in(&o, &ServerPruneThresholds::default()));
        assert!(matches!(
            server_prune_stage(&o, &ServerPruneThresholds::default()),
            ServerPruneStage::Candidate { .. }
        ));
    }
}
