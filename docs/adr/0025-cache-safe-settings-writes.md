# 0025. Cache-safe settings writes: only change the prefix when the tool set changes

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh (with CTO partner)

## Context

Prompt caching discounts the stable request prefix (`tools`, then `system`) at about 0.1x base
input, while a cache write costs 1.25x to 2x. If ctx changes a byte inside that prefix, the cache
breakpoints after the change miss and that span is re-cached at the write price. On a prefix reused
many times that swing can dwarf the tokens ctx removed (CTX-28).

ctx never edits a request on the wire (the MITM proxy was removed in ADR 0015). It influences Claude
Code through settings and hooks. So the only edits that can touch the cached prefix are:

1. MCP schema filtering, expressed as `permissions.deny` rules in `~/.claude/settings.json`. Claude
   Code omits the denied tools when it assembles the request, shrinking the `tools` block.
2. System injection via the `UserPromptSubmit` hook's `additionalContext`, which Claude Code folds
   into the `system` block.

The risk is not editing the prefix once. A stable smaller prefix is a net win: pay one cache write
when it shrinks, then read the smaller prefix at 0.1x. The risk is editing it *differently* on every
request. An oscillating prefix turns every request into a cache write instead of a read, which can be
a roughly 12x swing in the wrong direction.

In soft filter mode (the default), the `UserPromptSubmit` hook resyncs `permissions.deny` on every
prompt through `write_native_ctx_to_user_settings`. With `auto_profile_enabled = true`, the hook also
re-picks a profile per prompt. The write happened unconditionally, even when the resulting deny set
was byte-identical to what was already on disk. That left cache safety as an *observed* property of
the data rather than something the code guarantees.

## Decision

Make the settings write idempotent. `write_json_atomic_if_changed` serializes the target document,
compares it to the current file contents, and skips the write (no temp file, no rename, no mtime
change) when they are equal. `write_native_ctx_to_user_settings` and the observation-only writer use
it.

This makes the cache-safety property a code-enforced invariant: `~/.claude/settings.json`, and
therefore the cached `tools` prefix, changes only when the effective tool set genuinely changes. A
stable profile across a session produces zero settings writes and a byte-stable prefix, no matter how
many prompts run through the hook.

We deliberately do **not** add auto-profile session-stickiness yet. The live cache audit shows no
thrash (the `system-injected` bucket reads 98.8% from cache with 1.0% writes), and the spike's own
rule is to not add that guard until the data shows it is needed. It is documented as the gated next
step: if `ctx context cache-audit` ever shows the cache-write share climbing in the tools-filtered
buckets, pin the auto-profile decision per session so the kept tool set cannot flip inside a cache
window.

## Alternatives considered

- **Leave the unconditional write.** It is cache-neutral when the bytes match (Claude Code sends the
  same `tools` block either way), so it does not bust the cache today. But it leaves the safety
  property unenforced, rewrites the file every prompt (fs-watcher noise, atomic-rename churn), and
  would silently become a cache hazard if any future change made the serialized output vary per
  prompt. Rejected: the invariant is cheap and worth enforcing.
- **Diff only the `permissions` subtree.** More targeted, but more code and more ways to be wrong.
  Comparing the full serialized document is simpler and strictly correct: if anything ctx manages in
  the file changed, we write; otherwise we skip.
- **Pin auto-profile per session now (stabilize the tools block).** This is the right fix *if* the
  audit shows profile flips. It is a behavioral change (the profile would stop adapting mid-session),
  so adding it without data would be premature per the spike's discipline. Deferred and documented.

## Consequences

- Better: a stable profile yields a byte-stable cached prefix by construction, not by luck. No
  per-prompt settings churn. The cache-safety claim is now backed by code, not just a one-time
  measurement (which the data wipe erased anyway).
- Neutral: cache hit rate is unchanged in the steady state, because the skipped writes were already
  producing identical content. The value is the invariant and the removed churn, not a measured
  cache-rate gain. We say so plainly in `docs/cache-safety-spike.md`.
- We now have to keep serialization deterministic for the compare to hold. It is (`to_string_pretty`
  of a given value is stable, and the prior file was written by the same serializer). If it ever
  differs, the guard falls through and writes, so it can never leave stale content; it just stops
  skipping.
- Residual risk lives entirely in genuine tool-set changes: auto-profile flips and growing session
  expansions. Those are exactly what `ctx context cache-audit` measures, and the gated stickiness
  guard is the documented response.
