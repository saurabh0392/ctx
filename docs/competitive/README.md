# ctx competitive analysis

Last updated: 2026-06-13. Owner: ctx. Ticket: CTX-24. Strategy decision: ADR 0013.

This is a complete, honest competitive analysis for ctx, the context truth and safety layer for AI coding agents. It is written for a skeptical reader: every quantitative claim is labeled observed, vendor claim, or estimate, with a source and a date. Adoption and license facts marked observed were pulled from the GitHub API or primary repos on 2026-06-13.

## The top-line answer

- Where ctx honestly wins: on the surfaces RTK and the platforms miss (native Read, Grep, Glob, and MCP tool outputs), and on a claim no competitor makes (per-user proof that trimming did not cost you corrections).
- Where ctx loses today: on Bash command output (RTK is better and far better known), on free-and-default (the platforms ship native compaction for nothing), and on distribution (RTK has 62,127 stars; ctx has one machine of evidence).
- The one position to take: ctx is the context truth and safety layer for AI coding agents. It cuts the tool-output and MCP tokens your platform and RTK miss, on Claude Code and Cursor, and only after it proves on your own sessions that trimming did not cost you corrections.

Full version in 20-positioning.md. The strategic call is recorded in ADR 0013 (docs/adr/0013-competitive-position-surfaces-and-proof.md).

## How to read this set

Start with the top-line above, then 20-positioning.md. If you want the evidence first, read 00-landscape.md and 10-comparison-matrix.md, then the briefs. If you are an exec doing a skeptical review, go straight to 60-threats-and-moats.md and 90-open-questions.md.

## The document set

| File | What it is |
| --- | --- |
| 00-landscape.md | The market map: categories, who is in each, a one-screen diagram |
| 10-comparison-matrix.md | ctx vs each competitor across fixed dimensions |
| 20-positioning.md | Where ctx wins and loses, the wedge, the one-line position, three pillars |
| 30-market-and-segments.md | The buyer pain, ICP and personas, jobs-to-be-done, trends, a labeled size sketch |
| 40-pricing-and-business-models.md | OSS vs paid vs platform-bundled, and what ctx should be |
| 50-swot.md | ctx SWOT, grounded in the briefs |
| 60-threats-and-moats.md | The brutal version: platform absorption, and what is actually defensible |
| 70-gtm-implications.md | Positioning, narrative, naming, launch surfaces, partnerships, what not to do |
| 80-battlecards.md | One card per top competitor for community and sales |
| 90-open-questions.md | What could not be verified; spikes needed |
| sources.md | Every source: title, URL, date accessed, confidence note |
| competitors/ | One deep brief per competitor |

## The briefs

| Brief | Category | Classification | Threat |
| --- | --- | --- | --- |
| competitors/rtk.md | tool-output / command compressor | direct (primary) | high |
| competitors/claude-code.md | platform-native compaction | direct | high |
| competitors/cursor.md | platform-native compaction | direct | high |
| competitors/codex.md | platform-native compaction | direct | medium |
| competitors/gemini-cli.md | platform-native compaction | adjacent | low to medium |
| competitors/kompact.md | compression proxy (cluster) | direct (mechanism) | low now, medium as pattern |
| competitors/claw-compactor.md | compression library | direct (mechanism) | low |
| competitors/llmlingua.md | prompt compression (general) | adjacent | low |
| competitors/mcp-gateways.md | MCP filtering / routing | direct (one feature) | medium |
| competitors/memory-layers.md | memory (Mem0, Letta, Zep) | different problem | low |
| competitors/repo-packers.md | repo / context packers | adjacent | low |
| competitors/observability-gateways.md | proxy / observability | different problem | low |
| competitors/prompt-caching.md | prompt caching | different mechanism, same budget | low (competitor), high (constraint) |

## Coverage of the seed list

Every seed competitor is covered or explicitly placed:
- RTK and clones: rtk.md, plus the proxy cluster in kompact.md and claw-compactor.md.
- Prompt compression: llmlingua.md.
- Platform compaction: claude-code.md, cursor.md, codex.md, gemini-cli.md.
- Prompt caching: prompt-caching.md.
- MCP filtering / routing: mcp-gateways.md (covers mcp-tool-search, mcpmux, eznix86, rustishs, StackOne, Docker MCP Gateway, MCPJungle).
- Memory: memory-layers.md (Mem0, Letta, Zep).
- Repo packers: repo-packers.md (Repomix, code2prompt, files-to-prompt, Aider repo map; Gitingest, RepoPrompt, Continue, Cline, Roo noted as adjacent / different-problem).
- Proxies / observability: observability-gateways.md (Helicone, Langfuse, LiteLLM, OpenRouter; LangSmith, Braintrust, Arize noted).
- Unverified or thinly-sourced items (Kompress/Headroom, token-compressor, the research-frontier citations) are logged in 90-open-questions.md rather than asserted.

## A note on honesty

ctx's product is honesty about context. This analysis holds itself to the same bar. The live ctx result (about 343,000 tokens saved, about 0% corrections, about 5% re-reads on verifiable trimmed calls) is observed but early and from a single machine, and is labeled that way everywhere it appears. The defensible moat (production-behavior proof, cross-agent, neutral) is real in shape but unproven in fact. Read 60-threats-and-moats.md and 90-open-questions.md before drawing conclusions.

## Source brief

The original mission brief for this analysis is in HANDOFF-competitive-analysis.md.
