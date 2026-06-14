# 0014. Re-sequence the compaction-harm detector to the front as the lead wedge

- Status: accepted
- Date: 2026-06-13
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-25 (compaction-harm detector), part of the CTX-24 competitive follow-ups
- Extends: ADR 0013 (competitive position), strategy-context-truth-layer.md (Horizon 2, kill criteria), roadmap E2.1
- Re-sequences: roadmap-context-truth-layer.md critical path (which left "prove the per-tool loop first vs pull E2.1 forward" as an open decision) and the implicit Horizon 1 then Horizon 2 ordering

## Context

The competitive analysis (docs/competitive/) confirmed two things that change sequencing.

First, the defensible moat is not trimming. It is neutral, cross-agent, production-behavior proof. The single most defensible expression of that is the compaction-harm detector: "your Claude or Cursor compaction preceded N corrections this week." A platform will never ship an honest measurement that makes its own compaction look bad, and a benchmark-based tool cannot make the claim at all. See 60-threats-and-moats.md.

Second, ctx's top kill risk is signal sparsity. The per-tool causal loop needs enough corrections and re-reads, joined to enough trimmed runs per tool, to clear the honesty gate. After the reset to zero labels, that data is sparse, and the day-one story is weak (strategy doc, risks).

The roadmap already noted (E2.1, critical path) that the compaction-harm detector needs far less label volume than the per-tool model, because it joins corrections to a coarse, already-flagged event (Claude exposes a `pre_compact` flag) rather than to per-tool trimmed arms. So the feature that is both the most defensible and the least data-hungry was scheduled after the feature that is the riskiest to prove. That ordering is backwards for the competitive moment.

## Decision

Pull the compaction-harm detector forward to be the lead measurement feature, built in parallel with (not after) the per-tool proof loop.

Concretely:

- The compaction-harm detector (roadmap E2.1) moves from Horizon 2 to the front of the build queue, alongside Horizon 1 label accrual. It becomes the feature ctx leads the narrative with (ADR 0013, pillar 2: earned, not assumed, extended to "and we measure the platform's own context decisions too").
- Trimming and the per-tool causal loop are not paused or removed. They remain the engine that delivers savings and generates per-tool labels (see the discussion captured in CTX-24 comments: removing trimming would remove both ctx's savings and its only ground-truth source). The re-sequencing is about which measurement claim ships first, not about dropping the action.
- The compaction-harm detector is explicitly the hedge that still wins if per-tool signal stays sparse. If the per-tool gate cannot clear on real data, ctx still has a defensible, shippable product: an honest, cross-surface measure of when context compaction preceded corrections.

## What this deliberately does not do

- It does not remove or de-prioritize trimming. Trimming on native Read, Grep, Glob, and MCP outputs remains ctx's surface advantage and its label source.
- It does not lower or change the honesty gate math for the per-tool loop (ADR 0012 stands).
- It does not claim the detector is proven before it produces a reproducible, windowed, per-surface count on real sessions.
- It does not commit Codex or Gemini surfaces; the detector ships first where ctx already has signal (Claude Code, then Cursor).

## Alternatives considered

- Prove the per-tool loop fully first, then build the detector (the old default). Rejected: it puts the riskiest-to-prove feature ahead of the most defensible and least data-hungry one, exactly when the competitive window favors leading with proof.
- Build only the detector and pause trimming. Rejected for the reasons in ADR 0013 and the CTX-24 discussion: no trimming means no savings and no per-tool labels, and pure measurement is Langfuse and Braintrust territory.
- Wait for more label volume before deciding. Rejected: the decision is cheap to make now and the detector accrues its own (different, coarser) signal regardless.

## Consequences

- ctx gets a defensible, demonstrable feature sooner, and a hedge against its top kill risk.
- The narrative tightens: ctx trims where it has earned it, and measures context harm everywhere, including the platform's own compaction it cannot control.
- New honesty surface to keep clean: the detector must show "unknown" where a surface has no data, must not imply causation it cannot support (it reports corrections that followed compaction within a window, not proof the compaction caused them), and must label confidence per surface (Cursor lower than Claude Code).
- Referenced from docs/competitive/70-gtm-implications.md and 90-open-questions.md.
