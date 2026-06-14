# 0017. Loop-health view (CTX-26, increment 1)

- Status: accepted (IA superseded in part by 0022: Loop health is now the single per-tool surface)
- Date: 2026-06-14
- Deciders: Saurabh, CTX

## Context

The whole ctx position rests on the causal gate producing trustworthy results on real,
sparse data. The top kill risk (strategy doc) is signal sparsity: on one developer's traffic,
a tool may sit in "watching" for a long time before it has enough evidence to test, and longer
still before it earns. Today the dashboard shows per-tool counts but does not answer the one
question a skeptical developer actually asks: "is anything accruing on my machine, and how far
is each tool from a verdict?" Without that, the day-one story reads as a wall of zeros instead
of a loop that is visibly working.

CTX-26 has two halves. Half one (this ADR) is a read-only loop-health view over data we already
collect. Half two is richer outcome signals (re-reads, aborts, re-edits, undo language, error
retries), which can raise the positive-label rate but carry false-positive risk and need a
hand-labeled precision spot-check before they touch the gate. Shipping the two together would
couple a safe, observational change to a riskier one and stall both.

## Decision

Ship the loop-health view first, as a self-contained read-only increment. No gate math changes
(ADR 0012 stands), no new signals yet.

1. One definition of stage. Add `compress::activation::tool_stage`, a pure classifier over a
   tool's `CausalToolOutcome` and the same `CausalThresholds` the live gate uses, returning one
   of: `watching` (baseline arm below `min_baseline`, with how many left-alone runs to go),
   `learning` (baseline met, trimmed arm building, how many trimmed runs to go), `held` (baseline
   met but its left-alone correction rate is above the burn-in fuse, so autopilot will not trial
   it), `earned` (trimmed arm full and `causal_clears_bar` passes), or `blocked` (trimmed arm full
   but the harm interval is too high). The gate, the status label, and this view now share one
   definition of where a tool stands.

2. Honest accrual over time. Add `db::decisions_by_day`, a read-only per-day count of decisions
   and how many joined to an outcome, so the view can show whether signal is actually arriving and
   how much of it gets labeled, rather than a single lifetime total.

3. A dedicated view. Surface it on `/api/context` as a `loop_health` object and render it as its
   own "Loop health" tab, mirroring how the Compaction tab (CTX-25) was added. Empty and early
   states read as the loop working ("watching, 18 of 30 left-alone runs to start testing"), never
   as a placeholder or a fake number. When `total` is zero, the view says so plainly and explains
   what unlocks it.

## Alternatives considered

- Fold it into the Home story. Rejected: CTX-19 deliberately made Home one simple story; a
  per-tool distance-to-threshold diagnostic belongs one level down, not on the front door.
- Ship richer signals in the same change. Rejected: couples a safe observational view to a
  risky detector that needs a precision gate, and would delay the visible-proof win the view
  delivers on its own.
- Compute stage ad hoc in the dashboard. Rejected: the gate already owns "earned"; a second,
  drifting definition in the UI is exactly the kind of dishonesty a truth layer cannot ship.

## Consequences

- A skeptical developer can watch a tool move watching -> learning -> earned on their own
  sessions, with real numbers and an honest distance to each threshold.
- One more nav tab, and `tool_stage` becomes the shared source of truth for stage labels; any
  future gate change must update it in one place.
- This does not raise the positive-label rate. Sparsity is now visible, not solved. Richer
  outcome signals (CTX-26 increment 2) remain the lever for that, gated by a precision spot-check.
