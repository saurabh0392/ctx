# 0031. Same-file edit-follow label

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh (with CTO partner)
- Extends: ADR 0007 (per-decision retention model), ADR 0019 (richer outcome signals)
- Part of: CTX-46 (file-aware retention model), increment 2

## Context

The file-aware retention model (CTX-46) needs a training target that means "the agent actually
needed this read whole," observable on every read whether or not ctx trimmed it. ADR 0030 added the
file-identity features; this increment adds the label those features predict.

What we had:

- `outcome_correction`: the causal harm label the activation gate reads. A user pushed back after
  the decision. This stays the gate's only vote.
- `outcome_reread`: any later touch of the same path within the window. It does not distinguish a
  benign re-read from an edit, and it already fires for both (an edit is a later same-path touch).
- `reedit` in the observation-only `outcome_signals` JSON, computed only on the Cursor transcript
  path and never on the Claude path that the model trains on.

So the precise signal "the agent edited this file after reading it" existed nowhere the trainer
could use it. That edit is the strongest observational evidence that a read was load-bearing: you do
not edit a file you did not need to understand.

## Decision

Record a first-class same-file edit-follow label, `compress_decisions.outcome_edit_follow`: 1 when
the same file this decision touched is edited (an edit/write tool) within the outcome window, else
0. It is computed on both join paths from one shared edit-tool set
(`outcome_signals::EDIT_TOOL_NAMES`):

- Claude (timestamp join): a later same-path decision whose `tool_name` is an edit tool, using the
  same nearest-preceding attribution as `outcome_reread` so one edit is owned by the last read
  before it, not fanned across every earlier read. The SQL edit-tool predicate is generated from
  the shared set so it can never drift from `is_edit_tool`.
- Cursor (transcript ordinal join): the existing path-based `reedit` detection, now also persisted
  to the column rather than only the observation JSON.

The label is distinct from, and recorded alongside, `outcome_correction` and `outcome_reread`. It
does not feed the causal gate. It is exposed on `LabeledDecision` so the model can train on it.

## Propose, not dispose (the contract this label serves)

This label trains a model that *predicts which reads will be needed whole*, an observational prior.
It is not a harm label and never becomes one:

- The causal gate still proves, separately, that trimming did not raise corrections or re-reads.
- When the model is eventually allowed to act (a later increment), it may only *propose* a read as
  trim-eligible. The trim still goes through burn-in and the causal gate before it is trusted. No
  model score alone can apply a trim.

## Scope of this increment

In: the column and its migration, both joins, the shared edit-tool set and its SQL form, the label
on `LabeledDecision`, and tests pinning that an edit sets the label while a plain re-read does not.

Out (CTX-46 increment 3): switching the model's training target from P(correction) to the
observational P(needed whole), per-repo gating, the offline benchmark versus the kind-only model,
and the propose-not-dispose wiring behind a flag. Those are deferred because they cannot be
validated until labeled, file-tagged data accrues.

## Alternatives considered

- **Reuse `outcome_reread` as the "needed whole" target.** Rejected as the sole signal: it conflates
  a load-bearing edit with a benign re-read. Keeping edit-follow separate lets increment 3 weight or
  combine them deliberately rather than baking the conflation in now.
- **Detect edits only on the Cursor transcript path.** Rejected: Cursor rows are excluded from
  training, so the label would never reach the model. It must exist on the Claude path.
- **A dedicated edit-event table.** Over-engineered for a boolean per decision. The self-join on
  `command_or_path` reuses the machinery `outcome_reread` already trusts.

## Consequences

- The model gains a precise, observable target for "this read mattered," the foundation for
  file-aware trim proposals, accruing from normal usage with zero UX risk.
- No behavior change ships in this increment: the label is recorded and exposed, nothing trims
  differently, and `ctx learn` reports the same honest "not enough signal yet" until the target
  switch and enough data land.

### Empirical finding on first run (the honest blocker for increment 3)

Backfilling and re-joining the live corpus (1108 decisions, 848 joined) produced **zero** edit-follow
positives, while re-reads produced 198. The cause is not a bug in the label: it is that **every edit
decision in the corpus is on the Cursor surface** (138 `Write`, all `surface = 'cursor'`), and the
Claude/legacy surface that training reads has no edit decisions at all. Claude Code's `Edit`/`Write`
PostToolUse results are not landing as recorded decisions in this user's usage, so the timestamp
self-join can never find a same-path edit.

So the edit-follow target has signal only where ctx sees the full tool timeline (Cursor), and that
surface is excluded from training because its *correction* labels are language-derived and unproven.
That exclusion rationale does not obviously extend to this label: edit-follow is structural (we
observed a `Write` to the same path), not a language guess, so it is high-precision regardless of
surface.

Increment 3 must therefore resolve, with a decision recorded in its own ADR, one of:

1. Admit Cursor rows for the observational edit-follow target specifically (keep excluding their
   correction labels from the causal gate), since the structural signal is trustworthy; or
2. Record Claude `Edit`/`Write` results as decisions so edits enter the training corpus directly.

Until then the label is correct but all-zero for training, and `ctx learn` stays honestly untrained.
This increment ships the plumbing and surfaces the blocker rather than hiding a label that cannot yet
teach the model anything.
