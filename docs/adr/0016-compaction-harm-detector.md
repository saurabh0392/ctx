# 0016. Compaction-harm detector: reuse the transcript compaction signal, Claude-first, Cursor honest-unknown

- Status: superseded in part by ADR 0047
- Date: 2026-06-14
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-25 (compaction-harm detector, lead wedge)
- Extends: ADR 0014 (re-sequenced this feature to the front), ADR 0013 (position on surfaces and proof)

> Historical note: ADR 0047 adds native Claude/Codex post hooks and separates attempted,
> confirmed-completed, and transcript-inferred compaction. This ADR remains the origin of the
> non-causal correction-follow-up window only.

## Context

ADR 0014 pulled the compaction-harm detector to the front as ctx's lead measurement claim: a neutral, local, cross-agent count of corrections that followed a native context compaction. The honest framing is "corrections that followed compaction within a window," never "compaction caused them."

The data model already carries most of what this needs:

- Claude Code ingest (`conversations.rs`) detects a compaction from the transcript: a system row with `compactMetadata` parses to `Row::Compact`, and the exchange turn right before it gets the `pre_compact` flag. Correction turns already get `correction` / `correction_explicit` flags through the shared lexical guard. All of this is persisted to the `turns` table (`flags` is a JSON-array string, `turn_index` is the per-session order, `ts` is RFC3339).
- The `sessions` and `turns` tables are Claude-Code-only. Cursor turns are parsed in memory by the transcript adapter for the per-decision outcome join, but are not persisted as turns, and Cursor sessions never land in `sessions`.

So the question is not "how do we detect compaction" (we already do, for Claude). It is "what is the honest windowed measure, and how do we report surfaces where we have no signal yet."

## Decision

Build the detector as a read-only windowed self-join over the existing `turns` table. No new hook, no new ingest, no `compaction_events` table in v1.

1. Reuse the transcript compaction signal. A compaction event is a turn flagged `pre_compact` (the turn immediately before a `compactMetadata` system row). We do not add the Claude `PreCompact` hook: the hook fires before a compaction that may not happen, while the transcript records compactions that actually occurred, which is what an honest after-the-fact measure needs.

2. Window by turn index within a session. For each `pre_compact` turn at index `i`, a correction "follows the compaction" if a turn flagged `correction` has index in `(i, i + COMPACTION_FOLLOWUP_WINDOW_TURNS]`. The window is a small constant (5 turns), wider than the per-tool correction window (3 turns) because a compaction's effects can surface a turn or two later. Turn index, not wall-clock, is the ground truth here: it is contiguous per session and present on every surface that persists turns.

3. Report per surface with explicit confidence and honest unknowns. The detector returns one row per surface. Claude Code reports real counts at "observed" confidence. Cursor and Codex report `unknown` (no persisted compaction signal yet), never zero, because zero would falsely imply "we looked and found none." The UI must render unknown surfaces as "we can't see this yet," not as a clean result.

4. No causal language anywhere. The struct field and all copy say "followed" / "within N turns," never "caused" or "because of."

## Alternatives considered

- **Add the Claude `PreCompact` hook and a `compaction_events` table.** Rejected for v1: the transcript already records real compactions with their surrounding turns, so a hook adds a moving part and a pre-event (may-not-happen) signal for no extra fidelity. A dedicated table becomes worth it only when we persist Cursor/Codex compaction events; revisit then.
- **Window by wall-clock minutes (like the per-tool join).** Rejected as the primary key: some surfaces have no timestamps, and within a single session turn index is the more faithful order. We keep `ts` available for a future refinement but do not depend on it.
- **Persist Cursor turns now so Cursor shows real numbers.** Deferred: it is a larger ingest change and ADR 0014 says ship first where we already have signal. Cursor renders as honest-unknown until its turns/compaction events are persisted (a named follow-up).
- **Put the detector on the existing Home or Activity view.** Rejected: ADR 0014 makes this the feature ctx leads the narrative with, so it gets its own first-class view with a designed empty state, not a buried card.

## Consequences

- ctx gets a shippable, reproducible, per-surface count of corrections following compaction on real Claude Code sessions, with Cursor shown honestly as not-yet-visible. This is the defensible cross-agent claim from the competitive analysis, available now.
- The measure is only as good as the `pre_compact` and `correction` flags it reuses. That is acceptable: those flags already gate live behavior and are tested. If the flagging improves, the detector improves for free.
- New honesty surface to keep clean: the view must show `unknown` (not `0`) for surfaces without data, must label confidence per surface (Claude Code observed, Cursor lower), and must never imply causation. These are verified as part of the UI sign-off.
- A follow-up is created to persist Cursor compaction and correction turns so Cursor can graduate from `unknown` to a real, lower-confidence count.
