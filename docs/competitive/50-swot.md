# SWOT

Last updated: 2026-06-13. Grounded in the briefs (competitors/) and the matrix (10-comparison-matrix.md). Every claim here traces to one of those.

## Strengths

- The open surface. ctx trims native Read, Grep, Glob, and MCP tool outputs. RTK's hook is Bash-only by its own docs; the platforms only clear results coarsely and server-side. This is a real, verified gap ctx fills.
- Per-user causal proof. ctx only trims a tool after showing, on the user's own sessions, that trimming did not raise re-reads or corrections. No competitor in the scan does this. It is the honest differentiator.
- Trust posture. Local SQLite, no telemetry, no account, hook-first with no proxy and no CA (ctx never terminates TLS or edits the request on the wire). Stronger than RTK (opt-in telemetry), the platforms (server-side), and the proxies (full-API MITM).
- Cross-agent design. One controller normalizing Claude Code and Cursor, with adapters for more. Every native competitor sees only itself.
- Edit-intent guard. ctx will not trim a read the agent declared it will edit, on both Claude Code and Cursor (ADR 0011). A protective feature no competitor has, because they do not trim native reads.
- Honest, designed dashboard with real empty states (no placeholder counts), which matches the buyer pain around trust.

## Weaknesses

- Distribution and mindshare. RTK has 62,127 stars and the category framing; ctx has one-machine evidence. This is the biggest near-term gap.
- The proof is early and sparse. The live result (about 343K tokens, about 0% corrections, about 5% re-reads) is one machine. The whole position rests on the causal signal being trustworthy on real, sparse data, and the strategy doc names signal sparsity as the top kill risk.
- Bash trimming is generic. RTK's 100+ hand-written command-aware filters beat ctx's heuristic trim on shell output.
- No live hook on Cursor (transcript ingest only), so Cursor labels are lower-confidence and action timing is weaker (roadmap E2.2).
- No Codex or Gemini surface yet.
- Third-party install vs free first-party features. Higher friction than "it is already on."
- No team or enterprise governance features yet (RBAC, audit), which the MCP gateways already offer and enterprise buyers expect.

## Opportunities

- Own "outcome over ratio" before the field does. The research and product frontier is moving from compression ratio to tokens-to-complete-task. ctx's production-behavior proof is the cheapest, hardest-to-game version of that. Plant the flag.
- The compaction-harm detector (roadmap E2.1). Turn the biggest threat (native compaction) into ctx's most defensible feature: measure the corrections that follow a platform's own compaction, which no vendor will do against itself.
- Team and enterprise measurement layer. A cross-agent, honest view of context cost and correction impact is something teams will pay for (per the pricing pattern in 40-).
- Portable per-repo policy as a shareable artifact (roadmap E3.1), rhyming with Mem0's "memory passport" framing.
- Feed the eval stack (Langfuse, Braintrust) with a production-behavior signal (roadmap E3.3), becoming a complement rather than a competitor.
- Borrow, do not rebuild: structure-aware filters (RTK, Claw Compactor), reversible compression (Claw Compactor), LLMLingua-2 for prose-heavy blobs.

## Threats

- Platform absorption (existential). Anthropic or Cursor shipping per-tool trimming, MCP schema pruning, or trajectory-grounded compaction validation natively would narrow ctx's claim on those surfaces to proof and cross-agent coverage. Detailed in 60-threats-and-moats.md.
- RTK closing the lane. If RTK adds a credible native-Read and MCP-output story, it covers ctx's primary surface gap with far more distribution. Its PreToolUse architecture does not extend there naturally, which is the main reason this has not happened yet.
- Commoditization of MCP filtering. The MCP-gateway category is consolidating and being absorbed (Claude Code does search-first natively). ctx's MCP profile feature is the most exposed.
- Prompt caching as a constraint. If ctx busts the prompt cache it can cost more than it saves. CTX-28 narrowed this: ctx never edits the request on the wire (the proxy was removed, ADR 0015), so the only cached-prefix change it causes is system injection. Live data shows filtering on with higher cache-read share and lower cost. Remaining risk is an oscillating injected prefix, logged in 90-open-questions.md.
- The signal never matures. If the causal gate cannot clear on real, sparse data, the self-learning thesis fails. Mitigation: the measurement-only compaction-harm detector still wins with far less data (strategy doc kill criteria).
- A funded entrant polishes the compression-proxy pattern (Kompact-style) with real distribution and adds proof. Possible but not yet present.

## The SWOT in one line

ctx's strengths (open surface, proof, trust, cross-agent) line up almost exactly against the field's blind spots, but its weaknesses (distribution, sparse early proof) and its single biggest threat (platform absorption) mean the whole thing rides on moving fast and making the proof real before a platform makes it free.
