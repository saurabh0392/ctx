# Model gateway M5 — evidence, trust, and lifecycle ledger

Status: implementation complete; first-time-user comprehension and live-provider evidence pending
Date: 2026-07-21
Parent: `docs/model-gateway-implementation-plan.md`

## Outcome

M5 makes model-path state inspectable without storing model traffic. The main dashboard now joins
two independent sources:

- the owner-only lifecycle registry proves whether a route is registered, enabled, bypassed, or in
  conflict, and whether its exact loopback service and client setting agree; and
- SQLite stores content-free route evidence for attempted, provider-accepted, applied, held-whole,
  already-shortened, unknown, provider-rejected, transport-failure, and bypass events.

A card says active only when the ownership phase is `enabled`, the service returns the exact nonce-
bound health receipt, and the client setting is still CTX-owned. Shadow is labeled observation only.
Registration alone never appears active.

## Evidence contract

Every event is bound to route id, surface, captured client version, protocol, authentication mode,
fixed upstream, and route mode. Applied receipts also contain exact input/output character counts;
accepted receipts contain local transport latency. Closed outcomes and reason-code token grammar
prevent request bodies, headers, credentials, paths, arbitrary provider errors, or tool output from
entering the table. The table is capped at 20,000 rows.

HTTP acceptance is a successful provider status. SSE acceptance is the first complete `data:`
event, not response headers. Provider rejection and connection failure never create an applied
receipt. Acceptance retries are idempotent by rewind id.

## Main-card explanation

For each current route the dashboard shows:

- exact surface/auth/protocol/client version and fixed destination;
- lifecycle state and whether the route is shadow or testing;
- attempted, accepted, applied, held/unknown, failure, character, token-estimate, and latency proof;
- which client-side result path can cross the adapter and which hosted/direct/unsupported paths do
  not;
- the in-memory visibility boundary, absence of a CTX cloud relay, and raw-request non-persistence;
- local exact-original retention only for accepted trims, plus recovery and purge commands; and
- the immediate route-scoped bypass command.

Disabled routes leave content-free historical proof but no longer claim traffic control.

## Compaction correction

Only a native post event counts as completed compaction. Claude transcript `pre_compact` markers and
Cursor pre hooks are unconfirmed attempts. They never enter the completion total, affected-session
count, or correction-after-compaction denominator. Missing completion visibility stays unknown.

This implements ADR 0047 and removes the prior transcript-inferred completion claim from the API and
dashboard.

## Automated proof

- model-route storage rejects arbitrary reason strings and a seeded bearer secret;
- full route dimensions remain distinct in dashboard summaries;
- HTTP and SSE tests prove attempted, accepted, and applied timing;
- provider rejection and transport failure prove zero accepted/applied receipts;
- exact originals remain absent from route evidence and available through rewind only;
- repeated acceptance cannot double-count applied trims; and
- transcript-only pre markers are tested as attempts with no fabricated completion.

## Remaining M6 evidence

Run live Codex API-key and Claude Code routes on macOS and Linux, capture compatibility receipts,
measure p50/p95 latency and cache-adjusted value, exercise enable/bypass/re-enable/disable/reinstall,
and validate the main-card explanation with first-time beta users. ChatGPT-login, WebSocket, Cursor,
hosted-tool, and unverified provider paths remain held.
