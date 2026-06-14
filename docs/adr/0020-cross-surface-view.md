# 0020. Cross-surface view, and observe-only means applied=false

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh, CTX
- For: CTX-34 (CTX-27 increment 3)

## Context

ctx's neutrality is the moat: one honest layer across Claude Code, Cursor, and later Codex, which
no platform vendor will build. The live Cursor hook (CTX-27, ADR 0018) made ctx record Cursor
decisions stamped `surface = "cursor"` in real time, so the data to show both agents side by side
now exists. This ADR covers the view that surfaces it, and a correctness fix it exposed.

## Decision

Add a Surfaces view to the dashboard, fed by a new `surface_summary` query that aggregates
`compress_decisions` by provenance. It always returns Claude Code and Cursor, each with a `seen`
flag, so a surface ctx has not observed renders an honest "not seen yet" card rather than zeros
presented as a measured result (the same honesty pattern as the per-surface compaction view, ADR
0016). A NULL `surface` is a legacy/Claude row (provenance predates the column), so it folds into
claude-code.

The view states the real asymmetry instead of implying parity: Claude Code is "acting" (ctx trims
MCP and built-in output there), Cursor is "observing" (increment 1 watches only; trimming Cursor
MCP output is CTX-33, and built-in output stays observe-only per ADR 0018).

## Observe-only means applied=false (the fix this surfaced)

Building the view exposed a labeling bug in the Cursor hook: it recorded `applied = decision.apply`
even though increment 1 never rewrites Cursor output. That made the view claim "trims applied" on
Cursor and, worse, dropped those runs into the causal *trimmed* arm even though nothing was trimmed.

A trim that did not happen must never be recorded as applied. The hook now records `applied = false`
on Cursor for the whole observe-only phase; the would-do retention still rides along in the shadow
decision. The seven mislabeled rows on the author's machine were corrected to `applied = 0`, which
is what actually happened. CTX-33 will record a real apply when ctx genuinely rewrites Cursor MCP
output.

## Alternatives considered

- Define the view's "acted" metric to exclude Cursor instead of fixing the data. Rejected: the
  corrupt `applied` flag would still mislead the causal gate. Fix the source, not the display.
- Only show surfaces that have data. Rejected: hiding an unseen agent loses the cross-surface story
  ("ctx works across your agents") and the honest "not seen yet" is itself informative.

## Consequences

- The neutrality claim in the competitive docs is now demonstrable on one screen, with honest empty
  states and an explicit, non-parity asymmetry.
- `applied` now means "ctx actually shortened the output," uniformly across surfaces, so the causal
  trimmed/baseline split stays truthful as more surfaces come online.
- New per-surface aggregate to keep correct as surfaces are added; Codex slots in when it has data.
