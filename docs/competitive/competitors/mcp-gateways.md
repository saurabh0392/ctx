# MCP gateways and tool-search routers

- Category: MCP tool filtering / routing
- Classification: direct competitor (on ctx's MCP profile-filtering feature)
- One-liner: proxies that sit in front of many MCP servers and replace the full tool catalog with a few search-and-call meta-tools, cutting tool-schema tokens about 85-97%.
- URL / repo / docs: representative projects: https://github.com/KGT24k/mcp-tool-search, https://github.com/omer-ayhan/mcpmux, https://github.com/eznix86/mcp-gateway, https://github.com/rustishs/mcp-gateway, StackOne Tool Search (https://www.stackone.com/blog/mcp-token-optimization/), Docker MCP Gateway, MCPJungle.
- Maturity signals: a crowded, fast-moving cluster of mostly young open-source projects plus commercial entrants (StackOne, Docker, TrueFoundry). Individual star counts vary and move fast; not all captured this pass (logged in open questions). The pattern is consolidating fast and is being absorbed by platforms.
- License / pricing: mostly open-source (MIT-class) for the OSS projects; commercial gateways are usage- or seat-priced.

## What it does and how (mechanism)

The dominant pattern is search-first discovery. Instead of loading every MCP server's full tool schemas into context on every turn (often 8,000 to 27,000+ tokens for 5 to 12 servers), the gateway exposes 2 to 5 lightweight meta-tools: `search_tools`, `get_tool_schema`, `call_tool`, `list_servers`. The model searches for what it needs, pulls only the matching schema, and calls through the proxy. The full catalog never enters context. Claimed reductions: about 85-96% (mcp-tool-search), about 97% (mcpmux on 111 tools), about 97% (rustishs/mcp-gateway on 100 tools).

Some add per-upstream include/exclude tool lists, risk classification (block destructive tools by default), scoring by verb/domain/intent/history, and success tracking for ranking.

Where it sits: between the MCP client (Cursor, Claude Code, Codex, VS Code) and the upstream MCP servers. It is a routing proxy at the MCP layer, not the model API layer.

Importantly, StackOne notes that Claude Code now enables search-first tool discovery automatically, which means this whole category is being absorbed by the platforms.

## Claimed results vs verifiable results

- Claimed (vendors): 85-97% tool-context token reduction. Source: project READMEs, accessed 2026-06-13. Label: vendor claim; the token math (catalog size vs 4 meta-tools) is mechanically credible.
- Verifiable: the mechanism is simple and the savings are arithmetic (replace N schemas with ~4 tools). The cost is documented honestly by the projects themselves: extra LLM turns for first use of a tool (search then call), and discovery overhead for small catalogs.
- Trade-off (observed from the projects): search-first adds latency and turns. For small catalogs it is net negative. ctx's static profile filtering avoids the extra turns by pre-stripping rarely-used schemas, at the cost of less dynamism.

## Strengths

- Strong, mechanically obvious savings for users with many MCP servers.
- Cross-client (works with any MCP client).
- Some add governance (risk classification, per-tool allowlists) that ctx does not.
- Constant context footprint regardless of catalog size.

## Weaknesses and blind spots

- Extra turns and latency for first use of each tool. Search-first trades schema tokens for round trips.
- Another proxy to run and trust in the tool path.
- For users with 1-2 servers it is overhead, not savings.
- It only addresses tool schemas, not tool outputs or conversation. It is one slice of the token problem.

## Overlap with ctx

Direct on one feature: ctx's MCP tool-schema filtering (profiles) targets the same prefix tokens. The mechanisms differ. ctx strips schemas for tools the user rarely calls, statically, before the request, with no extra turns. The gateways replace the whole catalog with search meta-tools, dynamically, at the cost of extra turns. ctx's approach is lighter-touch and turn-free; the gateways scale better to huge catalogs.

ctx does not overlap on the gateways' aggregation and governance features.

## Where ctx is better / where it is worse

Better:
- No extra turns or latency (static strip vs search-then-call).
- Part of a single local layer that also trims tool outputs and proves safety, rather than a standalone proxy that only handles schemas.
- Profile filtering is informed by the user's actual tool usage, which is a small version of the same per-user adaptation ctx does everywhere.

Worse:
- For users with very large MCP catalogs (50+ tools), search-first scales better than static stripping.
- The gateways offer governance (RBAC, risk gating, audit) that enterprise buyers want and ctx does not provide.
- This is the most crowded and most quickly-absorbed sub-category. Claude Code already does search-first natively.

## Threat level to ctx: medium

Not a threat to ctx's core (tool-output trimming and proof), but a direct threat to ctx's MCP profile-filtering feature, and it is commoditizing fast with platform absorption already underway. ctx should treat MCP schema filtering as a supporting feature, not a headline, and not over-invest against a category the platforms are eating.

## What ctx should learn or steal

- Surface MCP catalog cost prominently and show the user the savings, the way the gateways do. Cursor's context ring already trains users to see it.
- Consider an optional search-first mode for users with large catalogs, but keep ctx's turn-free static stripping as the default for the common case (a handful of servers).
- Do not make MCP filtering the lead story. It is being absorbed. Lead with tool-output trimming plus proof, which the gateways do not touch.
