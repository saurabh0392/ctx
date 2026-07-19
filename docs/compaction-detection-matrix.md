# Compaction detection matrix

Verified against current vendor documentation on 2026-07-19.

CTX treats compaction as a two-step lifecycle:

1. **Attempted** means a pre-compaction hook fired. The platform may still fail, cancel, or skip the
   operation.
2. **Completed** means a post-compaction hook fired, or a persisted transcript contains a structural
   completion marker. CTX always labels which source supplied that fact.

The dashboard never converts an attempt into a completed compaction.

| Platform | Start signal | Completion signal | CTX label | Correction follow-up |
|---|---|---|---|---|
| Claude Code | Native `PreCompact`, matcher `manual|auto` | Native `PostCompact`, matcher `manual|auto` | `confirmed` for native-only data; `inferred` for transcript-only history; `mixed` when both are present, with separate source counts | Available for transcript-inferred history; pending for native-hook-only events |
| Cursor | Native `preCompact` | No public post-compaction hook | `attempt_only`; completion count is `null`, never zero | Unavailable until Cursor exposes completion or a stable transcript marker |
| Codex | Native `PreCompact`, matcher `manual|auto` | Native `PostCompact`, matcher `manual|auto` | `confirmed` | Pending until native events are joined to a stable turn timeline |

Sources:

- Claude Code: [Hooks reference](https://code.claude.com/docs/en/hooks)
- Cursor: [Hooks documentation](https://cursor.com/docs/hooks) and the official
  [`preCompact` hook listing](https://cursor.com/marketplace/hooks/precompact)
- Codex: [Hooks reference](https://learn.chatgpt.com/docs/hooks)

## Delivery de-duplication

Hook delivery can be retried. CTX derives a SHA-256 event key from surface, phase, stable session and
turn identifiers, trigger, platform sequence fields, and (when available) transcript file position.
Summary or instruction text may participate in the one-way digest but is never stored. An identical
delivery is inserted once.

The normalized ledger stores only:

- event key;
- timestamp;
- surface;
- phase (`attempted` or `completed`);
- optional session/turn identifiers; and
- trigger (`manual` or `auto`).

It stores no prompt, transcript content, compact summary, tool output, command, or path.

## OS and surface coverage

The event meaning is identical on macOS, Linux, and Windows. Only command launch differs:

| Integration | macOS / Linux | Windows | Semantic contract |
|---|---|---|---|
| Claude Code settings hook | `ctx hook claude-…-compact` | Installed `ctx` executable through Claude Code command hooks | attempted + completed |
| Cursor hooks file | `ctx hook cursor-pre-compact` | Installed `ctx.exe` through Cursor command hooks | attempted only |
| Codex plugin | `run-ctx.sh` | `run-ctx.ps1` via `commandWindows` | attempted + completed |

Fixture coverage must include manual/auto triggers, pre/post phases, duplicate retries, missing
optional IDs, two compactions in one session, and Cursor's attempt-only behavior. A pre/post pair is
one attempt plus one completion—not two completed compactions.

When Claude transcript ingestion later sees a session already represented by a native hook event,
CTX excludes that session's transcript markers from the inferred count. Transcript-only historical
sessions remain visible after native hooks are installed.

## Known limits

- A platform that exposes only a pre hook cannot prove completion. CTX says so directly.
- Native Claude/Codex completion events do not yet have a portable, stable post-compaction turn
  timeline, so the correction-within-N-turns field remains unavailable for those events.
- Historical Claude transcript inference is structurally stronger than a pre hook but remains
  labeled `inferred`, because transcript formats are platform-owned and may change.
