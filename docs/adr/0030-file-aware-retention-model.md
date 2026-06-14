# 0030. File-aware retention model: path-role features

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh (with CTO partner)
- Extends: ADR 0007 (per-decision retention model)

## Context

The local retention model (ADR 0007, `src/learn.rs`) predicts P(a decision precedes a correction)
from trim-shape features (drop ratios, risky-drop counts) and a tool-kind one-hot. It was blind to
*which file* a read touched, so it could not learn the most intuitive signal: reads of source you
are editing behave differently from reads of vendored code or generated output. We already log
`repo_key` and `file_ext` per decision and the model ignored them.

This is the first increment of CTX-46 (a file-aware model that learns which reads are safe to trim).
It is deliberately small and self-contained, because the larger pieces (a same-file edit-follow
label, per-repo gating, and letting the model propose trims) cannot be validated until real labeled
data accrues, and shipping them unproven would be a vanity feature.

## Decision

Add a path-role one-hot block to the model's feature vector: `src`, `test`, `config`, `generated`,
`vendored`, `docs`. The role is the `path_role` logged per read decision (CTX-45 / ADR 0029),
derived by `agent::path_role_of`. The single source of truth for the feature vector
(`learn::feature_vector`) is unchanged in contract, so live scoring and training stay identical.

- Historical rows and non-read decisions have no `path_role`, so the block is all-zero, a safe
  default. The feature takes effect as file-tagged data accrues.
- Adding features grows the vector, which invalidates any previously served model by feature-shape
  mismatch (`score_parts` returns `None` until a model is retrained at the new shape). The system
  reports "not enough signal" rather than serving a stale or mis-shaped model, so this is safe by
  construction.
- No behavior change to trimming: the model remains logged-only (`would_model_apply`) and never
  steers an `apply` decision. That wiring is a later CTX-46 increment, still gated by the causal
  proof.

## Alternatives considered

- **High-cardinality `file_ext` one-hot or hash now.** Deferred. Path role is low-cardinality,
  interpretable, and captures most of the file-class signal (a `.md` is docs, a lockfile is
  vendored, etc.) without sprawl. An extension bucket can follow once the role features prove their
  worth on real data.
- **Wait and add all CTX-46 features at once.** Rejected: a smaller, tested increment that starts
  improving the model as data lands beats a large change we cannot validate yet.

## Consequences

- The model can now learn file-role-dependent risk, the foundation for "which reads are safe to
  trim." It earns its keep only as labeled, file-tagged data accrues; until then `ctx learn` honestly
  reports insufficient signal, unchanged.
- The feature vector grew, so the next training run starts a fresh model version; nothing stale is
  served in the interim.
- Remaining CTX-46 work is unblocked but explicitly out of this increment: the same-file edit-follow
  label (needs Edit/Write path ingest), per-repo gating, the offline benchmark vs the kind-only
  model, and the propose-not-dispose wiring behind a flag.
