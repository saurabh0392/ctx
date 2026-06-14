# Comparison matrix: ctx vs the field

Last updated: 2026-06-13. Read this with 00-landscape.md (who is who) and the briefs in competitors/ (the detail and sources).

## How to read the dimensions

- Surfaces covered: which agents and which tool calls the product actually acts on.
- Native Read/Grep/Glob: does it reduce tokens from native file tools (not just shell)?
- MCP handling: does it reduce MCP tokens, and how (schema, output, or both)?
- Where it acts: pre-tool, post-tool, conversation, API, or retrieval.
- Proof of safety: does it measure, on the user's real behavior, whether the intervention hurt?
- Local / no account: runs locally with no telemetry and no sign-up?
- Maturity: adoption signal, with date.

## Direct and platform competitors

| Product | Surfaces | Native Read/Grep/Glob | MCP handling | Where it acts | Proof of safety | Local / no account | Maturity (dated) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| ctx | Claude Code (hook), Cursor (ingest) | Yes (PostToolUse trim) | Output trim + schema filter (via permission rules) | Hooks only, no proxy (never edits the wire) | Yes: per-user causal gate (re-reads, corrections) | Yes: SQLite, no telemetry, no account | Early, single-dev install; about 343K tokens saved, about 0% corrections, about 5% re-reads (observed, one machine) |
| RTK | 14 agents; real hooks on Claude Code, Cursor, Gemini | No (Bash hook only; needs explicit `rtk read`) | No schema filter; output only if run via shell | Pre-tool (rewrites command) | No (curated filters + tee on failure) | Local; opt-in aggregate telemetry | 62,127 stars, Apache-2.0, v0.28.2 (observed 2026-06-13) |
| Claude Code native | Claude Code only | Via context editing (clears stale results) | Clears tool results; no per-tool schema prune | Server-side compaction + context editing | No (self-graded) | No (first-party server-side) | Shipped; compaction beta `compact-2026-01-12` |
| Cursor native | Cursor only | Summarized into conversation | Coarse per-server toggle; catalog shown | Server-side summarization | No (self-graded) | No (Cursor backend) | Shipped; context ring in Cursor 3.3 |
| Codex native | Codex only | Blunt `tool_output_token_limit` cap | Same flat cap | Server-side compaction at ~90% | No | No | 90,868 stars (observed 2026-06-13) |
| Gemini CLI native | Gemini CLI only | `CONTENT_TRUNCATED` fallback | Same fallback | Compaction at 50% + tail preserve | No | No | Shipped (Google) |
| Kompact | Any (base-URL swap) | Yes, via full-prompt proxy | Schema + output | API proxy (MITM) | No (BFCL benchmark) | Local proxy; MITM the model API | 2 stars, MIT, v0.3.0 2026-03 (observed) |
| Claw Compactor | Wherever integrated | Yes (library) | Whatever integrator wires | Library transform | No (ROUGE benchmark) | Library, local | v7.x, MIT (vendor, 2026-06-13) |

## Adjacent and different-problem

| Product | What it reduces | Where it acts | Overlap with ctx | Classification |
| --- | --- | --- | --- | --- |
| LLMLingua family | Whole prompt by info score | Library, pre-send | Low (no agent surface) | Adjacent |
| MCP gateways (mcp-tool-search, mcpmux, StackOne, Docker) | MCP tool schemas | MCP-layer proxy, search-first | Direct on ctx's MCP profile feature | Direct (one feature) |
| Prompt caching (Anthropic, OpenAI) | Cost of the stable prefix | Provider billing | Different mechanism; design constraint | Different mechanism, same budget |
| Mem0 / Letta / Zep | Cross-session knowledge | Retrieval + persistence | Low; complementary | Different problem |
| Repomix / code2prompt / Aider map | What code enters context | Pre-loop retrieval | Low; complementary | Adjacent |
| LiteLLM / Helicone / Langfuse / OpenRouter | Routing, logging, evals | API boundary / SDK | Low; future integration | Different problem |

## What the matrix says in one paragraph

Only two columns separate ctx from everyone else: native Read/Grep/Glob coverage (which RTK and the platforms do not do well, the platforms only by coarse server-side clearing) and proof of safety (which literally no one else does). ctx is weakest on maturity and distribution (one-machine evidence vs RTK's 62K stars and the platforms' default reach) and on being a third-party install vs free first-party features. The strategy that follows from this matrix is in 20-positioning.md and ADR 0013: lead with the two columns ctx owns, treat the rest as table stakes or constraints.
