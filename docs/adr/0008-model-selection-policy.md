# 0008. Model selection policy: proof is the selector, local-first keeps it simple

- Status: accepted
- Date: 2026-06-12
- Deciders: Saurabh Sharan, ctx CTO partner
- Related: ADR 0007 (per-decision retention model), CTX-15

## Context

Two questions came up while wiring the retention model in:

1. Why logistic regression and not KNN / nearest neighbour / something fancier?
2. Why not "smart model selection by task", picking the best model per tool/repo/context?

Both are reasonable instincts. The answers are shaped by one fact that is easy to forget: ctx runs
entirely on the user's own machine. There is no server, no fleet to monitor, no cross-user data
pooling. The model is trained and served locally, inside a hook that fires synchronously on every
tool call.

A concrete finding made this urgent. After adding tool-kind features and retraining (holdout AUC
0.629), `ctx bench run` still showed the learned arm identical to the heuristic arm (same n, same
correction and re-read rates). The model says "safe" on nearly everything because the base
correction rate is ~2.6%, so it almost never crosses the fixed 0.15 act threshold. That is a
base-rate and threshold problem, not a model-family problem, and no amount of swapping families
fixes it.

## Decision

### Model family: logistic regression now, a small tree later, never KNN as the policy

Keep the hand-rolled L2 logistic regression as the serving model for now, because the local-first
constraints reward it:

- Tiny data with rare positives (~1.2k labels, ~2.6% positive). LR is stable here; KNN degrades
  because a query's nearest neighbours are almost all negatives, so it predicts "safe" even more
  uniformly than we already do.
- Sub-millisecond, dependency-free inference in the hook (a dot product over ~19 weights). KNN must
  hold the label set in memory and compute distances on every call; a real GBDT library adds binary
  size and per-platform build risk to `cargo install`.
- Calibrated probability we can threshold and, later, feed to the per-decision proof. KNN gives a
  coarse, poorly calibrated vote fraction.
- Inspectable weights, which the "what it learned on this repo" UI (ADR 0007) depends on.

The credible upgrade, when data supports it, is interaction features or a small gradient-boosted
tree (corrections are likely non-linear), capped at a single alternative. KNN is kept in mind only
as a retrieval aid for explanation ("last time you trimmed a file like this, you re-read it"), never
as the decision policy.

### Selection by task: the causal proof is the selector, not a leaderboard

We will do selection by task, but the selector is evidence, not a model bake-off:

- A model may act on a tool/repo slice only if the per-decision causal proof shows it does not
  increase corrections or re-reads there. Otherwise that slice falls back to the heuristic.
- Personalization comes from per-repo *data and gating*, not from per-repo *model families*.
- Model-family selection (LR vs one tree) is deferred. It multiplies the data needed (every
  candidate, on every slice, judged fairly) and invites a multiple-comparisons problem where a
  candidate "wins" by chance. On a local tool with sparse per-repo data, that selects noise. It is
  only worth revisiting once a single model has first beaten the rules somewhere.

## Local-install implications (why simple wins)

- Latency: the hook is synchronous and must not make the agent wait. Caps how heavy we can go.
- Footprint: one dependency-free Rust binary is a real install/trust feature; a model zoo or ML
  runtime breaks that.
- Privacy and cold start: all data stays local. That is the differentiation, but it also means each
  repo starts at zero labels and grows slowly, so data-hungry schemes rarely pay off.
- Determinism: training is deterministic so the same labels give the same model, which matters for
  debuggability when there is no central control.
- No monitoring: we cannot yank a bad model from a fleet, so safety must be self-enforcing. A clever
  selection scheme that can silently serve a bad model is dangerous here. Simple plus proven beats
  smart plus unwatched.

## Consequences

- The architecture stays model-agnostic (shadow score, then prove, then gate), so swapping LR for a
  tree later does not touch the wiring. Not a one-way door.
- The immediate priority is not the model family but Phase 2: replace the fixed 0.15 threshold with
  the per-decision proof so a model can earn specific slices. Until then the learned arm will keep
  mirroring the heuristic, and we report that honestly rather than dressing it up.
