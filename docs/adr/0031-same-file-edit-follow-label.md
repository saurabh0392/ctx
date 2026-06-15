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

## Making edits observable on the Claude surface (the real blocker)

The first live run showed zero edit-follow positives because the corpus had no edit decisions on the
training surface at all. Root cause: ctx's Claude `PostToolUse` hook was registered with the matcher
`Bash|Read|Grep|Glob|mcp__.*`, so Claude Code never invoked the hook for `Edit`/`Write`/`MultiEdit`.
Reads were recorded; edits were invisible. (The user edits via the Claude Code extension, so this
was a real gap, not just a workflow artifact.)

Decision: add `Edit|Write|MultiEdit` to the matcher so the hook observes edits, with a hard
guarantee that ctx **never trims an edit result** (`agent::decide_inner` forces edit tools
record-only regardless of preset, trial, or gate). An altered edit result could make the agent
misread what it just wrote, so observe-only is the only safe contract.

Edits are recorded into `compress_decisions` purely as timeline events for the join. Because ctx
never trims them, they must not pollute the trim model or the trim ladder, so a shared
`db::EXCLUDE_EDIT_TOOLS` fragment (pinned by test to `outcome_signals::EDIT_TOOL_NAMES`) drops edit
rows from training (`load_joined_decisions`), the per-tool activation gate
(`compress_tool_progress`), and the causal ladder (`causal_tool_outcomes`). Edits never get an
explore arm (they are never trim-eligible), so the randomized view excludes them naturally.

## Scope of this increment

In: the column and its migration, both joins, the shared edit-tool set and its SQL form, the label
on `LabeledDecision`, the `PostToolUse` matcher fix plus the never-trim guarantee and the
trim-corpus exclusion for edit tools, and tests pinning all of the above.

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

### Empirical finding on first run, and the fix

Backfilling and re-joining the live corpus (1108 decisions, 848 joined) produced **zero** edit-follow
positives, while re-reads produced 198. Investigation traced it to the `PostToolUse` matcher above:
edits never reached ctx on the Claude surface, so no same-path edit existed for the join to find.
Adding `Edit|Write|MultiEdit` to the matcher (this ADR) closes that gap; edits now record as
timeline events and the label can fire as new sessions land.

Two honesty notes that remain true:

- Edit-follow is structural (we observed a `Write` to the same path), not a language guess, so it is
  high-precision wherever ctx sees the tool. This is why it is safe to use as an observational
  target even though Cursor's *correction* labels are excluded from training.
- The matcher change only populates the label going forward. Existing joined rows predate any edit
  decision, so the one-time backfill leaves them at 0; that is correct (there were no observed
  edits then), not a hidden under-count.

Increment 3 (the model target switch, per-repo gating, benchmark) stays separate and is unblocked
once edit-follow positives accrue from real Claude-extension editing.
