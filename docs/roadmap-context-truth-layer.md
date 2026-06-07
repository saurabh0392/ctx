# ctx build roadmap: getting to the context truth layer

Status: draft for discussion
Date: 2026-06-06
Companion to: strategy-context-truth-layer.md

## How to read this

Work is grouped into horizons. Each epic says what it touches in the current codebase, whether it extends something or is net-new, what it depends on, and the proof gate (the evidence that closes it). Nothing is called "proven" until it clears the honesty gate on real labels.

Sequencing principle: data, then proof, then the wedge, then platform. We do not build the platform on an unproven loop.

## Current state (what already exists, so we do not rebuild it)

- Shadow collection: live again. `compress_decisions` records the would-do decision per tool result, applies nothing while `preset = off` (`src/compress/hook_io.rs`, `src/compress/shadow.rs`).
- Outcome join: windowed and completeness-gated. Claude uses a 15 minute time window, Cursor a 3 turn ordinal window, with surface provenance (`src/db.rs`, `src/surface/ingest.rs`).
- Model: 11-feature logistic with enforced honesty gates (100 labels, 15 positives, holdout AUC 0.60) in `src/learn.rs`.
- Per-tool activation gate: `PerToolGate` with observed correction and re-read rates (`src/learn.rs`, `src/compress/activation.rs`).
- Bench: `off`, `ctx-heuristic`, `ctx-learned` arms from the user's own labels; cross-system arms honestly "not measured" (`src/bench.rs`).
- Surface adapters: canonical types and a Cursor transcript adapter (`src/surface/`). No Codex. No live Cursor hook (ingest only).
- Outcome signal detection today is thin: one correction heuristic plus a few flags (`detect_flags` in `src/conversations.rs`). This is the sparsity risk.

## Horizon 0: foundation and honest hardening (unblock and de-risk)

Prerequisite for everything. Without reliable collection and visibility, nothing downstream can be trusted.

### E0.1 Durable shadow collection
- Decouple shadow collection from `experiment_hooks_enabled`. Install the record-only `PostToolUse` hook whenever `compress_shadow_enabled` is true, even in observation-only or experiment baseline phases. Shadow is observation, not intervention.
- Touches: `src/claude_settings.rs` (`apply_observation_only_to_settings_doc`, extract a shared post-tool hook helper), `src/config.rs`, regression test.
- Net-new: small. Extends existing.
- Proof gate: a test that observation-only settings still carry the shadow hook when shadow is on; running an experiment baseline no longer zeroes collection.

### E0.2 Collection health view
- Show the loop working: decisions over time, joined percentage, per-tool counts, and an honest estimate of distance to the training and activation thresholds.
- Touches: `/api/context`, Context tab (`src/dashboard_static/tabs/context.html`, `js/context.js`), `src/db.rs` queries.
- Proof gate: dashboard shows live counts and a "not yet" state until thresholds are met, never a placeholder.

### E0.3 Label audit tooling
- Sample joined labels and inspect them (the turn that triggered the correction, the decision it was joined to) so we can eyeball precision before trusting the corpus.
- Touches: new `ctx context labels` subcommand (`src/context_ctl.rs`, `src/cli.rs`), `src/db.rs`.
- Proof gate: can pull N labeled decisions with their evidence and judge precision by hand.

## Horizon 1: earn credibility on the loop

### E1.1 Richer outcome signals (highest priority, addresses the number one risk)
The current single short-turn heuristic will not produce enough trustworthy positives. Add independent signal types, each labeled with confidence:
- Re-read or re-run of a file or command the agent just touched (strengthen the existing `outcome_reread` join).
- Aborted or interrupted tool result (`tool_response.interrupted`, escape).
- Immediate re-edit of a file the agent just read or wrote.
- Explicit undo, revert, or "that is wrong" language (reuse and extend the Cursor lexical guard lexicon).
- Tool error followed by an immediate retry of the same command or path.
- Touches: `src/conversations.rs` (`detect_flags`), `src/db.rs` (join), `src/surface/cursor.rs` (lexicon), possibly a new signal module.
- Proof gate: a hand-labeled spot check showing precision holds, and enough positives to clear `MIN_POSITIVE_LABELS` without leaning on one noisy signal.

### E1.2 Reversible compression (rewind store)
- When ctx trims, store the original hash-addressed so the agent can re-expand a compressed block on demand. Makes compression safe by construction and fits the fail-closed ethos. This is also a differentiator the proxies (except Claw) lack.
- Touches: new `src/compress/rewind.rs`, apply path in `src/compress/hook_io.rs`, an MCP retrieval tool in `src/mcp.rs`.
- Net-new.
- Proof gate: a trimmed block round-trips to its verbatim original; retrieval works from the agent side.

