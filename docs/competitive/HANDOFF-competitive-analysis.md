# Session handoff: ctx competitive analysis (deep research + PM document set)

Paste everything below the line into a fresh session. It is self-contained: it assumes no memory of prior chats.

---

## Your mission

Produce a complete, honest, PM-grade competitive analysis for **ctx**. Two halves:

1. **Deep research on every competitor** in and adjacent to ctx's space, one rigorous brief each.
2. **Non-technical PM content** built on that research: market landscape, segmentation, positioning, battlecards, SWOT, pricing/business-model scan, threats and moats, and go-to-market implications.

Deliver a coherent set of markdown documents under `docs/competitive/`, written so a skeptical exec or an enterprise buyer would trust them. Distinguish observed fact from vendor claim from your own estimate, every time.

Do the research with real web searches. Cite sources with URLs and the date you accessed them. Today's date context: use the current year for all "latest" queries (it is 2026).

## Context: what ctx is (so you can position it accurately)

ctx is a **context truth and safety layer for AI coding agents**. It runs locally on a developer's machine and reduces the tokens an agent burns, without silently degrading the agent.

How it works today:
- **Tool-output trimming (PostToolUse).** After a tool runs (Read, Bash, Grep, Glob, and MCP server results), ctx shortens what the agent reads back, keeping errors and the lines that matter. The tool still runs in full.
- **Causal safety gate.** A tool only trims "for real" after ctx has proven, on the user's own sessions, that trimming it did not raise the rate of re-reads or corrections (Wilson/Newcombe before-vs-after). New tools enter a bounded automatic "burn-in" to earn this, then the gate keeps clean tools trimming and stops harmful ones.
- **MCP tool-schema filtering (profiles).** ctx hides schemas for MCP tools the user rarely calls via Claude Code permission rules, so Claude Code itself omits them and the prefix shrinks. ctx never edits the request body.
- **Edit-intent guard.** Protect-only: never trims a Read the agent has signaled it will edit. Works from narration on Claude Code and Cursor transcripts.
- **Local-first, hook-first.** Everything in SQLite on the user's machine, a local dashboard, no telemetry, no account. ctx influences agents only through hooks and settings; it never terminates TLS, installs a CA, or edits the request on the wire. (An earlier MITM proxy and a proxy-based "reasoning capture" idea were both removed: reasoning capture because Anthropic encrypts extended thinking end to end, ADR 0011; the proxy itself in ADR 0015.)
- **Surfaces.** Claude Code and Cursor today, with an agent-agnostic controller designed to extend to others (Codex, etc.).

Positioning thesis (to pressure-test, not assume): the platform compacts for the median user; ctx adapts to *your* repo and *your* work, and only acts when earned. The defensible wedge is the surfaces and proof other tools skip.

Live reality check (use as ground truth for honesty, not marketing): on the developer's own machine ctx had saved ~343K tokens, with corrections at ~0% and re-reads ~5% on the trimmed calls it could verify. It is early. Do not inflate.

## Why now (the trigger for this work)

A competitor, **RTK (Rust Token Killer)**, is winning the "token savings" framing hard (single Rust binary, `brew install rtk`, a PreToolUse hook that rewrites shell commands like `git status` into `rtk git status` and returns command-aware compact output, claimed 60-90% per command, ~50k GitHub stars as of mid-2026). Key structural facts to verify and build on:
- RTK's leverage is **command-aware semantic filters** (100+ hand-written), not generic trimming, and it **rewrites the command before it runs**.
- RTK's hook only fires on **Bash** tool calls. Native `Read`/`Grep`/`Glob` and **MCP outputs** bypass it. That is ctx's open lane.
- RTK skips per-user proof and instead relies on curated filters plus tee recovery of full output.

This analysis must answer: where does ctx actually win, where is it outclassed, and what is the honest, defensible position.

## Deliverables (the document set)

