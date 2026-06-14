# OpenAI Codex CLI (native compaction)

- Category: platform-native compaction
- Classification: direct competitor (native), and a future ctx surface
- One-liner: OpenAI's terminal coding agent with automatic context compaction at about 90% of the window plus a configurable tool-output token cap.
- URL / repo / docs: https://github.com/openai/codex, https://developers.openai.com/codex/config-reference
- Maturity signals: 90,868 GitHub stars (observed, GitHub API, 2026-06-13). License Apache-2.0. Very active (last push 2026-06-14, GitHub API). Backed by OpenAI.
- License / pricing: open-source CLI (Apache-2.0); the user pays for model usage. Compaction is a free built-in.

## What it does and how (mechanism)

Codex auto-compacts when token usage reaches about 90% of the model's context window. `model_auto_compact_token_limit` can lower the threshold but cannot exceed the window; there is no supported off switch (OpenAI maintainer, codex issue #10365, 2026-02-02). Compaction is server-side by default (`remote_compaction` feature flag controls remote vs local; OpenAI recommends leaving it on).

Separately, `tool_output_token_limit` (default around 128,000, configurable) caps how much of a single tool's output is fed back. This is a native, blunt tool-output truncation, the same general lever as ctx's trimming but without per-result intelligence or proof.

A 2026 commit adds a `body_after_prefix` scope so the compaction budget can target growth since the last compaction rather than the whole carried prefix (openai/codex commit 80fdd46, accessed 2026-06-13), which shows OpenAI is actively refining how compaction interacts with carried context and caching.

Known rough edges (observed from issues): auto-compaction checks token count between turns, not between tool calls within a turn, so a single turn with many large tool outputs can blow the window before compaction fires (codex issue #16033). Misconfigured `model_context_window` overrides cause thrashing.

## Claimed results vs verifiable results

- Claimed (vendor): compaction keeps long sessions inside the window. No public per-task quality delta. Label: vendor claim.
- Verifiable: the 90% trigger, the no-off-switch behavior, `tool_output_token_limit`, and the between-turns-only check are documented in maintainer comments, the config reference, and source-level issue analysis (observed, accessed 2026-06-13).

## Strengths

- Default, free, server-side, in a fast-growing CLI (90K+ stars).
- A real tool-output cap exists natively (`tool_output_token_limit`).
- Tight model integration; OpenAI iterates quickly.

## Weaknesses and blind spots

- Opaque and coarse. Codex compaction has been described as near-total summarization with little visibility into what was dropped (strategy doc cites about 99.3% ratio; treat the exact figure as an internal estimate, not verified here). The tool-output cap is a hard truncation, not a structure-aware trim.
- No off switch and known trigger gaps (within-turn overflow). Users report running out of context despite compaction.
- No proof and no cross-agent view. Self-grading, single-vendor.
- The blunt `tool_output_token_limit` can cut the wrong content (it truncates by token count, not by what matters).

## Overlap with ctx

`tool_output_token_limit` overlaps directly with ctx's tool-output trimming, but it is a fixed cap, not an earned, content-aware, per-tool decision. Codex compaction overlaps with the conversation-level compaction ctx does not do (ctx deliberately does not build a compactor; see strategy doc).

## Where ctx is better / where it is worse

Better:
- Content-aware, per-tool trimming that keeps errors and the lines that matter, instead of a flat token cap.
- Proof on the user's behavior, and the planned compaction-harm detector that could measure Codex's own compaction.
- Cross-agent normalization.

Worse:
- ctx does not yet have a Codex transport (roadmap E2.3 is gated on whether real Codex usage justifies it). Until then, ctx does not act on Codex at all.
- Native, free, default, server-side.

## Threat level to ctx: medium

High as a category force (it is a major platform), but medium as a direct threat today because ctx has no Codex surface yet and Codex's native trimming is crude. If OpenAI makes `tool_output_token_limit` content-aware or ships quality signals, the threat rises. The cross-agent and proof angles remain ctx's defense.

## What ctx should learn or steal

- Codex's blunt `tool_output_token_limit` is the strawman ctx should beat in messaging: "a flat cap throws away the wrong lines; ctx keeps the errors and the lines that matter, and only after it earns the trim."
- Gate the Codex adapter on real usage (roadmap E2.3). Do not build transport for a surface no ctx user runs.
- The within-turn overflow bug is a reminder that tool-output volume inside one turn is a real failure mode; ctx trimming arriving per-result is well placed to help, which is a positioning point.
