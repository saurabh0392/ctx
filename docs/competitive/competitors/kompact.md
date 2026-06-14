# Kompact (and the compression-proxy cluster)

- Category: tool-output / context compressor (transparent proxy)
- Classification: direct competitor (closest mechanism lookalike)
- One-liner: a transparent HTTP proxy that sits between the agent and the LLM provider and compresses the whole request on the fly, 40-70% claimed, zero code changes.
- URL / repo / docs: https://github.com/npow/kompact, https://pypi.org/project/kompact/
- Maturity signals: 2 GitHub stars (observed, GitHub repo, accessed 2026-06-13). License MIT. Created 2026-02-22, latest release v0.3.0 2026-03-21 (observed, GitHub/PyPI). Single contributor. Very early, effectively a research-grade project. Related names in the same cluster: Headroom / Kompress (a ModernBERT drop-in for LLMLingua), and token-compressor (an MCP server with an embedding-similarity gate). These are cited in ctx's internal strategy doc; not all were re-verified by independent web search in this pass (see 90-open-questions.md).
- License / pricing: MIT, free.

## What it does and how (mechanism)

Kompact is a drop-in proxy: point the agent's base URL at it and it intercepts the API request, compresses the context, and forwards to Anthropic or OpenAI. It runs an 8-transform pipeline: schema optimizer (TF-IDF selection over tool schemas), content compressors (TOON, JSON, code), extractive compression (TF-IDF sentence selection), an observation masker (history management), and a cache aligner (tries to preserve prefix caching). It adapts: short contexts get light compression, long contexts get aggressive optimization. Sub-millisecond overhead claimed.

It sits in the request path as a full-prompt proxy, so unlike RTK (Bash hook) or ctx (per-tool-result trimming), it can in principle touch everything in the request, including tool schemas and prior tool outputs.

## Claimed results vs verifiable results

- Claimed (vendor): 40-70% token savings; tool-schema reduction of about 20-27% regardless of model. Evaluated on BFCL (1,431 real API schemas) end-to-end through Claude, scored with a `context-bench` harness. Source: Kompact README / PyPI, accessed 2026-06-13. Label: vendor claim, with a benchmark harness but no independent replication.
- Verifiable: the mechanism, the pipeline stages, and the 2-star / single-contributor maturity are observed (GitHub, 2026-06-13).
- The key honest point: Kompact's quality signal is BFCL, a benchmark, not the user's own production behavior. This is exactly the gap ctx's strategy targets.

## Strengths

- Architecturally clean: a full-prompt proxy can compress schemas, history, and tool outputs in one place.
- Cache-aligner shows awareness that naive compression can bust prefix caching (a real trap; see prompt-caching brief).
- Benchmarks on a real tool-calling dataset (BFCL), which is more honest than ratio-only claims.

## Weaknesses and blind spots

- Tiny and unproven. 2 stars, one contributor, three releases. No adoption, no distribution.
- MITM on the model API. To compress the full request it must terminate the provider connection, which is a heavy trust ask and a fragility risk (provider wire changes break it). ctx took the opposite path: it removed its MITM proxy entirely (ADR 0015) and is hook-first, after also reverting proxy-based reasoning capture for the same trust reason (ADR 0011). ctx never terminates TLS or edits the request on the wire.
- Quality measured on a benchmark, not on the user. No causal safety gate, no per-tool earning, no protection for reads the agent will edit.

## Overlap with ctx

High in intent (cut tokens locally, no code change) and partial in mechanism. Kompact compresses the whole request via a proxy; ctx trims tool results via hooks and filters MCP schemas through Claude Code permission rules, never touching the wire. Both can reduce tool-schema and tool-output tokens.

## Where ctx is better / where it is worse

Better:
- Proof on the user's own corrections and re-reads vs Kompact's benchmark score.
- Fail-closed, per-tool earned activation vs Kompact's always-on transforms.
- Lighter trust posture: ctx uses hooks and permission rules only, with no proxy and no CA. Kompact is a mandatory full-API MITM.
- Edit-intent protection.

Worse:
- A full-prompt proxy can reach conversation history and the whole request; ctx's hook-based trimming is scoped to tool results plus permission-rule MCP schema filtering.
- Nothing on distribution; both are early, but the mechanism debate is moot until adoption exists.

## Threat level to ctx: low (today), medium (as a pattern)

Kompact itself is too small to be a threat. The pattern (a transparent compression proxy with a benchmark) is the more interesting signal: it confirms the market is converging on the same insight ctx was built around (outcome over ratio) but still measuring outcome synthetically. If a well-funded team ships a polished version of this with real adoption, the threat rises.

## What ctx should learn or steal

- The cache-aligner idea is important. ctx must verify its trims and schema filtering do not bust prefix caching and erase the savings (see prompt-caching brief, 70-gtm-implications.md).
- BFCL and `context-bench` are useful external benchmarks to cite for credibility, but ctx's real differentiator is the production-behavior label. Keep that front and center.
- The full-prompt proxy is the architecture ctx chose not to build, for trust reasons. That choice is a feature: say so plainly.
