# Model gateway M4 — Wave 1 surface lifecycle ledger

Status: implementation complete against isolated client profiles and service definitions; live
beta-user lifecycle proof pending
Date: 2026-07-21
Parent: `docs/model-gateway-implementation-plan.md`

## Outcome

M4 turns a registered Claude Code or Codex route into an explicit, recoverable user-level client
transaction. It does not silently enable model-path mode during ordinary `ctx setup`.

The supported flow is:

```text
ctx model-gateway probe --surface codex --run-client-version
ctx model-gateway setup codex --authentication api-key --mode shadow
ctx model-gateway enable codex-api --yes
ctx model-gateway status codex-api
ctx model-gateway doctor codex-api
ctx model-gateway bypass codex-api
ctx model-gateway enable codex-api --yes
ctx model-gateway disable codex-api
```

Claude Code uses the equivalent `setup claude-code` flow with `api-key`, `bearer-token`, or
`subscription`. Cursor returns a specific unavailable receipt because M0 did not find a documented
programmable routing boundary. Its standard hook and MCP paths remain separate and unchanged.

## Ownership boundary

CTX owns the smallest documented routing footprint each supported surface needs:

| Surface | User-level field | First M4 routes |
| --- | --- | --- |
| Codex | CTX-owned `model_provider` selection plus a newly created `model_providers.ctx-model-gateway` table in `~/.codex/config.toml` | OpenAI Responses over forced HTTP/SSE; API key only |
| Claude Code | `env.ANTHROPIC_BASE_URL` in `~/.claude/settings.json` | direct Anthropic Messages; API key, bearer token, or subscription receipt |
| Cursor | none | unavailable until a captured supported boundary exists |

Codex uses a CTX-owned custom-provider stanza with `requires_openai_auth = true` and
`supports_websockets = false`. The explicit false matters: the current relay supports HTTP/SSE but
does not yet terminate Responses WebSocket sessions. Base-URL-only activation and ChatGPT login stay
held until their WebSocket, fixed-backend, refresh, and account-UI contracts pass live capture.

CTX never copies the whole client configuration into its ownership registry. That prevents API
keys, bearer tokens, hooks, MCP definitions, prompts, and arbitrary user settings from entering CTX
lifecycle state. A pre-existing base URL is recorded only when it is a credential-free official
OpenAI or Anthropic URL. Custom, cloud-provider, malformed, credential-bearing, query-bearing, and
already-loopback routes fail closed.

## Atomic enable order

1. Validate the immutable CTX route and exact surface/auth matrix.
2. Run the client's version probe and prepare the field-scoped restoration receipt.
3. Persist an owner-only `prepared` receipt with a random listener identity; persist no credential.
4. Install the per-route launchd or systemd user service.
5. Require a health receipt matching route, surface, protocol, auth, destination, and listener
   identity within five seconds.
6. Atomically write only the supported client routing field.
7. Mark the transaction `enabled`.

If service installation or proof fails, CTX removes the service and prepared receipt without
changing the client. If the process crashes after the prepared receipt, doctor and bypass retain the
information needed to recover safely.

## Bypass, disable, reinstall, and uninstall

- `bypass` restores the original field first and retains the service/receipt for explicit re-enable.
- `disable` restores first, stops/removes the service, removes the exact unchanged route, then removes
  ownership.
- a user edit to the owned field is never overwritten; restoration stops with a conflict and keeps
  the receipt for diagnosis.
- `remove-route` refuses while the route owns a client field.
- `ctx setup --uninstall` disables every owned model route before removing CTX services or binaries.
- setup is idempotent when the generated route already matches, supporting reinstall without route
  duplication or reauthentication.

The service definition stores the content-free route id and listener identity. It contains no API
key, bearer token, authorization header, prompt, tool result, or source content. The gateway still
sees the client's request and authorization headers transiently in memory while forwarding them to
the fixed displayed provider; enablement requires explicit `--yes` consent.

## Proof included in this increment

- Codex TOML comments, selected model, plugins, hooks, existing provider tables, and MCP tables are
  preserved; the prior provider selection is restored.
- Claude Code hooks and a seeded API-key value survive enable/restore but never enter the ownership
  receipt.
- both adapters refuse destructive restoration after a user edit.
- clean profiles and profiles customized in unrelated fields restore without authentication state
  changes.
- route setup is idempotent and Cursor stays held.
- launchd definitions and health receipts contain no credential fields.
- the full repository test suite and strict clippy pass.

## Remaining live evidence

The code does not by itself prove provider authentication, refresh, account UI, or a real model turn.
Before M4's product exit is claimed, run the beta-user lifecycle on clean and customized macOS and
Linux profiles for:

- Codex API key first, then ChatGPT login only after the held backend/WebSocket gate is implemented;
- Claude Code subscription/bearer and API key independently;
- enable, an unchanged turn, a model-visible testing turn, bypass, re-enable, disable, reinstall,
  uninstall, and login continuity; and
- port conflict, fake listener, service crash, user edit, invalid credential, and provider rejection.

Cursor remains unavailable rather than borrowing evidence from the reusable Chat Completions pack.
