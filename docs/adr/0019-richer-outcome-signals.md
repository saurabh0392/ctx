# 0019. Richer outcome signals, observation-first behind a precision gate

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh, CTX
- For: CTX-32 (CTX-26 increment 2)

## Context

The causal activation gate (ADR 0012) and the learned model lean almost entirely on two outcome
labels: `correction` and `reread`. On one developer's sparse traffic, positive labels arrive
slowly, so tools sit in "watching"/"learning" a long time and the learned model rarely clears
`MIN_POSITIVE_LABELS` (15). CTX-26 increment 1 made that sparsity visible (loop-health view); it
did not fix it. More honest outcome signals would raise the positive-label rate.

But the same property that makes a signal useful makes it dangerous: anything that votes on a
tool's harm directly changes what ctx trims. A false-positive signal does not just add noise, it
corrupts the gate and can suppress or unlock trimming for the wrong reasons. For a product whose
whole pitch is "we prove trimming is safe," a corrupting label is worse than no label. So the proof
gate on CTX-32 is non-negotiable: no new signal may influence the gate until its precision is
shown on real, hand-labeled data.

What already exists (do not rebuild):

- `outcome_signals.rs` classifies user-turn language: explicit complaints (`wrong`, `revert`,
  `undo`, "doesn't work", ...) and terse redirects, with a confidence tier, plus an interrupt
  detector (`[Request interrupted by user]`).
- The join (`surface/ingest.rs` ordinal/fingerprint for Cursor, `db::join_compress_outcomes`
  timestamp-based for Claude) records `outcome_correction` and `outcome_reread` per decision.
- `TurnFlag` already carries `Correction`, `Aborted`, `PreCompact`. `reread` is "same input
  fingerprint called again within the window."

So of the five signals CTX-32 lists, three are largely covered already: re-read (exists), abort /
interrupt (exists), and explicit undo/revert/"that's wrong" language (exists in the lexicon). The
genuinely new ones are **immediate re-edit** (a file read or written, then edited again right
after) and **tool error then immediate retry** (a tool call that failed, retried within the
window). Both are structural: they depend on tool-call attributes we do not record yet (whether a
call failed; whether it was an edit), not on new user-language heuristics.

## Decision

Add the new signals observation-first, decoupled from the gate, and only let them vote after a
per-signal precision spot-check passes. Three increments, each separately shippable and gated.

1. **Observe, do not vote.** Record every signal that fires per decision as its own labeled
   observation, alongside the existing `outcome_correction` / `outcome_reread`, without changing
   what the gate counts as harm. Concretely: persist a per-decision `outcome_signals` set (signal
   name plus confidence tier) and populate it in both joins. The gate and the learned model keep
   reading only the proven labels. This is pure instrumentation: it cannot corrupt the gate because
   nothing new feeds it yet.

2. **Spot-check precision.** Add an audit command that samples decisions where each new signal
   fired, shows the surrounding turns, and lets a human mark true/false positive. Report precision
   per signal and the positive count each contributes. A signal is promotable only if its precision
   clears the bar (proposed: at least 0.8 precision on at least 20 hand-labeled samples) and it adds
   positives the corpus does not already get from corrections alone. These numbers live in the
   audit output, not in code that votes.

3. **Promote, one signal at a time.** Only signals that pass step 2 get folded into the harm label
   the gate reads, behind their confidence tier (explicit/structural-high vote at full weight;
   terse/structural-low stay down-weighted or fail-safe-only, mirroring `CorrectionClass`). Each
   promotion is its own small change with the audit result attached. ADR 0012 gate math is
   unchanged; we are only widening what counts as a labeled negative outcome, and only with proof.

Structural detectors (re-edit, error-then-retry) are implemented as pure functions over the
ordinal/fingerprint/flag timeline so they are unit-testable in isolation, then called from the
join exactly where `reread` is computed today. This requires adding two attributes to the parsed
tool-call model (did the call fail; is it an edit) and populating them per adapter.

## Alternatives considered

- Wire the new signals straight into the gate behind their confidence tiers and "watch the
  numbers." Rejected: this is exactly the corrupting-label risk the ticket calls out. Confidence
  tiers reduce weight; they do not prove precision. Proof has to come before influence, not after.
- One generic "dissatisfaction" label instead of per-signal labels. Rejected: it would hide which
  signal is noisy, making the precision spot-check impossible to act on per signal.
- Skip persistence and compute signals on the fly in the audit. Rejected: precision must be
  measured on the same labels the gate would later use, so they have to be recorded the same way.

## Consequences

- The positive-label rate can rise without ever risking the gate, because observation and influence
  are separate steps with a proof gate between them.
- New per-decision storage (`outcome_signals`) and two new tool-call attributes; both joins and the
  Cursor adapter must populate them. The dashboard can later show which signals are accruing.
- CTX-32 cannot be "done" in one sitting by construction: step 3 depends on real signals accruing
  and a human precision pass. That is the honest cost of not corrupting the gate, and it is the
  point of the ticket. Increment 1 (observe) and the audit (step 2 tooling) ship first and stand on
  their own.
