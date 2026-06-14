# Proxies and observability: Helicone, Langfuse, LiteLLM, OpenRouter

- Category: proxy / observability / LLM gateway
- Classification: different problem (they measure, route, and log; they do not trim tool output or prove context safety)
- One-liner: the control-plane layer between an app and model providers: routing, failover, cost tracking, tracing, and evals.
- URL / repo / docs: https://github.com/Helicone/helicone, https://github.com/langfuse/langfuse, https://github.com/BerriAI/litellm, https://openrouter.ai. Adjacent eval/observability: LangSmith, Braintrust, Arize.
- Maturity signals (third-party, May to June 2026 unless noted; not all re-verified via API this pass):
  - LiteLLM: open-source gateway, Apache-class, the de facto multi-provider router.
  - Langfuse: open-source (MIT), observability and evals; reported acquired by ClickHouse in January 2026 (ctx strategy doc; treat as needing confirmation, see open questions).
  - Helicone: MIT, observability proxy, free tier around 100K requests/month; one 2026 source reports it entered maintenance mode after an acquisition (label: unverified, logged in open questions).
  - OpenRouter: hosted marketplace, 400+ models, 5.5% credit purchase fee, zero inference markup.
- License / pricing: LiteLLM and Langfuse and Helicone have free open-source self-host plus paid tiers; OpenRouter is credit-based with a purchase fee.

## What it does and how (mechanism)

These sit on the request hot path or alongside it, but their job is governance and visibility, not context compression:

- LiteLLM: one API across many providers, virtual keys, budgets, fallbacks, retries, local cost tracking. A gateway.
- Helicone: a logging proxy (one-line base-URL swap) that records every request with cost, latency, tokens; also a gateway runtime.
- Langfuse: SDK-instrumented tracing, evals, prompt management, datasets, experiments. OpenTelemetry-native. The standard observability and eval platform.
- OpenRouter: a model marketplace with automatic fallback and price/throughput/quality routing.

Some gateways now offer prompt compression as an add-on feature (third-party mentions of Edgee, LockLLM), but it is a bolt-on, not the core.

Where they sit: at the model API boundary (LiteLLM, Helicone, OpenRouter) or as SDK instrumentation (Langfuse). They see requests and traces; they do not make per-tool-result trimming decisions and they do not measure correction impact of a context intervention.

## Claimed results vs verifiable results

- Claimed (vendors): cost visibility, routing reliability, eval coverage. These are well-established, widely-used products. Label: vendor claim, broadly corroborated by heavy third-party adoption.
- Verifiable: the role split (LiteLLM routes, Helicone logs, Langfuse traces and evals, OpenRouter is a marketplace) is consistent across multiple third-party comparisons (accessed 2026-06-13).
- Unverified this pass: the Langfuse/ClickHouse acquisition and the Helicone maintenance-mode claim. Logged in open questions.

## Strengths

- Mature, trusted, widely deployed. They own the observability and routing layer.
- Langfuse and Braintrust can gate on eval results ("prove it got better"), which rhymes with ctx's proof ethos, but their signal is offline eval datasets and LLM judges, not the user's real corrections.

## Weaknesses and blind spots (relative to ctx's problem)

- They do not act on context. They measure and route. None trims a tool result or filters an MCP schema as a core feature.
- Their quality signal is offline: eval sets, LLM judges, traces. Not the production behavioral join (corrections, re-reads, aborts) ctx uses.
- They are largely API-side; they do not sit inside the coding-agent tool loop the way ctx's hooks do.

## Overlap with ctx

Low on mechanism, interesting on philosophy. Langfuse and Braintrust share ctx's "measure before you trust" instinct, but they measure synthetically and do not change context. ctx's Horizon 3 plan is explicitly to emit its behavioral signal in a form these tools can ingest (OTel, Langfuse-compatible), positioning ctx as a production-behavior signal feeding the eval stack rather than competing with it.

## Where ctx is better / where it is worse

Better (for the context problem): ctx acts on context with proof from real behavior. These tools observe; they do not trim, and their proof is offline.
Worse (for observability and routing): ctx is not an observability platform, a router, or a marketplace, and should not try to be (strategy doc: do not build general observability).

## Threat level to ctx: low

Different problem. The risk is not competition but absorption-by-integration: a gateway could add a compression feature. If one did, it would still lack ctx's production-behavior proof and cross-agent coding-loop coverage.

## What ctx should learn or steal

- OpenTelemetry compatibility. Emit ctx's context-truth signal in OTel form so it lands in Langfuse or Braintrust (roadmap E3.3). This makes ctx a complement to the eval stack, not a competitor.
- Helicone's one-line setup is the friction bar to beat.
- Braintrust's "prove it got better" framing is close to ctx's; ctx's edge is that its proof comes from the human not having to fix the work, not from a judge. Say that distinction plainly.
