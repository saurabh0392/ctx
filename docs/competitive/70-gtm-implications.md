# Go-to-market implications

Last updated: 2026-06-13. Positioning, narrative, naming, launch surfaces, partnerships, and what not to do. This also collects the concrete product implications the research surfaced (the task did not change product code; these become follow-up tickets).

## Positioning and narrative (the short version)

Lead with the open lane and the proof, never with a savings percentage. The narrative is: "RTK and the platform handle the obvious stuff. They miss your native file reads, your searches, and your MCP tool outputs, and none of them prove they did not break your agent. ctx does both." Full version in 20-positioning.md, decision in ADR 0013.

The three pillars again, because GTM should repeat them everywhere: the open lane (native tools and MCP), earned not assumed (the causal gate), and yours and local and honest (no account, no telemetry).

## Naming

- Keep "ctx." It is short, lowercase, developer-native, and does not box ctx into "compressor." It supports the reposition to a truth-and-safety layer.
- Avoid sub-naming features as "compression." Call the action "trimming" and the product a "context truth and safety layer." Words matter here: "compressor" puts ctx in RTK's and the proxies' frame, where it loses.
- Do not pick a fight name. No "token killer" echo. ctx's story is honesty, not body count.

## Launch surfaces

- Primary: GitHub (README that leads with the open lane and proof, a one-line install, a single honest screenshot of the dashboard with real numbers and real empty states). This is where RTK won and where the solo AI-heavy developer lives.
- Secondary: the Claude Code and Cursor communities (forums, Discords) where context decay and token cost are actively discussed. Show up with the specific gap (native tools and MCP), not a generic pitch.
- Demo proof, carefully: the 343K-tokens / about 0% corrections / about 5% re-reads result, always labeled observed, early, single-machine. Pair it with the dashboard view that shows tools in watching, learning, and earned states (ADR 0012), so the honesty is visible, not asserted.
- Content: one sharp post on "the surface RTK misses" with RTK's own docs as the citation, and one on "why we measure corrections, not ROUGE." Both ride existing waves (RTK's popularity, outcome-over-ratio).

## Partnerships

- Eval stack (Langfuse, Braintrust): integrate, do not compete. Emit ctx's production-behavior signal in OpenTelemetry form so it lands in their dashboards (roadmap E3.3). This makes ctx a complement and borrows their distribution and credibility.
- Repo packers and memory layers (Repomix, Aider, Mem0): position as complementary in docs and examples. "Use Repomix to seed context, Mem0 for cross-session memory, ctx to keep the loop lean and honest." No conflict, shared buyer.
- RTK: do not partner, do not attack. Acknowledge it is good on Bash; point at the lane it leaves open. Respectful differentiation reads as confidence and is honest.

## Concrete product implications surfaced by the research (for follow-up tickets, not this task)

1. Cache-safety verification (high priority). Confirm ctx trimming and especially MCP schema filtering do not bust the prompt cache. If schema filtering edits the cached prefix, it can erase savings. Adopt the cache-aware pattern Claude Code's microcompact uses (edit after the cached prefix). See prompt-caching brief and 90-open-questions.md.
2. Structure-aware Bash and Read trimming (medium). Borrow command-aware filters (RTK) and content-type-aware stages (Claw Compactor) for git, test runners, linters, and code reads, instead of generic heuristics.
3. Pull the compaction-harm detector forward (high, strategic). Roadmap E2.1. It is the hedge against signal sparsity and the most defensible feature. ADR 0013 already leans this way.
4. Reversible compression / rewind store (medium). Roadmap E1.2, validated by Claw Compactor's design. Makes trimming safe by construction and is a talking point.
5. Dashboard parity with Cursor's context ring (medium). Show a clear token breakdown including MCP catalog and tool results. Cursor has trained users to expect this.
6. Real Cursor PostToolUse hook (medium). Today ctx only ingests Cursor transcripts; a live hook would let it act, not just observe (roadmap E2.2).

## What NOT to do

- Do not lead with a savings percentage. It is RTK's frame and ctx has not earned a general number.
- Do not call ctx a compressor. It reframes the product into the losing category.
- Do not compete with native compaction on its own terms ("we compact too"). Measure it instead.
- Do not make MCP schema filtering the headline. It is commoditizing and being absorbed.
- Do not build a MITM proxy. ctx removed its proxy entirely (ADR 0015) and is hook-first: no TLS termination, no CA, no editing the request on the wire. "We never touch your traffic" is a trust advantage, so keep it.
- Do not introduce telemetry to learn or to monetize. The no-telemetry posture is a differentiator against the entire field.
- Do not claim the moat is proven. It is one machine of evidence. Say "early" every time, or lose the trust the whole product is built on.
- Do not build Codex or Gemini surfaces before there is real usage to justify them (roadmap E2.3).
