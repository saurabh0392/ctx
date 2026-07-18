# ADR 0037: Codex plugin surface

Status: accepted
Date: 2026-07-18

## Decision

CTX integrates with Codex through a plugin. It observes supported local tool results through
`PostToolUse`, records native compaction lifecycle events, exposes the existing local MCP server,
and acts only by rewriting conservative shell commands in `PreToolUse` to the surface-aware
`ctx run` wrapper.

Automatic permission to trim is keyed by `surface + normalized tool + transform version`. Codex
cannot inherit an activation verdict from Claude Code or Cursor.

## Verified contract

The following was exercised against the installed Codex CLI 0.144.5, not inferred from docs:

| Capability | Result |
| --- | --- |
| `PostToolUse` receives Bash output | Verified |
| `PostToolUse` receives local MCP output | Verified |
| `PostToolUse.additionalContext` reaches the agent | Verified |
| `PostToolUse.updatedMCPToolOutput` replaces MCP output | Unsupported; Codex rejects it |
| `PostToolUse` blocking decision substitutes textual feedback | Verified; error-shaped and not shipped as a clean replacement path |
| `PreToolUse.updatedInput` rewrites a local Bash call | Verified |
| Hosted-tool interception | Unsupported by the local hook path |
| Compaction event payload detail beyond documented common ids | Unknown; stored fields remain optional |
| Unified-exec polling behavior across all Codex surfaces | Unknown; stable event-key deduplication is fail-open |

Sanitized payloads for the verified Bash and MCP result shapes live in `tests/fixtures/codex/`.
`updatedMCPToolOutput` returned an explicit runtime rejection while a positive-control
`additionalContext` response succeeded. A later live probe verified that a blocking decision can
replace what the model sees with textual feedback, but Codex reports the tool call as blocked/error
and the contract differs in code execution mode. CTX therefore does not ship that behavior as a
clean result replacement path.

## Consequences

The Codex dashboard status can be `observing` or `partially_active`. It must not report full
`active` parity while built-in and MCP results remain observation-only. Transcript parsing is not a
live dependency because Codex does not promise its transcript format as a stable integration API.
