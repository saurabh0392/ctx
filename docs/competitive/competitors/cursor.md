# Cursor (native context management)

- Category: platform-native compaction
- Classification: direct competitor (native, and a surface ctx supports)
- One-liner: Cursor's built-in conversation summarization plus a context-usage breakdown, all server-side and automatic.
- URL / repo / docs: https://cursor.com/docs/agent/prompting
- Maturity signals: shipped and default. The context-usage breakdown ("context ring") landed in Cursor 3.3 (Cursor changelog / forum, accessed 2026-06-13). Summarization has been on "for a very long time" per Cursor staff; only recently surfaced to the user (Cursor forum, accessed 2026-06-13). Backed by Anysphere (Cursor); well funded.
- License / pricing: bundled into Cursor. The user pays for usage; context management is a free built-in feature.

## What it does and how (mechanism)

When a conversation nears the model's context window, Cursor compresses older turns into a summary so the model keeps a high-level understanding while continuing. It is automatic; `/summarize` can trigger it manually. Cursor explicitly chose summarization over truncation or hard stops (Cursor forum, staff post, accessed 2026-06-13).

The context ring shows a breakdown by bucket: system prompt, tools, rules, skills, MCP catalog, subagents, summarized conversation, and conversation (messages, replies, tool calls and their results, file contents, search results). User control is limited: core tools (Read, Edit, Grep, Shell, Task) cannot be turned off; MCP servers and a few extras can be toggled.

Where it sits: server-side, inside Cursor's own backend. Cursor sends model calls to its backend, not directly to `api.anthropic.com`, so an Anthropic-scoped MITM proxy never sees Cursor traffic (this is why ctx covers Cursor through transcript ingest, not a live hook; see ADR 0011).

## Claimed results vs verifiable results

- Claimed (vendor): each summarization release is "our best yet, keeping the nuance of what you've worked on so far, but condensing it down." Source: Cursor forum staff post, accessed 2026-06-13. Label: vendor claim.
- Verifiable: the buckets and the summarization behavior are documented (observed from Cursor docs and forum, 2026-06-13).
- Community-observed (not vendor): quality degrades after roughly 20-30 exchanges and the agent re-reads files, per third-party guides. Source: developertoolkit.ai, accessed 2026-06-13. Label: third-party estimate, not a controlled measurement.

## Strengths

- Default, free, zero-install, native to the IDE most ctx users already run.
- The context ring is a genuinely good honesty surface: it shows where tokens go, including the MCP catalog. This is a UX bar ctx should match.
- It manages the whole conversation, not just tool outputs.

## Weaknesses and blind spots

- Summary-based compression loses detail. Users report having to re-explain context after a summarize, and the agent forgetting early instructions (Cursor forum and third-party guides, accessed 2026-06-13).
- No per-tool-output trimming exposed. The MCP catalog and tool results sit in the conversation bucket; the only lever is summarize or start a new chat.
- No proof. Cursor does not tell you whether a summarization hurt your task. Same self-grading gap as Claude.
- Limited user control over what gets condensed.
- The MCP catalog is a named, visible cost in the breakdown, which is exactly the prefix-token problem ctx's profile filtering targets, and Cursor offers only a coarse on/off per server.

## Overlap with ctx

Cursor summarizes conversation; ctx trims tool outputs and filters MCP schemas. Different mechanisms, same buyer pain (context-window pollution and token cost). ctx is the only one of the two that measures correction impact and that can strip individual MCP tool schemas rather than whole servers.

## Where ctx is better / where it is worse

Better:
- Per-tool-output trimming and per-MCP-tool schema filtering, finer than Cursor's whole-server toggle.
- Proof on the user's own behavior; Cursor offers none.
- The edit-intent guard now runs on Cursor too (ADR 0011, Part A), protecting a read the agent declared it will edit.

Worse:
- ctx has no live hook on Cursor. It relies on transcript ingest, so its Cursor labels are lower-confidence and its action timing is weaker than on Claude Code (roadmap E2.2).
- Cursor is the platform. It can ship its own per-tool trimming or an MCP schema pruner at any time and reach every user with no install.

## Threat level to ctx: high

Cursor is both a key surface for ctx and a competitor that can absorb the feature. If Cursor adds per-tool-output trimming or MCP schema pruning natively, ctx's value on Cursor narrows to proof and cross-agent coverage. Cursor's lack of a live third-party hook also caps how well ctx can act there today.

## What ctx should learn or steal

- The context ring. ctx's dashboard should show a clear, honest breakdown of where tokens go, including the MCP catalog and tool results, at least as legible as Cursor's.
- Surface MCP catalog cost prominently; it is the wedge for profile filtering and Cursor has trained users to look at it.
- Push for a real Cursor PostToolUse hook or richer transcript signal so ctx can act, not just observe (roadmap E2.2).