### E1.3 Honest before and after on one tool (Bash first)
- An explicit workflow: measure the baseline correction rate for a tool in shadow, deliberately activate it, then measure the post-activation correction rate on the same tool, and report the delta with sample size and a confidence interval. Gated by the honesty thresholds.
- Touches: `src/context_ctl.rs`, `src/bench.rs`, Context tab.
- Proof gate: a real, dated before-and-after table with n and a confidence interval, where activation only happens after the tool earns it.

### E1.4 Model maturation
- Once labels clear the gate, validate that the learned arm beats the heuristic on the user's own holdout. Expand features only if the gate is not cleared. Surface model history (the improving view exists).
- Touches: `src/learn.rs`, dashboard improving view.
- Proof gate: holdout AUC at or above 0.60 and the learned arm beats `ctx-heuristic` on the user's labels.

## Horizon 2: the wedge nobody else can build

### E2.1 Compaction-harm detector (consider pulling forward as a hedge)
- Detect native compaction events and join the corrections that follow within a window: "your agent's compaction preceded N corrections this week." Claude already exposes a `pre_compact` flag, so the join machinery is mostly reusable. This needs far less label volume than the per-tool model, so it is the fallback that still wins if E1.1 signal stays sparse.
- Touches: `src/conversations.rs` (compaction events), `src/db.rs` (new join and possibly a `compaction_events` table), Context tab.
- Proof gate: a reproducible, windowed count of corrections following compaction, per surface.

### E2.2 Cursor outcome quality
- Cursor has no live hook, so it relies on transcript ingest. Strengthen the adapter and verify join precision; keep Cursor labels lower-confidence.
- Touches: `src/surface/cursor.rs`, `src/surface/ingest.rs`.
- Proof gate: Cursor decisions join with windowed completeness and a documented confidence discount.

### E2.3 Codex transport
- Implement the transcript adapter for Codex so its sessions normalize into the canonical corpus.
- Touches: new `src/surface/codex.rs`, `src/surface/mod.rs`.
- Net-new. Gate on whether there is real Codex usage to justify it.
- Proof gate: Codex sessions parse into canonical turns and results, and ingest joins outcomes.

### E2.4 Cross-surface dashboard
- One honest view across Claude, Cursor, and Codex: context cost, correction impact, and compaction harm. Show "unknown" where a surface has no data.
- Touches: Context tab, `/api/context`, surface provenance already in `compress_decisions`.
- Proof gate: all available surfaces render with honest empty states.

## Horizon 3: platform

### E3.1 Portable per-repo policy artifact
- Export and import the learned model and activation gates keyed by repo, so a team can share an earned policy.
- Touches: `src/learn.rs` persistence, a new export format, CLI.
- Proof gate: an artifact round-trips across machines and reproduces the same decisions.

### E3.2 Context truth score and API
- A stable per-repo and per-agent context health score with a documented endpoint.
- Touches: `src/dashboard.rs` API, a scoring module.
- Proof gate: documented, stable API schema.

### E3.3 Eval-stack integration
- Emit the ctx behavioral signal in a form the eval stack can ingest (OTel conventions, Langfuse or Braintrust compatible), positioning ctx as a production-behavior signal rather than a competitor.
- Touches: a new exporter.
- Proof gate: the signal lands in one external tool.

## Cross-cutting workstreams (always on)

- Honesty and copy: every metric labeled observed versus estimated, "not yet" and "none" instead of blanks, never a placeholder tool count. Follows the house copy rules and `tool_metrics_ready`.
- Tests and golden fixtures: each new signal and join ships with regression tests; `make dashboard-check` stays green.
- Dashboard consistency pass (owed by the prior plan): Request Trace reskin, Savings reframed to cost per task, Proving tab wired to `/api/bench`, Profiles reskin.
- Build discipline: teardown, build, setup, verify after each change.

## Critical path and the key sequencing decision

1. Horizon 0 first. Reliable collection and visibility gate everything.
2. E1.1 richer signals is the linchpin. Start it early, in parallel with label accrual, because signal sparsity is the top kill risk.
3. E1.3 before-and-after on Bash is the first real credibility milestone.
4. The open decision: prove the per-tool loop first (finish Horizon 1, then the wedge), or pull E2.1 compaction-harm forward as a hedge. E2.1 needs less label volume and is the fallback that still wins, so there is a strong case to run it in parallel with Horizon 1 rather than after it.

## Open decisions needed
- Sequencing: loop-first, or hedge by pulling the compaction-harm detector forward.
- Reversibility surface: an MCP retrieval tool, a hook, or both.
- Codex priority: is there enough real Codex usage to justify the adapter now.
- Tracking: keep this as a doc, or break the epics into Linear issues.
