# Open questions and what needs a spike

Last updated: 2026-06-13. What this analysis could not verify, and what needs primary research or an engineering spike before it can be trusted or claimed.

## Could not verify in this pass (needs primary confirmation)

1. RTK's downstream task quality. RTK publishes per-command savings (60-90%) but no benchmark showing filtered output did not cause the agent to re-run or err. Needed: an independent test of task completion with RTK on vs off. Until then, RTK's savings are output reduction, not net tokens-to-complete-task.
2. Codex compaction ratio. The strategy doc cites about 99.3% for Codex (and OpenAI generally). Not re-derived from a primary source this pass. Treat as internal estimate.
3. Langfuse / ClickHouse acquisition (January 2026) and Helicone maintenance-mode-after-acquisition. Both come from secondary sources (strategy doc and one 2026 comparison). Confirm against primary announcements before citing in anything external.
4. Star counts and funding for Letta and Zep were taken from third-party 2026 articles, not re-verified via API this pass. Mem0 (58,492), RTK (62,127), Codex (90,868), LLMLingua (6,287), Repomix (26,245), code2prompt (7,405) were verified via GitHub API on 2026-06-13.
5. Claw Compactor adoption (stars, real users). The high version number (v7.x) with low public visibility is unexplained. Confirm whether it has real usage or is a fast-iterating solo project.
6. The "compression-proxy cluster" names from the strategy doc: Kompress / Headroom (ModernBERT drop-in for LLMLingua) and token-compressor (MCP server with embedding-similarity gate). Kompact and Claw Compactor were verified; the others were not independently confirmed this pass.
7. Research-frontier citations in the strategy doc: Factory.ai's probe-based eval and the "99.3% compression scores lower" quote, the ICAE/SWE-bench "79 fewer issues" result, Slipstream's trajectory-grounded validation (+2.6 to 8.8% on SWE-bench), and ProcCtrlBench. These shape ctx's thesis and are cited as internal-strategy facts. They were not re-fetched from primary papers in this pass. Confirm before using any of them in external material.

## Engineering spikes ctx needs (product-side, not this task)

1. Cache-safety (highest). Does ctx trimming, and especially MCP schema filtering, bust the prompt cache? MCP tool definitions usually sit in the cached prefix; if anything changes that prefix it can force full-price reprocessing and erase the savings. First pass done in `docs/cache-safety-spike.md` (CTX-28). Key finding: ctx never edits a request on the wire (the MITM proxy was removed in CTX-29 / ADR 0015), so it never rewrites the cached `tools` block. Tool filtering shrinks the prefix through Claude Code's own settings, tool-output trimming lands after the prefix, and the one remaining prefix edit is system injection. The real risk is an oscillating prefix, not editing per se. The read-only `ctx context cache-audit` command measures this from real traffic, and the live profile A/B shows filtering on with higher cache-read share and ~14% lower cost than off. Still open: a controlled A/B on system injection (now collecting a control arm at `inject_pct = 50`) to put a signed number on its net effect.
2. Signal density. After the recent reset to zero labels, can the causal gate accrue enough trustworthy positives (corrections, re-reads, aborts) on real usage to clear the honesty thresholds? This is the top kill risk in the strategy doc. Spike: instrument and report time-to-first-earned-tool on real installs.
3. Compaction-harm detector feasibility. Can ctx reliably detect native compaction events (Claude exposes a `pre_compact` flag) and join the corrections that follow, per surface, with enough precision to make the claim "your compaction preceded N corrections this week"? This is the strategic hedge; validate it can be built with far less label volume.
4. Cursor live hook. Is a real Cursor PostToolUse hook possible, or is transcript ingest the ceiling? This caps how well ctx can act (not just observe) on Cursor.

## Strategy questions for the next review

1. Is the team/enterprise measurement layer (40-pricing) the right monetization, and when do we start building toward it vs staying purely OSS land-grab?
2. Do we pull the compaction-harm detector fully forward as the lead feature (it is the most defensible and least data-hungry), or prove the per-tool loop first? ADR 0012 and the roadmap leave this open; the competitive picture argues for pulling it forward.
3. How do we credibly benchmark against RTK without entering its Bash frame? A fair head-to-head would measure net tokens-to-complete-task on native-tool-heavy and MCP-heavy workflows, where RTK does not act. Worth designing.
4. What is the minimum honest external claim for a public launch, given the proof is one machine deep? 20-positioning.md proposes the structural-gap framing plus labeled early numbers; validate that is enough to land.

## Method notes (so this can be redone)

- Adoption and license facts marked observed were pulled from the GitHub API or primary repo pages on 2026-06-13.
- Mechanism facts were taken from vendor docs, repo READMEs, and (for Codex and Gemini CLI internals) maintainer comments and third-party source analysis, all accessed 2026-06-13.
- Anything from ctx's own docs/strategy-context-truth-layer.md is internal and is labeled as such; it is not independent verification.
- Full source list in sources.md.
