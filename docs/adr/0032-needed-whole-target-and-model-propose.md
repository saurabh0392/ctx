# 0032. Needed-whole training target, file-aware proof, and gated model proposing

- Status: accepted
- Date: 2026-06-19
- Deciders: Saurabh (with CTO partner)
- Extends: ADR 0007 (per-decision retention model), ADR 0030 (file-aware features), ADR 0031 (edit-follow label)
- Part of: CTX-46 (file-aware retention model), increment 3

## Context

Increments 1 and 2 made the retention model file-aware (path-role features) and added the
observational labels (re-read, same-file edit-follow). But the model was still fit against
P(correction), and that target never had enough positives to clear the honesty gate: with
compression off there is very little to correct, so the model could not beat a coin flip and stayed
untrained. Meanwhile the observational "did the agent need this read whole" signal is roughly ten
times denser. On the user's own corpus when this shipped, one repo had 104 joined reads with 50
needed-whole positives, against 35 corrections corpus-wide.

We also needed to answer two questions before the model is ever allowed to change a live trim:

1. Does knowing *which file* a read touched actually help, or is it noise dressed up as signal?
2. How does the model steer without silently overriding the safety guards that protect working reads?

## Decision

Three things, all local, deterministic, and off by default at the live layer.

1. **Train on the observational "needed whole" target.** The model now predicts
   `needed_whole = re-read OR same-file edit-follow within the outcome window`, not P(correction).
   For a read, edit-follow is a strict subset of re-read (an edit is a same-file touch), so the
   binary target is effectively "was it re-read or edited," with edit-follow kept for precision and
   the dashboard story. The volume gate now counts needed-whole positives. The causal correction
   gate is unchanged and still governs every live trim; this model is a separate "propose" signal.

2. **Prove file-awareness against a kind-only twin.** On every train we fit twice on the identical
   holdout split: the full model, and a twin with the path-role block masked to zero. The file-aware
   model only earns `file_aware_wins` when it beats the twin's holdout AUC by `MIN_FILE_AWARE_MARGIN`
   (0.02). Proposing is gated on this, so file features that do not earn their keep never steer.

3. **Per-repo, default-off, propose-only steering.** A new flag `compress_model_propose` (default
   off) lets the model *propose* trimming a working read the static edit guard would keep. A proposal
   can only lift that guard; it is ANDed with `base_apply`, so the preset, burn-in, and causal
   activation gate still decide whether the proposed trim is actually taken. The proposal is further
   confined to a repo that has cleared its own label gate (`MIN_REPO_LABELS` joined and
   `MIN_REPO_POSITIVES` needed-whole), so the model never steers a repo on another repo's confidence.
   A test pins that no model score alone can make a trim apply.

## Alternatives considered

- **Keep P(correction) and wait for more corrections.** Rejected: with compression off the signal
  barely accrues, so the model would stay untrained indefinitely. The observational target is both
  denser and the more honest question for "is this read safe to drop."
- **Add a second model instead of switching the target.** Rejected for now: two models, two
  benchmark arms, and two stories to keep honest, for no extra signal. The retention model's job is
  "which reads are safe," which is exactly the needed-whole question.
- **Train a separate logistic per repo.** Rejected as premature: only one repo has real volume.
  A global fit with a per-repo readiness gate gives the same protection without overfitting a repo
  with a handful of labels.
- **Add a hashed repo-id feature.** Rejected: with one dominant repo it would memorize the repo,
  not learn transferable structure. Per-repo gating belongs at the propose decision, not in the
  features.
- **Let the model trim directly when confident.** Rejected: it would bypass the causal gate that is
  the system's whole honesty claim. Propose-only keeps the gate as referee.

## Consequences

- The model can finally train where it never could, on a signal that is the right question for
  retention. `ctx context learn` now reports the target, both AUCs, whether file-awareness wins, and
  which repos are ready to propose.
- `score_decision` / `score_parts` now return P(needed whole); low means safe to trim. The benchmark
  `ctx-learned` arm keeps the same "low score, would trim" logic and still measures real outcomes, so
  its semantics stay coherent. The dashboard `base_correction_rate` is kept as a separate, honest
  correction stat alongside the new `base_need_rate`.
- Live behavior is unchanged by default: `compress_model_propose` is off, and even on, a proposal is
  inert until a repo clears its gate and the file-aware model beats its twin.
- We now carry a per-repo readiness map and a kind-only twin fit per train. Both are cheap (one extra
  logistic on the same split) and local.
- Still pending real proof: the model must clear `MIN_HOLDOUT_AUC` on the needed-whole target and
  beat the twin on the user's accruing data before proposing is worth turning on. That is the "wait
  on data" step; the mechanism is shipped and gated, the verdict is not claimed.
