# 0023. Cursor compaction via the preCompact hook, at lower confidence

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-31 (persist Cursor compaction so it graduates from unknown)
- Extends: ADR 0016 (compaction-harm detector), ADR 0018 (Cursor as a live hook surface)

## Context

ADR 0016 shipped the compaction-harm detector Claude-first and showed Cursor as honest-unknown ("not visible yet"), with a named follow-up to make Cursor real. ADR 0016 read Claude's compactions from the transcript (`compactMetadata` -> `pre_compact` turn), and assumed Cursor would be persisted the same way once we parsed its turns.

That assumption was wrong. A spike on real Cursor transcripts (CTX-31) found Cursor's `.jsonl` carries only `user` and `assistant` rows with tool calls and text nested in content. There is no `system` row, no `compactMetadata`, no structural compaction marker at all. So the transcript path for Cursor compaction is a dead end.

Cursor does, however, expose a live `preCompact` hook event, fired just before it compacts a conversation, with a rich payload: `trigger` (auto/manual), `context_usage_percent`, `context_tokens`, `context_window_size`, `message_count`, `messages_to_compact`, `is_first_compaction`, and the `conversation_id`. This is the only honest signal that a Cursor compaction happened.

## Decision

Capture Cursor compactions live from the `preCompact` hook and report them at a distinct, lower confidence.

1. Register a second ctx-owned Cursor hook. Setup writes both `postToolUse` (ADR 0018) and `preCompact` into `~/.cursor/hooks.json`, idempotently, and uninstall strips both. The `preCompact` entry calls `ctx hook cursor-pre-compact`.

2. Persist each event in a dedicated `cursor_compactions` table (not `turns`). Cursor has no persisted turn timeline, and the payload's natural key is `conversation_id` with `message_count` as the conversation position. A dedicated table keeps the Claude path (transcript-derived turns) and the Cursor path (live events) cleanly separate. Every metric is optional and recorded as NULL when absent, so a row never overstates what Cursor told us. The handler is purely observational: it never blocks or alters the compaction and always emits `{}`.

3. Report Cursor at a new `observed_low` confidence. The detector counts real `cursor_compactions` rows, but the correction follow-up (did a correction land within N turns after?) is not computed yet, because Cursor turns are not persisted with a correction signal. So `followed_by_correction` is `None` (explicitly, not a fake `0`) and the surface reads `observed_low`, rendered as "watching live" rather than "observed here". A surface is still `unknown` only when ctx has seen no Cursor activity at all; once any live Cursor decision exists, zero compactions is an honest "none yet", parallel to Claude's observed-zero.

## Alternatives considered

- **Persist Cursor turns and reuse the Claude transcript path.** Rejected: the transcript has no compaction marker, so there is nothing to persist from it. The live hook is the only source.
- **Reuse the `turns` table for Cursor compactions.** Rejected for now: `turns` is built around a per-session turn index from the Claude transcript. Cursor has no such persisted timeline, so forcing its events in would muddy both. Revisit if/when we persist full Cursor turns for the correction join (increment 2).
- **Report Cursor at the same "observed" confidence as Claude once we have counts.** Rejected as dishonest: a live pre-event count without the correction follow-up is genuinely weaker evidence than Claude's after-the-fact transcript measure. The `observed_low` tier and "watching live" label say so plainly.
- **Block or defer the compaction from the hook.** Rejected: `preCompact` fires before a compaction the platform owns. ctx measures, it does not interfere.

## Consequences

- Cursor graduates from "not visible yet" to a real, live compaction count on the compaction view, with the lower confidence shown honestly. This is the follow-up ADR 0016 promised.
- New honesty surface to keep clean: `observed_low` must never render as a clean "m of n" result, and must not imply causation. The correction follow-up for Cursor is a named increment 2; until then the card says "whether a correction followed is coming next".
- A pre-event signal can in principle fire for a compaction that is then aborted. In practice Cursor fires `preCompact` immediately before compacting; if this proves noisy we can reconcile against `is_first_compaction` / `message_count` later. Acceptable for a lower-confidence count.
- The detector and the master switch stay aligned: like the Cursor `postToolUse` hook, `preCompact` recording is gated on `compress_enabled`, so a user who turns ctx off stops all collection.
