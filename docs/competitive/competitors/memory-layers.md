# Memory layers: Mem0, Letta (MemGPT), Zep

- Category: memory layer
- Classification: different problem (cross-session memory, not context-window safety)
- One-liner: services that give agents long-term memory across sessions by extracting, storing, and retrieving facts, so the agent recalls without re-reading everything.
- URL / repo / docs: https://github.com/mem0ai/mem0, https://github.com/letta-ai/letta, https://www.getzep.com (Graphiti: https://github.com/getzep/graphiti)
- Maturity signals (observed unless noted):
  - Mem0: 58,492 GitHub stars (GitHub API, 2026-06-13). License Apache-2.0. Raised $24M (seed plus Series A) by October 2025 (TechCrunch, 2025-10-28). Vendor claims 100,000+ developers and exclusive memory provider for the AWS Agent SDK (vendor, label: vendor claim).
  - Letta (formerly MemGPT): about 13,000 stars (third-party stat, 2026; not re-verified via API this pass). $10M seed led by Felicis (third-party, 2026). White-box, model-agnostic agent runtime.
  - Zep: built on Graphiti, an Apache-2.0 / MIT temporal knowledge graph. Raised $24M Series A led by Basis Set Ventures, October 2025 (third-party, 2026).
- License / pricing: Mem0 open-source plus a cloud platform (usage-priced). Zep open-core (Graphiti free; Zep cloud from a free tier through $25/mo Flex to enterprise). Letta open-source plus platform.

## What it does and how (mechanism)

These reduce context a fundamentally different way than ctx. Rather than trimming what a tool returns this turn, they persist knowledge across turns and sessions and inject only the relevant slice when needed.

- Mem0: extracts facts into a three-tier store (user, session, agent) backed by hybrid vector plus graph plus key-value. Self-edits on conflict rather than appending duplicates.
- Zep: stores every fact as a node in a temporal knowledge graph (Graphiti) with validity windows, so it can reason about facts that change over time.
- Letta: hands memory tools to the agent itself (`core_memory_append`, `archival_memory_search`), so the agent manages its own tiered context, in-context vs archival.

Where they sit: retrieval and persistence side, outside the per-turn tool path. They shape what enters context across the whole lifetime of an agent, not what a single tool returns.

## Claimed results vs verifiable results

- Claimed (vendors): strong long-memory benchmark numbers (Mem0 reports 94.8 on LongMemEval with its April 2026 algorithm; Zep reports beating MemGPT on DMR and large LongMemEval gains). Source: vendor repos and papers, accessed 2026-06-13. Label: vendor claim on memory-recall benchmarks, not coding-task outcome.
- Verifiable: the funding events, star counts (Mem0), and licenses are observed or well-sourced. The architectures are documented.

## Strengths

- A real, funded, fast-growing category with clear enterprise pull (memory is sticky and valuable).
- Solve a problem ctx does not: recall across sessions.
- Strong distribution (Mem0 at 58K stars, AWS Agent SDK relationship).

## Weaknesses and blind spots (relative to ctx's problem)

- They do not address per-turn context-window pollution from tool outputs or MCP schemas. A memory layer does not stop a single `git diff` or MCP result from burning thousands of tokens this turn.
- They add infrastructure (a store, sometimes a graph database) and an extraction step. Heavier than a local trimming layer.
- Quality is measured on memory-recall benchmarks, not on whether trimming the live context caused a correction.

## Overlap with ctx

Low and mostly conceptual. Both "reduce context," but at different layers: memory across sessions vs tool output within a turn. A developer could run Mem0 and ctx together with no conflict. ctx's strategy doc is explicit: do not build a memory layer; Mem0, Zep, and Letta own it.

## Where ctx is better / where it is worse

Better (for ctx's actual problem): per-turn token reduction, local-first with no store to operate, no extraction model, proof on the user's behavior.
Worse (for the memory problem): ctx does nothing for cross-session recall. It is not trying to.

## Threat level to ctx: low

Different problem. The only risk is narrative confusion (both say "less context"), which positioning must clear up.

## What ctx should learn or steal

- The funding and benchmark discipline. These teams publish, raise, and benchmark hard. ctx should be as rigorous in how it presents proof.
- The "memory passport" framing (portable across apps) rhymes with ctx's Horizon 3 portable per-repo policy artifact. There is a future where a learned ctx policy is as portable as a memory store.
- Do not compete here. Cross-link in positioning so buyers see ctx and memory layers as complementary, not alternatives.
