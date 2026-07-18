# CTX beta privacy notice

Effective: 2026-07-17

CTX is local by default. It has no account system and no background telemetry.

## Stored locally

Depending on enabled settings, CTX may store session/tool metadata, transform decisions, aggregate
token counts, prompts or prompt-derived text, and verbatim originals of applied trims in `~/.ctx`.
Settings allow prompt storage and embeddings to be disabled, prompt-derived fields to be purged, and
indexed data to be deleted.

## Optional sends

Nothing is sent merely because CTX is running. Network sends happen only after a user chooses an
explicit action:

- checking for or installing a beta update sends the participant ID, release target, and version request;
- a beta check-in sends the exact `ctx.beta-checkin.v1` JSON shown in the preview plus four answers;
- an issue report sends text/attachments the user provides and the reviewed diagnostic bundle;
- screenshots are uploaded only after the user chooses Send.

The aggregate beta snapshot excludes prompts, tool output, command text, file paths, repo names,
tool/MCP names, source code, cost amounts, and arbitrary JSON.

## Retention

- Aggregate beta check-ins: up to 365 days.
- Optional issue-report screenshots: up to 30 days; links placed in the private issue expire after seven days.
- Private GitHub issue text: retained for triage until resolved or deletion is requested.
- Local data: until the user deletes it. Uninstall removes integrations and the beta credential but does not wipe the database.

## Identifiers and access

The service uses a pseudonymous 16-hex participant ID derived from the one-time invite token. The
operator keeps the ID-to-cohort-label roster in encrypted AWS SSM, outside the repository. Removing
that roster entry revokes future beta access.

For access or deletion requests, email `saurabhsharan03@gmail.com` with the participant ID shown by
`ctx doctor`.
