# Gemini CLI (native compaction)

- Category: platform-native compaction
- Classification: adjacent competitor (native, not a current ctx surface)
- One-liner: Google's terminal agent that compacts at 50% of a 1M-token window using extract-plus-tail summarization, with a tool-output truncation fallback.
- URL / repo / docs: https://github.com/google-gemini/gemini-cli, behavior documented in third-party source analysis (https://wasnotwas.com/writing/context-compaction/, accessed 2026-06-13).
- Maturity signals: shipped and default in Gemini CLI. Backed by Google. (Star count not re-verified in this pass; logged in open questions.)
- License / pricing: open-source CLI; the user pays for model usage. Compaction is a free built-in.

## What it does and how (mechanism)

Gemini CLI fires compaction at 50% of the context window by default, configurable in `~/.gemini/settings.json`. Because Gemini models expose a 1M-token window, the default trigger is around 524,000 tokens. The 50% threshold is a quiet admission that the nominal 1M window does not perform uniformly across its full range (third-party source analysis, accessed 2026-06-13).

The mechanism is distinctive: extract plus tail preservation, not full replacement. The last 30% of the conversation (by character count) is kept verbatim. The earlier 70% is summarized, and the summary is injected as a user-role message before the preserved tail, followed by a synthetic acknowledgment ("Got it. Thanks for the additional context!").

If compression inflates token count instead of reducing it, Gemini sets a `hasFailedCompressionAttempt` flag and skips auto-compression for the rest of the session, falling back to a `CONTENT_TRUNCATED` path that trims tool output with no LLM call. The `/compress` slash command bypasses both the threshold and the failure guard.

Where it sits: inside the Gemini CLI client, single-vendor.

## Claimed results vs verifiable results

- Verifiable (from source analysis): 50% threshold, extract-plus-tail, tail preservation at 30%, the failure flag and `CONTENT_TRUNCATED` fallback. Source: third-party reading of the Gemini CLI source, accessed 2026-06-13. Label: observed via secondary source (not re-derived from source here).
- No vendor per-task quality delta published. Label for any quality claim: not available.

## Strengths

- Tail preservation keeps recent live conversation verbatim, which avoids the worst of summary-induced amnesia near the working edge.
- A sensible failure guard (do not keep compressing if it is not helping).
- Built for genuinely large windows.

## Weaknesses and blind spots

- The `CONTENT_TRUNCATED` fallback is blunt tool-output trimming with no intelligence and no proof.
- Single-vendor, self-grading, no production-behavior signal.
- Character-count tail preservation is a heuristic; it does not know which 30% the agent actually needs.

## Overlap with ctx

The fallback tool-output trimming overlaps with ctx's core action, again as a blunt cap rather than an earned content-aware trim. The summarization layer is conversation-level, which ctx does not do.

## Where ctx is better / where it is worse

Better: content-aware per-tool trimming, proof, cross-agent view, edit-intent protection.
Worse: native, free, default; and ctx has no Gemini CLI surface today.

## Threat level to ctx: low to medium

Low as a direct threat because it is not a current ctx surface and its tool trimming is a fallback, not a headline feature. Medium as part of the broader platform-absorption pattern: every major agent now ships some native compaction.

## What ctx should learn or steal

- Tail preservation is a clean idea: protect the most recent working context unconditionally. ctx's edit-intent guard is a more targeted version of the same instinct.
- The failure guard ("stop compressing if it stops helping") mirrors ctx's fail-closed gate philosophy and is a good public talking point: even Google's own compactor knows to back off, but it backs off globally, while ctx backs off per tool with evidence.
