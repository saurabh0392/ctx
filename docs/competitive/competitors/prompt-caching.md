# Prompt caching (Anthropic and OpenAI)

- Category: prompt caching
- Classification: different mechanism, same budget (it competes for the cost ctx also targets, and it interacts with ctx)
- One-liner: providers store a processed prompt prefix and bill repeats of that prefix at a deep discount, cutting cost without removing any content.
- URL / repo / docs: https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching, https://platform.openai.com/docs/guides/prompt-caching
- Maturity signals: shipped, GA, first-party from Anthropic and OpenAI. The default cost-reduction mechanism for any serious agent.
- License / pricing: a billing feature, not a product. Pricing detail below.

## What it does and how (mechanism)

Prompt caching reduces cost by reusing computation on a stable prefix, not by shrinking context:

- Anthropic: explicit. Mark up to 4 breakpoints with `cache_control: { type: "ephemeral" }`. Cache reads cost 0.1x base input (a 90% discount). Cache writes cost 1.25x base input for a 5-minute TTL or 2x for a 1-hour TTL. Example (Sonnet 4.6, $3/MTok base): a 50K-token system reused 100 times in 5 minutes drops from $15.00 to about $1.68. Source: Anthropic pricing and Respan, accessed 2026-06-13. Label: vendor pricing, observed.
- OpenAI: automatic and implicit. Any prompt over 1,024 tokens has its longest matching prefix cached for roughly 5-10 minutes; cached input on GPT-5 family bills at about 0.1x base, with no cache-write premium. No code changes. Source: OpenAI docs and Respan, accessed 2026-06-13.

Where it sits: inside the provider's billing and inference. It requires a stable leading prefix. Any change to prefix tokens busts the cache.

## Why it matters to ctx (the interaction, not just the comparison)

This is the single most important cross-cutting fact in the whole analysis for ctx's engineering and messaging:

- Caching makes the stable prefix (system prompt, tool definitions, early history) cheap to resend. Compression makes the prefix smaller. They can fight each other. If ctx (or any compressor) edits content inside the cached prefix, it busts the cache, and the user can end up paying full price on a much larger share of tokens, wiping out the savings.
- Claude Code's cached microcompact is explicitly designed to drop tool results without invalidating the cached prefix. That is the bar. A naive trimmer that rewrites cached content is worse than doing nothing.
- ctx trims tool results, which generally appear after the cached prefix (in the conversation, not the system prompt), so ctx's trimming is largely cache-compatible by position. But ctx's MCP schema filtering edits tool definitions, which usually sit in the cached prefix. Filtering them changes the prefix and can bust the cache. This needs verification and is logged in 90-open-questions.md.

## Claimed results vs verifiable results

- Verifiable: the discount structure (0.1x reads; 1.25x/2x writes on Anthropic; automatic on OpenAI over 1,024 tokens) is documented provider pricing (observed, accessed 2026-06-13).
- The interaction risk (compression busting cache) is mechanically certain and is acknowledged by the field (Kompact ships a "cache aligner"; Claude Code ships cached microcompact). Label: observed mechanism, ctx-specific impact not yet measured.

## Strengths (as a competing approach to the same budget)

- Zero quality risk. It removes no content; the model sees the same prompt. It cannot cause a correction.
- First-party, default (OpenAI automatic), deep discount (about 90%).
- Composes with everything.

## Weaknesses and blind spots

- It only helps the repeated, stable prefix. It does nothing for one-off large tool outputs, growing conversation, or context-window pollution. A 40K-token `git diff` read once is not cached-away.
- It does not reduce context-window pressure at all; the tokens are still there, just cheaper. It does not help with model focus degradation in long sessions.
- Cache writes cost a premium; low-reuse content can cost more.

## Overlap with ctx

Conceptually adjacent (both cut cost), mechanically opposite (cache keeps content and discounts it; ctx removes content). They are complementary when designed carefully and adversarial when designed carelessly.

## Where ctx is better / where it is worse

Better: ctx reduces context-window pressure and one-off tool-output bloat, which caching cannot touch, and it helps model focus, not just cost.
Worse: caching is risk-free and free-by-default; ctx must prove it does not hurt. For the stable prefix, caching is simply the right tool and ctx should not fight it.

## Threat level to ctx: low (as a competitor), high (as a constraint)

Not a competitor in the usual sense. But it is a hard design constraint. The honest customer message is "turn on prompt caching for your stable prefix; use ctx for the tool-output and context-pollution problem caching cannot solve, and ctx is built to not bust your cache."

## What ctx should learn or steal

- Cache-safety must be a first-class design rule. Verify that ctx trimming and especially MCP schema filtering do not invalidate the prompt cache (open question and a likely follow-up ticket).
- Messaging: position ctx and caching as a stack, not a choice. "Cache the prefix, trim the tool output, prove it was safe."
- Adopt the cache-aware pattern Claude Code's microcompact uses: edit what sits after the cached prefix; leave the prefix intact unless the savings clearly beat the re-write cost.
