# 0012. Earn tools automatically with bounded burn-in; shelve Phase 2 exploration

- Status: accepted
- Date: 2026-06-12
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-23 (auto burn-in), supersedes the active use of CTX-15/CTX-16 (learned model) and ADR 0009 (Phase 2 exploration)
- Extends: ADR 0006 (earned savings), SAU-150 (causal before/after gate)

## Context

The causal gate in `compress/activation.rs` is correct: a tool only earns auto-activation once it
has enough "left alone" runs (baseline arm) and enough "actually cut" runs (trimmed arm), with the
trimmed correction and re-read rates not measurably worse than baseline. The problem is not the
gate, it is how a tool ever gets a trimmed arm.

Today the only ways a tool gets trimmed (and so builds its "after" arm) are:

1. A hand-written list, `compress_trial_tools`, currently `["Bash", "Read"]`.
2. `compress_force_active`, a blunt bypass.

That creates a chicken-and-egg. A tool not on the hand-list can never trim, so it never builds a
trimmed arm, so it can never clear the gate, so it never trims. On a real install this showed up
exactly as expected: Bash and Read had healthy trimmed arms (39 and 31 runs) and were saving real
tokens, while every other compressible tool (Grep, Glob, and high-value MCP results that are
97-99% trimmable) sat at zero trimmed runs forever. The product looked inert because its on-ramp
was a two-item list a human had to maintain.

Two facts make an automatic on-ramp safe:

- **Trimming only risks a re-read, not lost data.** Compression shrinks what the model sees in the
  tool result; the full output still lands in the user's transcript, and error text and secrets are
  preserved/redacted by existing safeguards. The worst case of a bad trim is the model asking for
  the content again (`outcome_reread`), occasionally a correction. Both are measured.
- **We already have a strong "before" arm for free.** Every compressible tool accumulates baseline
  rows (would-trim, left alone) just by being used. That is the evidence needed to decide a tool is
  worth trialing.

Separately, Phase 2 randomized exploration (ADR 0009) was meant to gather an unbiased control arm
by withholding 20% of eligible trims. After weeks of real use the control arm had a handful of
samples (2 Read, 2 Bash): nowhere near enough to support a per-decision causal claim, while the
withholding measurably cut savings. It was paying a real cost for data we could not use.

## Decision

### Automatic bounded burn-in (CTX-23)

Add an automatic on-ramp so a tool earns without a hand-written list. A tool enters **burn-in**
(starts trimming to build its "after" arm) when, and only when:

- `compress_enabled` and the new `compress_auto_trial` flag are on (default on), and
- the current preset allows the tool's kind (burn-in respects autopilot; it never trims when the
  user has autopilot off), and
- the tool has a solid baseline arm (`baseline_n >= min_baseline`, currently 30), and
- the tool's "after" arm is not yet full (`trimmed_n < min_trimmed`, currently 30), and
- the baseline correction rate is not pathological (a sanity fuse: do not start trimming a tool
  whose left-alone output already correlates with a high correction rate).

While in burn-in the tool trims, capped implicitly at `min_trimmed` runs, because once the trimmed
arm is full the existing causal gate (`causal_clears_bar`) takes over: clean tools become earned
and keep trimming, harmful tools fail the gate and stop. So burn-in is a bounded window between
"enough before" and "enough after", after which the honest causal comparison decides.

The hand-written `compress_trial_tools` list stays as a manual override for forcing a trial, but it
is no longer the only path. `compress_force_active` is unchanged.

### Shelve Phase 2 exploration and the learned model

- Default `compress_explore_rate` to `0.0`. Exploration no longer runs. The plumbing
  (`explore_arm`, `explore_tool_outcomes`) stays so it can be re-enabled deliberately if a future
  decision needs a clean control arm, but it is off and makes no behavior change.
- The learned retention model stays exactly where it already is: shadow-only. It records a score on
  each decision (`features.model_score`, `would_model_apply`) but never touches `apply`. It is not
  removed, it is parked. It does not appear as a user-facing capability until it can be shown to
  beat the heuristic gate on real data.

## What this deliberately does not do

- It does not trim when autopilot (the preset) is off. Burn-in is part of autopilot, not a bypass.
- It does not let the learned model steer trims. Still shadow-only.
- It does not add a mid-trial early-abort beyond the entry sanity fuse. Because a bad trim only
  costs a re-read and the burn-in window is bounded, v1 relies on the entry fuse plus the causal
  gate at the end. A mid-trial abort (stop early if the partial trimmed arm looks clearly worse) is
  a noted future refinement, not v1.
- It does not change the gate math (Wilson/Newcombe) or its thresholds. Same definition of earned.

## Alternatives considered

- **Lower the gate thresholds so the current arms clear.** Rejected: the thresholds are calibrated
  to the sample size already (ADR/SAU-150). The problem was never the bar, it was that only two
  tools could ever reach it.
- **Expand the hand-written trial list.** Rejected as the endpoint: it does not scale and it is not
  autopilot. A human curating which tools may trim is the opposite of "ctx trims each tool as it
  earns it".
- **Keep Phase 2 running to feed a per-decision model.** Rejected: it cost real savings for data
  that did not accumulate to a usable size. Earn-by-tool via burn-in is the honest mechanism that
  actually ships value now.

## Consequences

- Value flows automatically. Any compressible tool with a solid clean baseline now starts earning
  on its own, including high-value MCP results, not just the two hand-listed tools.
- The honest story holds: a tool still has to clear the same causal before/after bar to stay
  trimming. Burn-in is a bounded, low-risk window (worst case: a re-read), gated behind autopilot.
- The dashboard gains a real third state per tool: watching (building baseline), learning
  (burn-in), earned (cleared the gate). The UI must show this plainly.
- Savings stop being withheld for an experiment, so observed savings should rise immediately.
- The "proven safe" language must stay honest: a tool in burn-in is being tested, not yet proven.
  Only earned tools may carry proof language.
