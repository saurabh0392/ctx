# Pricing and business models

Last updated: 2026-06-13. What people actually pay for in this space, how each competitor is monetized, and the honest implication for whether ctx is a product, a feature, or an open-source land-grab.

## What is actually paid for here (the pattern)

Three models show up, and they map cleanly to the categories:

1. Free and open source, no monetization yet. The entire token-saving core is here: RTK (Apache-2.0, free), Kompact (MIT), Claw Compactor (MIT), LLMLingua (MIT), the OSS MCP gateways, the repo packers (Repomix, code2prompt, files-to-prompt, all MIT). None of these charges for the savings itself. This is the key fact: nobody has successfully monetized "cut my agent's tokens" as a standalone paid product.

2. Bundled into the platform, free to the user. Claude Code compaction and context editing, Cursor summarization, Codex and Gemini compaction, and prompt caching. The user pays for tokens; context management is a free feature that makes the platform stickier. The platforms monetize the model, not the compaction.

3. Paid where there is operational burden or governance. This is where money actually changes hands:
   - Memory layers: Mem0 (open core plus usage-priced cloud; raised $24M by October 2025), Zep (open-core Graphiti plus cloud from free through $25/mo Flex to enterprise; raised $24M Series A October 2025), Letta (open plus platform; $10M seed). People pay to not operate a memory store.
   - Observability and gateways: Langfuse (open source plus paid cloud), Helicone (free tier around 100K req/mo plus paid), LiteLLM (free self-host plus enterprise), OpenRouter (5.5% credit fee). People pay for hosting, scale, RBAC, and dashboards.
   - MCP governance: enterprise MCP gateways (TrueFoundry and similar) charge for RBAC, audit, and a managed registry.

The throughline: customers pay for managed infrastructure, governance, and proof at scale. They do not pay for a local algorithm that saves tokens.

## How each competitor monetizes (dated where known)

| Competitor | Model | Monetization | Source / date |
| --- | --- | --- | --- |
| RTK | OSS (Apache-2.0) | None observed; opt-in aggregate telemetry only | RTK repo, 2026-06-13 |
| Claude Code / Cursor / Codex / Gemini | Platform-bundled | Free feature; they monetize model usage and subscriptions | Vendor docs, 2026-06-13 |
| Prompt caching | Provider billing feature | A discount, not a product | Provider pricing, 2026-06-13 |
| Kompact / Claw Compactor / LLMLingua | OSS (MIT) | None | Repos / PyPI, 2026-06-13 |
| MCP gateways (OSS) | OSS | None; enterprise variants charge for governance | Project repos, 2026-06-13 |
| Mem0 | Open core | Usage-priced cloud; $24M raised by Oct 2025 | TechCrunch 2025-10-28 |
| Zep | Open core | Cloud tiers ($25/mo Flex up to enterprise); $24M Series A Oct 2025 | Third-party, 2026 |
| Letta | Open core | Platform; $10M seed | Third-party, 2026 |
| Langfuse / Helicone / LiteLLM | Open core | Cloud, scale, enterprise features | Third-party, 2026-06-13 |
| OpenRouter | Marketplace | 5.5% credit purchase fee | Third-party, 2026 |

## So what is ctx: product, feature, or land-grab?

The honest read, given the pattern above:

- ctx as a paid individual product: weak. The category is free. An individual will not pay much for token savings when RTK and the platforms are free. Do not build the business on this.
- ctx as a feature: this is the absorption risk, not a business. If "trim native tool output and prove it" is just a feature, the platforms can ship it. ctx must be more than a feature to survive (see 60-threats-and-moats.md).
- ctx as an open-source land-grab with a team and enterprise layer: this is the realistic path. The free, local, no-account tool wins the solo developer and builds the proof corpus and trust. Monetization comes later and at the team and org level, where the pattern shows people actually pay:
  - The measurement layer: a cross-agent, honest view of context cost and correction impact, including native-compaction harm. This is governance and proof, which teams and enterprises pay for.
  - The portable per-repo policy artifact (roadmap E3.1): a team-shareable, earned context policy. This is infrastructure, which orgs pay to manage.
  - A context-truth score and API that feeds the eval stack (roadmap E3.2, E3.3): positions ctx as a paid signal in a stack buyers already fund (Langfuse, Braintrust).

## Pricing implications and guardrails

- Keep the core free, local, open source. That is the land-grab, and it matches every successful tool in the category. Charging for local trimming would lose to free RTK and free platforms instantly.
- Monetize the team and org layer (measurement, governance, portability), not the local algorithm. That is where the comparable companies (Mem0, Zep, Langfuse) actually earn.
- Do not introduce telemetry to monetize. ctx's no-telemetry, no-account posture is a trust asset and a differentiator against everyone. A paid team tier should be opt-in and locally or self-hosted, not a data play.
- Resist a usage-based "percent of savings" model for individuals. Savings are unproven per-user and small individually; metering them would be both hard and low-trust.

## One-line recommendation

ctx is an open-source land-grab on the solo AI-heavy developer, with the paid business deferred to the team and enterprise measurement-and-governance layer, never the local algorithm. Free where the category is free; paid only where the comparable companies prove people pay (hosting, governance, proof at scale).