Create these files. Keep prose humanized (short sentences; no em dashes or arrow glyphs in prose; "n/a" not a dash for empty values). Lead with the recommendation, then evidence.

```
docs/competitive/
  README.md                         # index + how to read this set + last-updated date
  00-landscape.md                   # the market map: categories, who is in each, a one-screen diagram (mermaid ok)
  10-comparison-matrix.md           # ctx vs each competitor across a fixed set of dimensions (table)
  20-positioning.md                 # where ctx wins/loses, the wedge, the one-line position, messaging pillars
  30-market-and-segments.md         # PM: market context, ICP/personas, jobs-to-be-done, segments, trends, rough sizing with assumptions shown
  40-pricing-and-business-models.md # PM: how each competitor is priced/monetized (or OSS), implications for ctx
  50-swot.md                        # ctx SWOT grounded in the briefs
  60-threats-and-moats.md           # what could kill ctx (esp. platform absorption), what is actually defensible
  70-gtm-implications.md            # PM: positioning, narrative, naming, launch surfaces, partnerships, what NOT to do
  80-battlecards.md                 # one short battlecard per top competitor: their pitch, our counter, traps, proof points
  90-open-questions.md              # what we could not verify, what needs primary research or a spike
  competitors/<slug>.md             # one deep brief per competitor (template below)
  sources.md                        # every source: title, URL, date accessed, and a confidence note
```

## Per-competitor brief template (`competitors/<slug>.md`)

Each brief must use this structure and stay honest about evidence quality:

```
# <Competitor name>

- Category: <tool-output compressor | platform-native compaction | MCP filtering/routing | memory layer | repo/context packer | proxy/observability | prompt caching | other>
- One-liner: <what it is, plainly>
- URL / repo / docs:
- Maturity signals: <stars, last release, funding, adoption signals> (with dates and sources)
- License / pricing:

## What it does and how (mechanism)
Be specific about the mechanism: where it sits in the request path (pre/post tool, proxy, retrieval), what it actually compresses or manages, and the surfaces it covers (Bash only? native Read/Grep? MCP? whole-prompt?).

## Claimed results vs verifiable results
Separate vendor claims from anything independently shown. Cite both.

## Strengths
## Weaknesses and blind spots
## Overlap with ctx
## Where ctx is better / where it is worse
## Threat level to ctx (low/medium/high) and why
## What ctx should learn or steal
```

## Competitors and categories to cover

This is a **seed list, not a ceiling**. Search hard and add anything real you find. Be exhaustive within reason; it is better to include a thin brief on a marginal player than to miss a category.

- **Tool-output / command compressors:** RTK (Rust Token Killer) [primary], any RTK clones or alternatives, shell-output summarizers, ANSI/log strippers marketed for agents.
- **Prompt / context compression (general):** Microsoft LLMLingua / LongLLMLingua / LLMLingua-2, any "prompt compression" libraries or services, semantic-token-reduction tools.
- **Platform-native compaction:** Anthropic Claude Code `/compact` and context editing / context management, Anthropic memory tool, Cursor's own context summarization and management, OpenAI/Codex equivalents, Gemini CLI context handling. Treat the platforms as the biggest competitive force (absorption risk).
- **Prompt caching:** Anthropic and OpenAI prompt caching (different mechanism, same buyer pain). Explain how it competes for the same budget.
- **MCP tool filtering / routing:** tools that prune or route MCP tool schemas, MCP gateways/proxies, tool-selection/routers, "too many tools" mitigations.
- **Memory layers:** Mem0, Letta / MemGPT, Zep, and similar. They reduce context a different way; assess overlap honestly.
- **Repo/context packers (retrieval-side):** Aider repo map, Repomix/repopack, code2prompt, files-to-prompt, RepoPrompt, Continue.dev, Cline, Roo Code context handling. These shape what enters context; relevant to the same JTBD.
- **Proxies / observability that touch context:** Helicone, Langfuse, LiteLLM, OpenRouter, any LLM gateway that offers compression or trimming. Most are observability, not compression; say so, but check for overlap.

