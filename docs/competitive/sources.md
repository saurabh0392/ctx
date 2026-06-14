# Sources

Last updated: 2026-06-13. Every source used, with what it supports, the date accessed, and a confidence note. Confidence: high (primary, observed), medium (vendor doc or maintainer statement), low (secondary blog or third-party stat).

## Verified via GitHub API on 2026-06-13 (confidence: high)

| Repo | Fact | Value |
| --- | --- | --- |
| rtk-ai/rtk | stars, license, last push | 62,127 stars, Apache-2.0, pushed 2026-06-12 |
| openai/codex | stars, license | 90,868 stars, Apache-2.0 |
| microsoft/LLMLingua | stars, license, last push | 6,287 stars, MIT, pushed 2026-04-08 (no release since v0.2.2, 2024-04-09) |
| mem0ai/mem0 | stars, license | 58,492 stars, Apache-2.0 |
| yamadashy/repomix | stars, license | 26,245 stars, MIT |
| mufeedvh/code2prompt | stars, license | 7,405 stars, MIT |
| npow/kompact | stars, license, dates | 2 stars, MIT, created 2026-02-22, v0.3.0 2026-03-21 |

## RTK

- https://github.com/rtk-ai/rtk (README, accessed 2026-06-13). Mechanism, Bash-only scope, 100+ commands, 14 agents, tee recovery, v0.28.2, opt-in telemetry, Apache-2.0. Confidence: high (primary repo).
- https://github.com/rtk-ai/rtk/blob/master/hooks/README.md (accessed 2026-06-13). Hook mechanism per agent, "can modify command" table. Confidence: high.
- https://github.com/rtk-ai/rtk/blob/master/INSTALL.md (accessed 2026-06-13). Install modes. Confidence: high.
- https://piedpay.medium.com/oh-my-zsh-im-so-slow-me-then-let-s-break-up-d30cff532f70 (Medium, April 2026). Early star count (~28K) used only to show growth rate. Confidence: low (blog; it incorrectly states MIT license, which the repo contradicts).

## Platform-native compaction

- https://code.claude.com/docs/en/context-window (accessed 2026-06-13). Claude Code compaction, /compact, subagents. Confidence: high (vendor docs).
- https://platform.claude.com/docs/en/build-with-claude/compaction (accessed 2026-06-13). Server-side compaction, compact_20260112, 150K default trigger, beta header. Confidence: high.
- https://platform.claude.com/docs/en/build-with-claude/context-editing (accessed 2026-06-13). clear_tool_uses_20250919, clear_thinking_20251015. Confidence: high.
- https://code.claude.com/docs/en/prompt-caching.md (accessed 2026-06-13). How compaction interacts with caching. Confidence: high.
- microCompact.ts source reference via search (claude-code-best mirror, accessed 2026-06-13). Microcompact time-based and cached paths. Confidence: medium (source mirror, not official repo).
- https://cursor.com/docs/agent/prompting (accessed 2026-06-13). Cursor context compression, context ring buckets. Confidence: high (vendor docs).
- https://forum.cursor.com/t/summarizing-chat-context-why/102842 (accessed 2026-06-13). Cursor staff on summarization choice. Confidence: medium (vendor forum staff).
- https://forum.cursor.com/t/context-usage-breakdown/159913 (accessed 2026-06-13). Context ring in Cursor 3.3, bucket table. Confidence: medium (vendor forum).
- https://developertoolkit.ai/en/cursor-ide/quick-start/context-management/ and /advanced-techniques/token-management/ (accessed 2026-06-13). "Degrades after 20-30 exchanges." Confidence: low (third-party guide; community-observed, not controlled).
- https://github.com/openai/codex/issues/10365 (accessed 2026-06-13). OpenAI maintainer: 90% trigger, no off switch, model_auto_compact_token_limit. Confidence: high (maintainer statement).
- https://github.com/openai/codex/commit/80fdd4688f6fa8143488c206d4c14dc193905254 (accessed 2026-06-13). body_after_prefix scope. Confidence: high (primary commit).
- https://github.com/openai/codex/issues/16033 (accessed 2026-06-13). Between-turns-only compaction check. Confidence: medium (issue analysis).
- https://wasnotwas.com/writing/context-compaction/ (accessed 2026-06-13). Codex 90% and Gemini CLI 50% / extract-plus-tail / CONTENT_TRUNCATED fallback. Confidence: medium (third-party source-code reading; detailed and specific).

## Prompt compression

- https://github.com/microsoft/LLMLingua (accessed 2026-06-13). LLMLingua family, up to 20x, LLMLingua-2 BERT classifier, MIT. Confidence: high (primary repo).
- https://raw.githubusercontent.com/microsoft/LLMLingua/main/Transparency_FAQ.md (accessed 2026-06-13). Method and limitations. Confidence: high.
- ACL 2024 paper (aclanthology.org/2024.findings-acl.57), arXiv 2403.12968. LLMLingua-2 method. Confidence: high (peer-reviewed).

