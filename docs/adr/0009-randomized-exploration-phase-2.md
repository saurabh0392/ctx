# 0009. Phase 2: randomized exploration for unbiased per-decision proof

- Status: accepted
- Date: 2026-06-12
- Deciders: Saurabh Sharan, ctx CTO partner
- Related: ADR 0007 (per-decision retention model), ADR 0008 (model selection policy), CTX-15

## Context

The per-decision model mirrors the heuristic on every decision it has seen (verified live: it would
trim the same 405 decisions, identical 3.2% correction and 14.3% re-read rates). That is not a model
weakness, it is a data weakness. Every outcome we have logged is confounded: we only ever observe
what happened when the rules *did* trim. We have almost no counterfactual, no "what would have
happened if we had kept it." Without that, no model can learn to safely decline a trim, and no proof
can show that declining was right.

The existing `causal_tool_outcomes` tries to fake a counterfactual by comparing shadow rows
(applied=0, but only because the preset was off or the tool was not activated) against applied rows.
That assignment is not random, so the comparison is biased: the decisions ctx happened to trim are
not exchangeable with the ones it happened to leave alone.

This is the central tension of a local-first, single-user product: causal proof needs a real
experiment, and the only traffic we have is the user's own.

## Decision

Run a real randomized experiment on the user's own trim-eligible decisions.

### Mechanism

- A decision is **eligible** when it would actually trim: the tool is trialing or activated, the
  read guard does not block it, and it would drop at least one line.
- On each eligible decision, draw `u ~ Uniform[0,1)`. With probability `compress_explore_rate`
  (default 0.20), assign **control**: withhold the trim and tag the row `explore_arm = "control"`.
  Otherwise assign **treatment**: trim as normal and tag `explore_arm = "treatment"`.
- Non-eligible decisions are untouched and untagged (`explore_arm = NULL`). A randomized control and
  an ordinary shadow row are both `applied = 0` but mean very different things, so the arm tag is a
  separate column, never inferred from `applied`.
- Outcomes (correction / re-read) attach through the normal label join. Per tool, the unbiased
  effect of trimming is `rate(treatment) - rate(control)`.

### Why this design

- **It only ever withholds a trim.** Exploration never trims more aggressively, so its only cost is
  forgone savings on the control fraction. There is no added risk to the user. This is what makes it
  acceptable to run on real sessions.
- **Eligibility gates the cost.** Where nothing would trim (preset off, no trial), nothing is
  explored, so a fresh install pays nothing until the user opts a tool into trimming.
- **Coarse first.** Per-tool proof accrues fastest, so we prove at the tool level before attempting
  repo × file-type slices, which on one user's volume may never reach significance.
- **Dependency-free randomness.** Assignment uses a SplitMix64 finalizer over the high-resolution
  clock, not the `rand` crate, to keep the single-binary install promise from ADR 0008.

### Rate

Set to **0.20** for the current single user, an explicit choice to trade more short-term savings for
faster proof. `compress_explore_rate = 0.0` disables exploration entirely. The default ships at 0.20
but is inert until a tool is actually trimming.

## What this is not

- Not a behavior change to how aggressively ctx trims. It can only hold trims back.
- Not yet the model steering anything. The model still does not touch `apply` (ADR 0007 Phase 3).
  Exploration produces the unbiased data; wiring the model to act on it is a later, separately gated
  step that requires per-tool proof first.
- Not a fine-grained per-repo proof yet. That is deferred until per-tool proof clears and volume
  justifies it.

## Consequences

- New `explore_arm` column on `compress_decisions`; new `explore_tool_outcomes` query computing the
  randomized control-vs-treatment counts.
- The dashboard's "road to Phase 2" becomes "Phase 2 is running": it shows exploration is on, how
  many control and treatment samples have accrued per tool, and the per-tool effect as it firms up.
- Honest limitation surfaced in the UI: on one user's traffic this is a slow burn, and we say so
  rather than implying fast results.
