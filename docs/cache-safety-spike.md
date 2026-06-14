# Cache-safety spike (CTX-28)

- Status: findings + measurement shipped (bucket + per-arm A/B view); simplified after the proxy was removed (CTX-29 / ADR 0015), so the only remaining prefix edit is system injection; injection control arm collecting
- Date: 2026-06-13
- Linear: CTX-28 (under the competitive-analysis epic CTX-24)
- Related: `docs/competitive/competitors/prompt-caching.md`, `docs/competitive/90-open-questions.md`

## The question

ctx saves tokens by editing the request. Prompt caching also saves tokens, but in a
different way: it reuses computation on a stable prefix and bills cached reads at about 0.1x
base input, while a cache write costs 1.25x to 2x. If ctx edits the part of the request that
the provider caches, it can force a cache write and re-process the whole prefix at full price.
That penalty can be larger than the tokens ctx removed.

So the spike asks: does ctx bust the prompt cache, and if so, where and how badly.

## What sits in the cached prefix

Anthropic caches a prefix in this order: `tools`, then `system`, then `messages`. A cache
read matches the longest common prefix up to a `cache_control` breakpoint. If you change any
byte inside that prefix, everything from the change point onward stops matching, so the cache
breakpoints after it miss and that span is re-cached at the write price. Claude Code sets its
breakpoints on the tool and system blocks and on the conversation, so the `tools` and
`system` blocks are exactly the cached prefix.

## What ctx does to each part (grounded in the code)

The key fact: **ctx never edits a request on the wire.** It is hook-first. It influences Claude
Code through hooks and settings, and it never terminates TLS or rewrites the request body. (An
older MITM proxy could edit the `tools` array in flight; it was removed in ADR 0015, after ADR
0011 had already pulled out its only differentiated use.) So nothing ctx does touches the cached
`tools` block directly. Here is what each edit does and where it lands.

1. **MCP tool-schema filtering (soft mode, default).** ctx does not touch the request. It writes
   Claude Code permission rules (`permissions.deny`) so Claude Code itself omits the filtered
   tools when it assembles the request. The `tools` block Claude Code sends is smaller, but ctx
   never edits the wire, so there is no in-flight prefix edit. As long as the rule set is stable,
   the prefix is stable and caches normally. (Strict mode swaps connectors via `allowedMcpServers`
   the same way, also through settings, not the wire.)

2. **System injection.** ctx returns `additionalContext` from the `UserPromptSubmit` hook, which
   Claude Code folds into the system prompt. This fires for the always-on prefix
   (`inject_enabled`) and the conditional coach, behavior-guard, and budget-guard hints. Either
   way the system block Claude Code sends changes, so this is the one thing ctx does that affects
   the cached system prefix.

3. **Tool-output trimming.** The `PostToolUse` path (`src/compress/*`) shortens a tool result
   before it is handed back into the conversation. By the time that text becomes part of a
   cached prefix on a later turn, it is already trimmed, and ctx never rewrites text that was
   cached on an earlier turn in place. So trimming edits content that lives after the prefix,
   and it is cache-safe by position. It is not counted in the audit below.