## Compression proxies and libraries

- https://github.com/npow/kompact, https://pypi.org/project/kompact/ (accessed 2026-06-13). 8-transform proxy, 40-70%, BFCL eval, cache aligner, 2 stars. Confidence: high (primary repo) for mechanism; vendor claim for savings.
- https://github.com/open-compress/claw-compactor, https://pypi.org/project/claw-compactor/ (accessed 2026-06-13). 14-stage pipeline, reversible, AST-aware, ROUGE-L comparison, MIT, v7.x. Confidence: high (primary) for mechanism; vendor claim (self-published comparison) for benchmarks.

## MCP gateways

- https://github.com/KGT24k/mcp-tool-search (accessed 2026-06-13). 4 meta-tools, 85-96% reduction. Confidence: high (primary) for mechanism; vendor claim for percentages.
- https://github.com/omer-ayhan/mcpmux (accessed 2026-06-13). 5 tools, ~97% on 111 tools, scoring, risk gating. Confidence: high / vendor claim.
- https://github.com/eznix86/mcp-gateway, https://github.com/rustishs/mcp-gateway (accessed 2026-06-13). Search-first, ~97%. Confidence: high / vendor claim.
- https://www.stackone.com/blog/mcp-token-optimization/ (accessed 2026-06-13). Search-first patterns; "Claude Code enables this automatically." Confidence: medium (vendor blog, but the Claude Code claim is specific and checkable).

## Memory layers

- https://github.com/mem0ai/mem0 (accessed 2026-06-13). 58,492 stars, Apache-2.0, April 2026 algorithm benchmarks. Confidence: high (primary) for stars/license; vendor claim for benchmarks.
- https://techcrunch.com/2025/10/28/mem0-raises-24m-... (accessed 2026-06-13). $24M raise, adoption stats. Confidence: medium (reputable press, vendor-sourced figures).
- https://agentmarketcap.ai/blog/2026/04/08/... and https://aicraftguide.com/article/mem0-vs-letta-vs-zep-... and https://preuve.ai/blog/ai-memory-systems-statistics-2026 (accessed 2026-06-13). Letta ~13K stars and $10M seed; Zep $24M Series A Oct 2025, Graphiti, pricing tiers. Confidence: low to medium (third-party aggregations; not re-verified via API).

## Repo and context packers

- https://github.com/yamadashy/repomix (accessed 2026-06-13). 26,245 stars, MIT, tree-sitter compress, MCP server, v1.14.1. Confidence: high.
- https://github.com/mufeedvh/code2prompt (accessed 2026-06-13). 7,405 stars, MIT, Rust, v4.2.0. Confidence: high.
- https://rywalker.com/research/code-intelligence-tools (accessed 2026-06-13). ~70% Repomix reduction, Aider repo map ~1K tokens, npm downloads. Confidence: low to medium (third-party research).
- https://github.com/glincker/stacklit/blob/master/COMPARISON.md (accessed 2026-06-13). Packer comparison table, Aider ~43K stars. Confidence: low (competitor-authored comparison).

## Proxies, observability, gateways

- https://klymentiev.com/blog/llm-gateway-guide, https://llmcfo.com/research/litellm-vs-helicone-vs-langfuse, https://pocketlantern.dev/..., https://www.cekura.ai/blogs/helicone-vs-langfuse-vs-cekura (accessed 2026-06-13). Role split LiteLLM/Helicone/Langfuse/OpenRouter, pricing, Helicone maintenance-mode mention, OpenRouter 5.5% fee. Confidence: low to medium (third-party comparisons; Helicone maintenance-mode and Langfuse/ClickHouse are unconfirmed, see open questions).

## Prompt caching

- https://www.respan.ai/articles/llm-prompt-caching (accessed 2026-06-13). Anthropic explicit (0.1x reads, 1.25x/2x writes, 5min/1hr), OpenAI automatic over 1,024 tokens, worked pricing example. Confidence: medium (third-party but detailed and consistent with provider docs).
- https://pecollective.com/tools/anthropic-api-pricing/ (accessed 2026-06-13). Anthropic per-model cache pricing. Confidence: medium.
- https://aicost.tools/blog/prompt-caching-llm-cost-math/ (accessed 2026-06-13). Cache write fees, TTLs, OpenAI no-write-premium. Confidence: medium.
- Anthropic and OpenAI official prompt-caching docs (referenced; the mechanism facts are corroborated across the above). Confidence: high for the mechanism.

## Internal (ctx) sources, not independent verification

- docs/strategy-context-truth-layer.md, docs/roadmap-context-truth-layer.md, docs/adr/* (repo). ctx's thesis, the live 343K/0%/5% result (observed, one machine), the research-frontier citations (Factory.ai, Slipstream, ProcCtrlBench, ICAE), and the Tier 2 proxy cluster names. Confidence: internal; the live numbers are observed-but-early; the research citations are not re-verified (see 90-open-questions.md).
