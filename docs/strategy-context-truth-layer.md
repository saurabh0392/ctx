# ctx strategy: from "another compressor" to the trust layer for context

Status: draft for discussion
Date: 2026-06-06
Owner: ctx
Source: competitive research across native agent compaction, compression proxies, observability and eval platforms, memory layers, plus the 2026 research frontier on compression evaluation.

## TL;DR recommendation

Stop positioning ctx as a compressor. The compression-ratio game is already commoditized and we will lose it. The whole field has converged on the insight ctx was built around: compression ratio is the wrong metric, outcome is the right one. But everyone measuring outcome does it with LLM judges on benchmarks or offline eval sets. Nobody measures it from the real user's own production behavior, across multiple agents, with fail-closed activation. That is ctx's lane.

Reposition ctx as the context truth and safety layer: the only thing that proves, in your actual workflow, whether a context intervention (yours, a compressor's, or the agent's own compaction) helped or hurt, and only acts once earned.

## The landscape

### Tier 1: native agent compaction (the real default competitor)
- Claude Code: mature server-side compaction (`compact_20260112`), microcompact for tool results, subagents, 1M context.
- Cursor: flash-model summarization and file condensation, no tuning, degrades after 20-30 exchanges.
- Codex: opaque server-side compaction at about 99.3% ratio, zero visibility into what was dropped.

These are free and improving fast. Competing on "we compact too" loses.

### Tier 2: compression proxies (the direct lookalikes)
- Kompact: 40-70% savings, transparent proxy, content-aware transforms, savings dashboard.
- Claw Compactor: 14-stage pipeline, AST-aware, reversible via a rewind store, zero inference cost.
- Kompress / Headroom: ModernBERT drop-in replacement for LLMLingua.
- token-compressor: MCP server with an embedding-similarity gate.

They compete on ratio and ROUGE. None measure real downstream task harm.

### Tier 3: observability and eval
- Langfuse (open source, ClickHouse-acquired Jan 2026), LangSmith (LangChain-bound), Braintrust (eval-first, CI gates, "prove it got better"), Helicone (gateway and cost), Arize.
- They measure and gate, but they do not act on context, and their signal is offline eval datasets and LLM judges.

### Tier 4: memory layers
- Mem0 (fact extraction and personalization), Zep (temporal knowledge graph, wins LongMemEval), Letta/MemGPT (agent runtime).
- Different problem (cross-session memory), not context-window safety. Do not fight here.

### The research frontier (the important part)
Three 2026 results validate ctx and reveal where the edge is moving:
- Factory.ai built probe-based compression eval and said plainly: "Compression ratio turned out to be the wrong metric entirely. OpenAI achieves 99.3% compression but scores lower on quality. What matters is total tokens to complete a task."
- An ICAE / SWE-bench paper showed a compressor that improved similarity scores solved 79 fewer issues. Similarity is not outcome.
- Slipstream introduced trajectory-grounded compaction validation (+2.6 to 8.8% on SWE-bench) and named the exact problem ctx attacks: when the compactor drops information the agent later needs, task outcomes silently degrade, with no error signal at the moment of compaction.
- ProcCtrlBench formalized "control preservation" and "context window thrashing" as measurable process defects.

## The wave and the gap

The wave: outcome beats ratio is now consensus. There is momentum to ride.

The gap nobody has filled: everyone measures outcome synthetically (LLM judges, probes, benchmarks, offline eval). ctx's behavioral join, the user's own corrections, re-reads, and aborts in real sessions, is a different and cheaper ground truth. Not "a judge thinks the summary looks fine" but "the human did not have to fix it." And ctx is agent-agnostic via the surface adapters, where Factory is its own agent and the natives only see themselves.

## ctx's defensible moat (and what is not)

Real moat:
1. Production behavioral outcomes as the label, not a judge. Hardest to game, cheapest to collect, per-user and per-repo.
2. Fail-closed, evidence-gated activation. We only compress what the user's own data proved safe. This is now honest: windowed join, enforced AUC gate, surface provenance.
3. Cross-agent normalization. One controller that learns across Claude Code, Cursor, Codex.

Not a moat, to be honest: the compression transforms themselves (commoditized), and any single-agent integration. And the moat is currently unproven because we just reset to zero labels and the real correction signal looks sparse. The day-one story is weak. That is the central risk.

## The reposition

Product equals measurement plus governance plus cross-agent learning. Compression becomes one governed action among many, not the headline.

One-line pitch: ctx proves whether your agents' context decisions are costing you corrections, in your real work, and only changes what it can prove is safe.

## Roadmap (three horizons, evidence-gated)

### Horizon 1: earn credibility on the loop (now)
- Accrue real labels. Enrich the outcome signal beyond short-turn corrections: explicit re-reads, aborts, immediate re-edit of a file the agent just touched, undo or revert language. Sparse correction data is the number one risk, and more signal types fix it.
- Add reversible compression (a rewind store, like Claw Compactor): the agent can re-expand a compressed block on demand. This makes compression safe by construction and fits the fail-closed ethos.
- Ship the honest before and after on exactly one tool (Bash is already ready): turn it on deliberately, measure whether the windowed correction rate moves. That single proven result is worth more than any ratio claim.

### Horizon 2: the wedge nobody else can build
- Measure the native agent's own compaction harm using the same behavioral join: "your Claude/Cursor/Codex compaction preceded N corrections this week." Factory cannot (they are an agent), Braintrust cannot (offline), the natives will not (they grade their own homework). This reframes the entire Tier 1 threat as something ctx measures and governs.
- Cross-surface dashboard: one honest view of context cost and correction impact across all your agents.

### Horizon 3: platform
- Per-repo learned context policy as a portable, team-shareable artifact.
- A context truth score and API that plugs into the eval stack (Langfuse, Braintrust) as a production-behavior signal rather than competing with them.

## What we will explicitly NOT do
- Chase compression-ratio or ROUGE benchmarks.
- Build a memory layer (Mem0, Zep, Letta own it).
- Build general observability (Langfuse, Braintrust own it).
- Build an agent runtime (Letta).

## Risks and honest kill criteria
- Signal sparsity (highest risk): if after real usage the behavioral signal cannot separate good from bad interventions (cannot clear the AUC gate on real data), the self-learning thesis fails. Fallback that still wins: the measurement-only compaction harm detector (Horizon 2), which needs far less signal to be valuable.
- The natives close the gap: Slipstream-style trajectory validation will get productized. Speed and the agent-agnostic angle are the defense.
- Adoption friction: value only appears after labels accrue. Mitigate by leading with the measurement story (works immediately) before the learning story (needs volume).