So the only thing ctx does that changes the cached prefix is system injection (#2), and that is
stable when it is the always-on prefix. Tool filtering (#1) shrinks the prefix through Claude
Code's own settings, never an in-flight edit. Tool-output trimming (#3) never touches the prefix.

## Where the real risk is: instability, not editing

Editing the prefix once is cheap. The danger is editing it differently on every request.

- **A stable smaller prefix is a net win.** If the kept tool set (or the injected system text)
  is the same on every request in the cache window, ctx pays one cache write the first time the
  prefix shrinks, then every later request reads the new, smaller prefix at 0.1x. Steady state
  is cheaper than not filtering at all.

- **An oscillating prefix is a net loss.** If the prefix changes request to request, every
  request is a cache write at 1.25x to 2x instead of a 0.1x read. On a prefix reused many times,
  that is roughly a 12x swing in the wrong direction, which can dwarf the tokens ctx removed.

Two things can cause that oscillation:

- **Auto-profile switching.** `config.auto_profile_enabled` plus `auto_profile_info` can pick a
  different profile per request. If the kept tool set flips within a session, the deny rules flip
  with it, so the `tools` block Claude Code assembles changes and the cache is written each time.
  It is off on this machine (`auto_profile_enabled = false`).
- **Conditional system injection.** The coach, behavior-guard, and budget-guard hints fire only
  on some turns. Each time one fires or stops firing, the system block changes and that turn
  takes a cache write. The always-on prefix injection is fine once it is stable; the
  intermittent hints are the thrash risk.

## Illustrative math (estimate, not measured)

Take a tools-plus-system prefix of about 20K tokens, reused 50 times in a 5-minute window, on
a $3/MTok model.

- No ctx: 1 write + 49 reads = about (1.25 + 49 x 0.1) x 20K x $3/M = about $0.37.
- ctx, stable filter removing 4K of MCP schemas: same shape on a 16K prefix = about $0.30. A
  small win, plus the one-time re-cache.
- ctx, prefix changes every request: 50 writes x 16K x 1.25 x $3/M = about $3.75. About 10x
  worse, and the 4K we saved per request does not come close to paying for it.

The point is not the exact numbers. It is that the sign of ctx's net effect flips on whether
the prefix is stable.

## The measurement instrument

`ctx context cache-audit` (read-only, ships in this change) groups the user's own enriched
requests by what ctx did to the prefix and reports cache behavior per bucket:

```
ctx context cache-audit            # all time
ctx context cache-audit --days 7   # last 7 days
ctx context cache-audit --json     # machine readable
```

Buckets are `untouched`, `tools-filtered`, `system-injected`, and `tools+system`. For each it
shows the share of input tokens that were cache reads (good), cache writes (the warning sign),
and fresh uncached input. The data comes from `hook_traces`, which records `tools_removed` and
`inject_chars` per request and is enriched with `cache_read_tokens` and
`cache_creation_tokens` from the matching turn.

Read the bucket view as a smell test, not proof: the enrichment join matches a request to a turn
by session and time, so it is correlational across real traffic.

When an experiment is running (`[ab_test]` with a feature below 100), the command also prints a
**By experiment arm** section that compares cache-read share, cache-write share, and average cost
between the feature on (treatment) and off (control), per feature, from the `ab_group` tags the
hook records. That is the clean comparison: same machine, same period, randomized per request.
Its one weakness is that prompt caching is stateful across a session and the arms are assigned
per request, so the arms can contaminate each other's cache. A sticky per-session assignment
would tighten it further.

## Recommended guards (decision gated on the audit)

Do not add guards before the data says they are needed. If the audit shows thrash:

1. **Stabilize the tools block.** Pin the auto-profile decision per session so the kept tool set
   does not flip inside a cache window. This keeps filtering a one-time re-cache plus a steady
   win instead of a per-request write.
2. **Make system injection cache-aware.** Keep the injected system text stable across requests
   where possible. For the intermittent hints, only inject when the expected benefit beats one
   cache write, or move the hint somewhere outside the cached prefix.
3. **Leave tool-output trimming alone.** It is cache-safe by position.

## First measurement on this machine (2026-06-13)

There is already an A/B running here (`[ab_test] profile_pct = 50`), so we get something better
than the correlational bucket view: a treatment-vs-control split. `ctx context cache-audit` now
prints both. The prefix-touch buckets and, when an experiment is live, a per-arm comparison.

### What the data confirms about the mechanism

`tools_removed = 0` on all 294 enriched requests, as expected: ctx never edits the `tools`
array. MCP control happens entirely through Claude Code's settings (`permissions.deny`), not by
ctx editing the request body. So ctx is not writing the cached `tools` block on any request. With
the proxy removed (ADR 0015), there is no longer any code path that could.

### Profile A/B (filtering gate), per arm

| arm | requests | cache-read | cache-write | avg cost |
| --- | --- | --- | --- | --- |
| filter on (`P:T`) | 107 | 68.5% | 30.8% | $1.844 |
| filter off (`P:C`) | 103 | 65.6% | 33.8% | $2.151 |

Filter-on has a higher cache-read share, a lower cache-write share, and about 14% lower cost.
That is the opposite of cache-busting, and the cost figure already prices cache reads at 0.1x and
writes at 1.25x, so it is net of caching. Self-tuning's own verdict on this experiment is
`beneficial`.

Three honest caveats:

- **Mechanism is murky.** With `tools_removed = 0` in both arms (ctx never edits the wire) and
  `tools_kept` differing by only one tool, the cost and cache gap is not coming from tool
  stripping. Some of the 14% may be confounded by which sessions and prompts randomly landed in
  each arm. Read it as strong-suggestive, not a clean causal number.
- **Per-request assignment shares one cache.** The cache is stateful across a session, and the
  arms are assigned per request, so they can contaminate each other's cache. A sticky per-session
  assignment would give a cleaner cache read.
- **Injection has no control arm yet.** `inject_pct` was 100, so all 210 requests were
  injection-treatment. It has now been set to 50 to start collecting a control arm; re-run the
  audit after a few days of use to compare.

### Injection, adaptive, coaching

All three were at 100% (always on), so the audit reports a single arm (n=210, 67.0% cache-read,
32.3% write) and no control. The 67/32 split with the system block touched every request is
consistent with a stable prefix plus normal cache turnover, not thrash, but it cannot be a
verdict without a control arm. Flipping `inject_pct` to 50 starts that.

## What is verified vs. what needs your data

- Verified by code: ctx never edits a request on the wire (the proxy is gone, ADR 0015). The only
  edit that touches the cached prefix is system injection. Tool filtering shrinks the prefix
  through Claude Code's settings, and tool-output trimming lands after the prefix.
- Verified by live A/B data on this machine: the profile/filtering gate does not bust the cache
  here. Filter-on shows higher cache-read, lower cache-write, and ~14% lower cost than filter-off,
  net of cache pricing. `tools_removed = 0` throughout, consistent with filtering running through
  settings rather than the wire.
- What is left: a clean verdict on system injection, the one remaining prefix edit. Its control
  arm is now being collected (`inject_pct = 50`); re-run the audit after a few days of use to
  compare the injection-on and injection-off arms.
