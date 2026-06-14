# Claude Code (native compaction and context editing)

- Category: platform-native compaction
- Classification: direct competitor (highest absorption risk)
- One-liner: Anthropic's own context management built into Claude Code and the Claude API: server-side compaction, context editing that clears stale tool results, and microcompact.
- URL / repo / docs: https://code.claude.com/docs/en/context-window, https://platform.claude.com/docs/en/build-with-claude/compaction, https://platform.claude.com/docs/en/build-with-claude/context-editing
- Maturity signals: shipped and default in Claude Code. Server-side compaction is in beta behind the `compact-2026-01-12` header (Anthropic docs, accessed 2026-06-13). Context editing strategies `clear_tool_uses_20250919` and `clear_thinking_20251015` are documented and live. Backed by Anthropic; effectively unlimited resources.
- License / pricing: bundled into Claude Code and the Claude API. Free to the user as a feature; the user pays for tokens. No separate charge.

## What it does and how (mechanism)

Three layers, all native:

1. Compaction (`compact_20260112`). When a conversation approaches a token threshold (default 150,000 tokens, configurable, minimum 50,000), the API summarizes older history into a structured summary and continues from there. Runs server-side. Also exposed in Claude Code as `/compact`, with optional focus instructions. This is summary-based: it replaces history with a generated summary.

2. Context editing (`clear_tool_uses_20250919`). Clears old tool results (file contents, search results) from history once Claude has processed them, server-side, before the prompt reaches the model. The client keeps the full unmodified history; only what the model sees is trimmed. A separate strategy clears thinking blocks. This is the native analog closest to ctx's tool-output trimming, but it is threshold-and-age based (clear stale results when context grows), not a per-result decision made when the result arrives.

3. Microcompact (Claude Code internal). A lightweight first layer that surgically clears stale tool results and replaces them with placeholders like "[Old tool result content cleared]". Two triggers: time-based (server cache expired) and cached microcompact (uses cache-editing API to drop tool results without invalidating the cached prefix). Source: Claude Code source reference and docs, accessed 2026-06-13.

Where it sits: server-side, inside the model provider. It sees only Claude Code (or whatever client calls the Claude API with these flags). It is single-vendor by construction.

## Claimed results vs verifiable results

- Claimed (vendor): compaction "extends the effective context length" and "keeps the active context focused and performant." Context editing improves model focus by removing irrelevant content. No public per-task accuracy delta is attached to these claims. Source: Anthropic docs, accessed 2026-06-13. Label: vendor claim.
- Verifiable: the mechanisms, triggers, thresholds, and beta headers are documented primary facts (observed from Anthropic docs and the Claude Code source reference, 2026-06-13).
- Honest gap relevant to ctx: Anthropic grades its own compaction. There is no native signal that says "this compaction event preceded N corrections in your real work." That measurement gap is the ctx wedge (see strategy doc and 60-threats-and-moats.md).

## Strengths

- It is free, default, server-side, and improving fast. Most users never install anything.
- Cache-aware. Cached microcompact drops tool results without busting the prompt cache, so it is cheap.
- Deep integration: it sees the true token state and the model's own behavior, and can act with no hook latency.
- Trust: it is first-party. No third-party MITM, no extra binary.

## Weaknesses and blind spots

- It grades its own homework. When compaction or context editing drops something the agent later needs, the failure is silent. There is no native, production-behavior signal that ties a compaction event to a later correction or re-read.
- It is reactive and coarse. Compaction fires near a threshold and summarizes in bulk. Context editing clears by age. Neither is a per-tool, earned decision tuned to your repo.
- Single-vendor. It only manages Claude. A developer using Cursor (its own backend), Codex, and Claude Code gets three different opaque strategies and no unified view.
- Summary-based compaction can lose nuance (well documented in user reports for the equivalent Cursor behavior; same risk class here).

## Overlap with ctx

Partial and important. Context editing and microcompact both reduce tool-result tokens, which is ctx's core action. But they trigger differently (threshold/age vs per-result) and they do not prove safety on the user's own behavior. ctx's tool-output trimming and Anthropic's tool-result clearing aim at the same tokens by different means.

## Where ctx is better / where it is worse

Better:
- Cross-agent. ctx normalizes across Claude Code, Cursor, and more. Anthropic only sees itself.
- Proof. ctx ties interventions to the user's own corrections and re-reads. Anthropic does not surface this.
- Per-tool, earned, fail-closed activation rather than bulk threshold summarization.
- The measurement angle ctx can build: detect native compaction events and count the corrections that follow ("your Claude compaction preceded N corrections this week"). Anthropic will not ship this against itself.

Worse:
- Free, default, zero-install, server-side, first-party trust. ctx is a third-party local layer the user must install.
- Anthropic can change the wire format or absorb any single feature instantly. ctx rides on top of a platform that can move under it.

## Threat level to ctx: high

This is the absorption risk in person. If Anthropic ships trajectory-grounded compaction validation (the research frontier in the strategy doc) or per-session quality signals, it narrows ctx's distinct claim on Claude Code. The defenses are speed, the cross-agent angle (Anthropic cannot measure Cursor or Codex), and the honesty angle (a vendor will not credibly grade its own compaction for you).

## What ctx should learn or steal

- Cache-aware clearing. Anthropic's cached microcompact drops tool results without invalidating the prompt cache. ctx must make sure its trims do not needlessly bust the cache and erase the savings (see prompt-caching brief and 70-gtm-implications.md).
- Reframe Tier 1 as something ctx measures, not competes with. The compaction-harm detector (roadmap E2.1) turns the biggest threat into ctx's most defensible feature.
- Do not try to out-compact the platform. Lead with proof and cross-agent coverage.
