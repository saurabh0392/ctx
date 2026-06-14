# 0013. Competitive position: do not race RTK on Bash; win on the surfaces and the proof others skip

- Status: accepted
- Date: 2026-06-13
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-24 (competitive analysis epic)
- Extends: strategy-context-truth-layer.md, ADR 0006 (earned savings), ADR 0012 (auto burn-in)
- Source: docs/competitive/ (full briefs and synthesis), competitive research accessed 2026-06-13

## Context

A competitive pass across the whole space (docs/competitive/) forces a positioning decision that should not live buried in a competitor brief.

The facts that drive it:

- RTK (Rust Token Killer) owns the "token savings" framing on Bash. 62,127 GitHub stars, Apache-2.0, `brew install rtk`, 100+ command-aware filters, 14 agents (observed via GitHub API and the RTK repo, 2026-06-13). On terminal commands it is genuinely good and well distributed.
- RTK's hook fires only on Bash tool calls. By RTK's own documentation, native `Read`, `Grep`, `Glob`, and MCP tool results bypass it. Those are exactly where Cursor and default Claude Code spend tokens.
- The platforms (Claude Code compaction and context editing, Cursor summarization, Codex auto-compaction, Gemini CLI) all ship native context management for free, server-side, single-vendor, and they grade their own work. None measures, from the user's real corrections, whether a context intervention helped or hurt.
- The compression proxies and libraries (Kompact, Claw Compactor, LLMLingua) and the MCP gateways all measure quality synthetically (ROUGE, BFCL, benchmarks) and none proves safety on the user's own behavior.

If ctx leads with "we save tokens too," it walks into RTK's frame on Bash (where RTK is better and better known) and into the platforms' frame on compaction (where they are free and default). Both are losing fights.

## Decision

ctx competes on two axes RTK and the platforms structurally do not own, and treats raw token savings as table stakes rather than the headline.

1. Surfaces. ctx's lead surface is native tool output and MCP results: `Read`, `Grep`, `Glob`, and MCP tool outputs trimmed after the fact (PostToolUse), plus MCP tool-schema filtering. This is RTK's open lane, because RTK's PreToolUse command rewrite cannot reach native tool results without the user re-routing work through the shell.

2. Proof. ctx only trims a tool "for real" after it has shown, on the user's own sessions, that trimming did not raise re-reads or corrections (the causal gate, ADR 0006 and 0012). No competitor in the scan does per-user causal proof. This is the honest, defensible claim.

Consequences for how we build and talk:

- Do not invest in beating RTK on Bash command-aware filtering as a headline. We will still improve Bash trimming (borrow structure-aware filters), but Bash is a supporting surface, not the pitch.
- Do not position ctx as a compactor or compete with native compaction on its own terms. Reframe native compaction as something ctx measures: the compaction-harm detector (roadmap E2.1) is pulled toward the front as the most defensible feature.
- Treat MCP schema filtering as a supporting feature, not the lead. The MCP-gateway category is commoditizing and is already being absorbed by Claude Code's native search-first discovery.
- Treat prompt caching as a design constraint, not a competitor: ctx must not bust the cache, and we say "cache the prefix, trim the tool output, prove it was safe."
- The one-line position becomes: ctx is the context truth and safety layer for AI coding agents. It trims the tool output and MCP noise the platform and RTK miss, on Claude Code and Cursor, and only after it proves, on your own work, that trimming did not cost you corrections.

## What this deliberately does not do

- It does not drop Bash trimming. It de-prioritizes Bash as the marketing frame.
- It does not claim proof we do not have. Today's live numbers (about 343K tokens saved, about 0% corrections, about 5% re-reads on verifiable trimmed calls, on the developer's own machine) are early and single-machine. We label them observed-but-early and never present them as a general result.
- It does not commit to a Codex or Gemini surface yet. Those remain gated on real usage (roadmap E2.3).
- It does not turn ctx into observability, memory, or a repo packer. Those are other people's categories (strategy doc).

## Alternatives considered

- Compete with RTK head-on on token savings and Bash. Rejected: RTK is better on Bash and far better distributed. We would lose the frame.
- Lead with MCP schema filtering. Rejected: crowded, commoditizing, being absorbed by the platforms; thin as a standalone wedge.
- Lead with a general compression proxy (Kompact-style full-prompt MITM). Rejected: heavy trust cost, fragile against wire changes, and it still would not prove safety on real behavior. ctx already reverted a proxy capability for trust reasons (ADR 0011).
- Position as an eval/observability product. Rejected: Langfuse and Braintrust own it; ctx should feed that stack, not compete (roadmap E3.3).

## Consequences

- Clear, honest narrative that does not pick fights ctx loses. The pitch points at a real gap (native tool output and MCP, with proof) that the incumbents do not cover.
- Higher bar on the proof story. The whole position rests on the causal gate producing trustworthy results on real, sparse data. If the signal stays too sparse to clear the gate (the top kill risk in the strategy doc), the fallback is the measurement-only compaction-harm detector, which needs far less data and still differentiates.
- Platform absorption remains the existential threat. The defenses are speed, cross-agent coverage (a single vendor cannot measure its rivals), and the credibility of proof a vendor will not run against itself. Stated plainly in docs/competitive/60-threats-and-moats.md.
- This ADR is referenced from docs/competitive/20-positioning.md, which carries the customer-facing version.
