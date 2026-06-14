# LLMLingua / LongLLMLingua / LLMLingua-2 (Microsoft)

- Category: prompt / context compression (general, research library)
- Classification: adjacent (different delivery, same buyer pain)
- One-liner: Microsoft Research's prompt-compression family that drops low-information tokens using a small language model (or a BERT classifier in v2), claimed up to 20x compression.
- URL / repo / docs: https://github.com/microsoft/LLMLingua, https://llmlingua.com
- Maturity signals: 6,287 GitHub stars (observed, GitHub API, 2026-06-13). License MIT. Latest release v0.2.2 (2024-04-09); last repo push 2026-04-08 (observed). The lack of a release since 2024 suggests the library is stable but not actively shipping new versions; it is research-grade infrastructure, not a product.
- License / pricing: MIT, free. It is a library; the user pays for the compressor model's inference.

## What it does and how (mechanism)

LLMLingua compresses a prompt by scoring tokens with a small language model (GPT-2 or a LLaMA-7b class model) and dropping low-perplexity, low-information tokens before the prompt is sent to the main model. LongLLMLingua extends this to long-context and retrieval settings. LLMLingua-2 replaces the perplexity approach with a BERT-level token classifier trained by data distillation from GPT-4, which is task-agnostic and 3-6x faster.

Where it sits: it is a Python library you call in your own pipeline (`PromptCompressor.compress_prompt(...)`), or via the promptflow integration. It is not an agent integration. To use it in a coding agent you would have to build the plumbing yourself.

## Claimed results vs verifiable results

- Claimed (vendor / paper): up to 20x compression while preserving in-context-learning and reasoning ability; LLMLingua-2 is 3-6x faster than v1 and better out of domain. Source: repo README and the ACL 2024 paper, accessed 2026-06-13. Label: vendor / peer-reviewed claim.
- Verifiable: the method, the API, and the academic publications are public and reproducible (observed).
- Relevant honest point: it requires running a separate compressor model (cost and latency), and it compresses by information score, not by code structure. ctx's strategy doc cites the field's finding that perplexity-style dropping "destroys code identifiers, JSON keys, and log patterns" (Claw Compactor comparison), which is a known weakness for coding work specifically.

## Strengths

- Rigorous, peer-reviewed, from Microsoft Research. The most academically credible name in prompt compression.
- Task-agnostic v2 is fast and broadly applicable.
- A reference point the whole field compares against.

## Weaknesses and blind spots

- Not an agent product. No hook, no proxy, no coding-agent integration out of the box. A developer would have to wire it in.
- Compresses by information score, which is weaker on structured code, JSON, and logs than AST-aware approaches.
- Requires a compressor model (latency, memory, cost). Not zero-inference.
- No per-user proof, no causal gate; quality is measured on research benchmarks.
- Stale release cadence (no new release since 2024-04).

## Overlap with ctx

Low to moderate. Both reduce tokens, but LLMLingua is a general prompt-compression library, not a tool-output trimmer for coding agents. There is no overlap on surfaces (LLMLingua has none) and no overlap on the proof or governance layer.

## Where ctx is better / where it is worse

Better:
- It is a product with surfaces, a dashboard, and a safety gate. LLMLingua is a library.
- Structure-aware trimming for tool output beats perplexity dropping for code.
- Production-behavior proof and edit-intent protection; LLMLingua has neither.
- Zero extra inference for ctx's heuristic trims.

Worse:
- Academic credibility and a strong general-purpose compression algorithm. If ctx ever needs heavy semantic compression of prose-like context, LLMLingua-2 is a proven option to call rather than rebuild.

## Threat level to ctx: low

Different delivery model and not aimed at coding agents. It is more a potential dependency than a competitor.

## What ctx should learn or steal

- For prose-heavy tool output (long docs, web fetches, large text blobs), LLMLingua-2 is a callable component ctx could integrate behind its safety gate rather than build from scratch.
- The reframing lesson is already internalized in ctx's strategy: compression ratio is the wrong metric. LLMLingua's own benchmark history is part of why the field moved to outcome.
