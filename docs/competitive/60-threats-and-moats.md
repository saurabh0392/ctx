# Threats and moats: the brutal version

Last updated: 2026-06-13. This is the section a skeptical exec turns to first. It is deliberately hard on ctx.

## The one threat that matters: platform absorption

Be honest: the most likely way ctx dies is not a competitor out-executing it. It is Anthropic or Cursor shipping "good enough" versions of ctx's features for free, server-side, to every user, with no install.

The evidence this is already happening:
- Claude Code ships compaction (`compact_20260112`), context editing that clears stale tool results (`clear_tool_uses_20250919`), and cache-aware microcompact. That is server-side tool-output reduction, the core of what ctx does, already in the product.
- Cursor ships automatic summarization and a context-usage breakdown (the context ring, Cursor 3.3) that even itemizes the MCP catalog, the exact cost ctx's profile filtering targets.
- Claude Code already does search-first MCP tool discovery natively (StackOne, 2026-06-13), absorbing the MCP-gateway category.
- The research frontier (trajectory-grounded compaction validation) points straight at "validate that compaction did not hurt," which is adjacent to ctx's proof claim. When a platform productizes that, it competes with ctx's headline on that platform.

What absorption does to each ctx feature:
- Tool-output trimming: the platforms can do this server-side, coarsely, today. ctx's edge is per-tool earning, content-awareness, and proof, not the act of trimming.
- MCP schema filtering: most exposed. Being absorbed now. Do not lead with it.
- Edit-intent guard: a platform could add this trivially if it chose to. It is a nice feature, not a moat.
- Proof / measurement: the hardest for a platform to copy honestly, for a structural reason below.

## What is actually defensible (and what is not)

Not defensible (say so plainly):
- The compression transforms. Commoditized. RTK, Claw Compactor, LLMLingua, and the platforms all have them. ctx's heuristics are behind RTK's filters on Bash.
- Any single-agent integration. A hook on one agent is copyable and absorbable.
- MCP schema filtering. Crowded and being absorbed.
- "We save tokens." Everyone says this. It is table stakes.

Genuinely defensible (the moat, such as it is):
1. Production behavioral outcomes as the label. ctx's signal is the user's own corrections, re-reads, and aborts in real sessions, not a judge, a benchmark, or an offline eval. This is the hardest to game (the human really did or did not have to fix it), the cheapest to collect (it is already in the transcript), and per-user and per-repo. No competitor collects it.
2. Cross-agent normalization. One controller that learns across Claude Code, Cursor, and more. A platform structurally cannot do this: Anthropic will not measure Cursor; Cursor will not measure Codex. Only a neutral local layer sees across agents.
3. The credibility of measuring a vendor against itself. The single most defensible product idea here is the compaction-harm detector: "your Claude (or Cursor) compaction preceded N corrections this week." A vendor will never ship an honest measurement that makes its own compaction look bad. ctx can, because it is neutral and local. This reframes the biggest threat into ctx's most defensible feature.

The uncomfortable truth: the moat is real in shape but unproven in fact. It depends on the behavioral signal being trustworthy on real, sparse data. The strategy doc names signal sparsity as the number one kill risk, and the live corpus was recently reset to zero labels. The moat is a thesis with one machine of evidence behind it, not a demonstrated fact. An exec should treat it as promising and unverified.

## Why a platform probably will not build the moat (the structural argument)

- Neutrality. A platform measuring its own compaction harm is a conflict of interest. It will not publish "our compaction made you correct 30 times this week." A neutral third party can.
- Cross-agent. The value of one honest view across Claude Code, Cursor, and Codex only exists for a tool that is not any of them. No platform will measure its rivals.
- Trust posture. ctx's no-telemetry, no-account, local-first stance is a deliberate trust position. Platforms run server-side and have business reasons to keep data. ctx's posture is a wedge precisely because the incumbents cannot easily match it.

These are real structural advantages. They are not permanent. A neutral, well-funded entrant could occupy the same seat.

## The honest threat ranking

| Threat | Likelihood | Impact | ctx's defense |
| --- | --- | --- | --- |
| Platform absorbs trimming / compaction-validation | High | High | Speed, cross-agent, measure-the-platform, trust posture |
| Signal stays too sparse to prove the moat | Medium to high | High (kills the self-learning thesis) | Fallback to measurement-only compaction-harm detector (less data needed) |
| RTK closes the native-tool / MCP lane | Medium | High | RTK's pre-tool architecture does not extend there naturally; out-build before they pivot |
| MCP filtering commoditized away | High | Low (not the headline) | Already de-prioritized (ADR 0013) |
| Prompt caching busted by ctx, savings erased | Medium | Medium | Cache-safety as a design rule; verify and fix (open question) |
| Funded entrant polishes the proxy pattern with proof | Low to medium | Medium to high | Move first on production-behavior proof |

## What an exec should take away

ctx is attacking a real, open seat (neutral, cross-agent, production-behavior proof) that the incumbents have structural reasons not to take. That is the bull case. The bear case is that the moat is one machine of evidence deep, the platforms are shipping adjacent features for free every month, and the whole thesis fails if the behavioral signal never gets dense enough to trust. The right posture is urgency: prove the loop on real data fast, ship the compaction-harm detector as the hedge, and never claim the moat is proven before it is.