For each, decide: direct competitor, adjacent, or different-problem. Put that judgment in the brief and the matrix.

## Non-technical PM content (the second half)

Beyond the briefs, write the PM artifacts a product lead would bring to a strategy review:

- **Market and segments (`30-`):** the buyer's real pain (token cost, context-window pollution, agent quality), ICP and 2-3 personas (e.g. solo AI-heavy dev, platform/AI-tooling team, enterprise procurement), jobs-to-be-done, adoption trends, and a rough market-size sketch with every assumption stated and labeled as an estimate. No fake TAM.
- **Pricing and business models (`40-`):** OSS vs paid vs platform-bundled, what people actually pay for here, and what that implies for whether ctx is a product, a feature, or an OSS land-grab.
- **Positioning and messaging (`20-`, `70-`):** the one-line position, three messaging pillars, the honest claim ctx can make today, and the claims it must not make yet. Battlecards (`80-`) for sales/community conversations.
- **Threats and moats (`60-`):** be brutal about platform absorption (Anthropic/Cursor shipping native compaction). State plainly what is defensible and what is not.

## Research method and rigor rules

- Use real web search and fetch primary sources (GitHub repos, official docs, changelogs, pricing pages). Prefer primary over blog summaries; use blogs only to find leads.
- For every quantitative claim, record: the number, the source, the date, and whether it is **observed**, a **vendor claim**, or your **estimate**. Never present a vendor claim or estimate as proof.
- Capture maturity signals with dates (stars, last release, funding). These move fast; stamp them.
- When you cannot verify something, say so in `90-open-questions.md` rather than guessing.
- Keep `sources.md` complete enough that someone could redo the analysis.

## Quality bar (repo conventions you must follow)

- **Honesty over hype.** This is the product's core value; the analysis must model it. Observed vs claimed vs estimated, always.
- **Apple-grade clarity.** Executive-first. Recommendation, then tradeoffs, then detail. No filler.
- **Humanized copy.** Short sentences. No em dashes or `--` as punctuation, no arrow glyphs in prose, no middle-dot bullet chains. Use "n/a" / "none" / "not yet" for empties.
- **Minimal, well-structured files.** Match the document set above. Cross-link between docs.

## Process

- File a Linear ticket before doing the work. Team **ctx**, project **"Context truth layer"**. Title it as a competitive-analysis epic, with the document set as the checklist. Link any sub-tickets.
- If the research forces a strategic decision (e.g. "stop competing with RTK on Bash; aim at Read/MCP"), capture it as an ADR in `docs/adr/` (next number in sequence) and reference it from `20-positioning.md`. Do not bury a strategy decision inside a competitor brief.
- Do not change product code as part of this task. This is research and writing only. If you find concrete product implications, list them in `70-gtm-implications.md` and/or as follow-up Linear tickets.

## Definition of done

- Every file in the document set exists and is internally consistent (the matrix, briefs, and positioning agree).
- Every competitor in the seed list is covered or explicitly dismissed with a reason, plus any you discovered.
- Every number is sourced and labeled by evidence quality.
- `README.md` indexes the set and carries a last-updated date.
- A Linear ticket exists and links the work. An ADR exists if a strategy call was made.
- The top-line answer is stated plainly somewhere obvious: where ctx honestly wins, where it loses, and the one position it should take.

## First steps

1. Read this repo's `docs/strategy-context-truth-layer.md` and any `docs/roadmap-*` and recent `docs/adr/*` to ground yourself in ctx's current thesis and state.
2. File the Linear epic.
3. Start with RTK and the platform-native compaction category (highest threat), then fan out across the categories.
4. Build the briefs first, then synthesize the PM documents on top of them.

---

(End of handoff prompt.)
