# 0047. Cross-platform compaction events distinguish attempts from completion

- Status: accepted
- Date: 2026-07-19
- Supersedes: ADR 0016 and ADR 0023 where they treat a pre hook as a completed compaction
- Extends: ADR 0037 (Codex plugin surface)

## Context

CTX used three incompatible meanings for "compaction": a Claude transcript pre-compaction marker, a
Cursor `preCompact` hook, and Codex pre/post hooks while the query counted only Codex pre events.
The dashboard collapsed all three into one count. That made Cursor look more certain than its hook
contract allows and ignored the strongest native completion signal on Codex.

Current Claude Code and Codex hook contracts expose both pre and post events. Cursor's current
public contract exposes only `preCompact`.

## Decision

1. Normalize native events into `attempted` and `completed` phases.
2. Count only `completed` as a completed compaction.
3. Report detection and confidence separately: `native_post/confirmed`,
   `native_post+transcript_pre/confirmed`, `native_pre_only/attempt_only`,
   `transcript_pre_only/attempt_only`, or `none/unknown`.
4. Prefer native Claude completion events after the new hook is installed. Retain older transcript
   `pre_compact` markers only as unconfirmed attempts. Exclude transcript markers from sessions that
   already have native events so the attempt count is not double-counted.
5. Derive retry-stable SHA-256 delivery keys and persist metadata only.
6. Keep correction follow-ups nullable unless CTX has a stable turn timeline to join.

## Consequences

- Cursor may show attempts but never a fabricated completion count.
- Claude and Codex can prove current completion with `PostCompact`.
- Historical Claude data remains useful as attempt/pressure evidence, but it never increases the
  completed total or enters correction-after-compaction analysis.
- API clients receive explicit attempted/completed, confirmed, and unconfirmed-attempt fields;
  `compaction_events` remains temporarily as a compatibility alias for total completed events only.
