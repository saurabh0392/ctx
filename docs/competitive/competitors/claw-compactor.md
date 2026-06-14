# Claw Compactor

- Category: tool-output / context compression engine (library)
- Classification: direct competitor (mechanism lookalike, library form)
- One-liner: an open-source 14-stage compression pipeline with reversible compression and AST-aware code handling, zero LLM inference cost.
- URL / repo / docs: https://github.com/open-compress/claw-compactor, https://pypi.org/project/claw-compactor/
- Maturity signals: software version 7.x (vendor README/PyPI, v7.1.0 on PyPI, accessed 2026-06-13). License MIT. Authored by "OpenClaw" (openclaw.ai). Star count not captured this pass (logged in open questions). The high version number with low public visibility suggests an actively iterated but not yet widely adopted project.
- License / pricing: MIT, free.

## What it does and how (mechanism)

Claw Compactor is a compression library, not a proxy. It chains 14 specialized stages through an immutable data flow: AST-aware code analysis (tree-sitter), JSON statistical sampling, simhash-based deduplication, and more, each stage's output feeding the next. It routes content by type, so code identifiers, JSON keys, and log patterns are handled by structure-aware stages rather than a single perplexity score.

Two notable properties:
- Reversible compression: it can re-expand a compressed block. This is the same idea as ctx's planned rewind store (roadmap E1.2).
- Zero LLM inference cost: all stages are deterministic, no model calls.

Where it sits: wherever the integrator puts it. It is a building block. OpenClaw also ships an agent ("OpenClaw") and an RTK plugin integration exists, so the compactor can live inside an agent's tool path.

## Claimed results vs verifiable results

- Claimed (vendor): 15-82% compression depending on content; ROUGE-L of 0.653 at rate 0.3 and 0.723 at rate 0.5, beating LLMLingua-2 (0.346 / 0.570) and SelectiveContext on a self-published comparison table. Latency under 50ms, zero dependencies. Source: Claw Compactor README / PyPI, accessed 2026-06-13. Label: vendor claim, self-published comparison.
- Verifiable: the stage architecture and reversibility are documented in the repo. The benchmark numbers are vendor-run, not independently replicated.
- Honest point: ROUGE-L measures text overlap with a reference, not whether the agent completed the task. ctx's strategy doc cites research (ICAE on SWE-bench) showing a compressor can improve similarity scores while solving fewer issues. ROUGE is not outcome.

## Strengths

- Content-type-aware stages are genuinely better than single-signal perplexity drop for code, JSON, and logs.
- Reversible by design, which is the safest compression posture and matches ctx's fail-closed ethos.
- Deterministic, fast, no model calls, no dependencies.

## Weaknesses and blind spots

- It is a library, so it inherits whatever measurement (or none) the integrator adds. By itself it has no per-user proof and no causal safety gate.
- Quality claims rest on ROUGE-L, which the broader field (and ctx's own thesis) has flagged as the wrong metric.
- Adoption and trust signals are thin.

## Overlap with ctx

Mechanism overlap on the compression transforms (AST-aware, dedup, structure-aware trimming) and on reversibility. ctx's tool-output trimming and planned rewind store do similar things. The difference is governance: ctx wraps transforms in a causal gate and production-behavior proof; Claw Compactor is the transform engine without the governance.

## Where ctx is better / where it is worse

Better:
- Governance and proof. ctx decides whether to apply a transform based on the user's own corrections and re-reads. Claw Compactor applies; it does not prove.
- Cross-agent integration and a shipped product (dashboard, surfaces), not just a library.

Worse:
- The transforms themselves. Claw Compactor's 14 content-aware stages are more sophisticated than ctx's current heuristics. This is a place ctx could borrow or even integrate rather than rebuild.

## Threat level to ctx: low (direct), but high-value to learn from

As a competitor it is a library with limited reach. As a source of ideas it is one of the most aligned: reversible, content-aware, zero-inference. The strategy doc already names it as the model for ctx's rewind store.

## What ctx should learn or steal

- Reversible compression with a rewind store (already on the roadmap, E1.2). Claw Compactor is proof the pattern works and is the reference to study.
- Content-type-aware stages for code, JSON, and logs. ctx should upgrade its Bash and tool-output trimming toward structure-aware filters (overlaps with the RTK lesson).
- Do not chase ROUGE-L. Cite outcome, not overlap. The transforms are commoditized; the proof is not (strategy doc, "the compression transforms themselves are not a moat").
