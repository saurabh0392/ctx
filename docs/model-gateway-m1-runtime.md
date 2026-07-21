# Model gateway M1 transparent runtime ledger

Status: implementation complete; live provider evidence pending

Date: 2026-07-21

Scope: provider-neutral HTTP/SSE pass-through with transformations hard-disabled

## What exists

M1 adds a separate loopback model-gateway process without enabling it during `ctx setup` and
without editing any coding-client profile:

- a private, versioned route registry at `~/.ctx/model-gateway-routes.json`;
- one dedicated `127.0.0.1` port, surface, protocol, auth identity, exact request path, and fixed
  provider class per route;
- compiled OpenAI and Anthropic destinations only—route input cannot provide an arbitrary URL;
- exact-path `POST` relay for `/v1/responses` and `/v1/messages`;
- byte-preserving request bodies under a 16 MiB bound;
- streaming response relay suitable for SSE without buffering the whole response;
- a content-free `GET /__ctx/health` route receipt that distinguishes listener readiness from an
  unverified upstream;
- redirect refusal, ambient-proxy refusal, browser-Origin refusal, forwarded-host removal, and
  standard plus `Connection`-nominated hop-by-hop header removal; and
- graceful foreground shutdown on Ctrl-C.

At the M1 boundary every route stored `transformationsEnabled: false`; loading a hand-edited true
value failed closed. M3 supersedes that schema with explicit `shadow` and narrow `testing` modes.
M1 itself calls no trimming strategy and writes no applied/savings receipt.

## Commands

The registry is intentionally separate from surface activation:

```text
ctx model-gateway add-route <id> \
  --surface codex \
  --protocol openai-responses \
  --authentication api-key \
  --upstream openai \
  --port 8871

ctx model-gateway list-routes
ctx model-gateway serve <id>
ctx model-gateway remove-route <id>
```

`add-route` changes only CTX-owned state. It prints the client base URL, accepted path, and fixed
destination, but it does not put that URL into Codex or Claude Code. For Codex the displayed base
includes `/v1`; for Claude Code it is the loopback origin because the clients compose provider
paths differently.

Cursor registration is rejected because M0 holds that route. OpenAI Chat Completions is part of
the protocol vocabulary but cannot be registered for a Wave 1 surface until Cursor or another
surface earns a compatibility decision.

## Fidelity and isolation evidence

Fake-upstream tests prove:

- recorded OpenAI Responses and Anthropic Messages bodies arrive upstream byte-for-byte, including
  whitespace, with their respective authorization headers;
- authorization reaches the fixed upstream while `Host`, `Forwarded`, `X-Forwarded-*`, `X-CTX-*`,
  and hop-by-hop routing controls cannot redirect it;
- an unexpected path or method is rejected before any upstream request;
- oversized and browser-originated requests are rejected before upstream;
- provider redirects are returned to the client and never followed by CTX;
- provider error status, rate-limit headers, and body are returned unchanged;
- the first SSE event reaches the client before a delayed second event, and both bytes are
  unchanged; and
- the health receipt performs no upstream request and contains no prompt or credential data.

Registry tests prove duplicate ports, Cursor routes, unknown surface/protocol/provider combinations,
arbitrary provider URLs, privileged ports, and hand-enabled transforms fail closed. The registry is
written mode `0600` on Unix and contains no credential field or value.

## Evidence boundary

M1 proves the local runtime against isolated fake upstreams. It does not yet prove:

- live OpenAI, Anthropic, ChatGPT-login, or Claude subscription acceptance;
- WebSocket relay;
- request streaming above the bounded buffered body;
- a client profile's setup, bypass, restoration, or uninstall path;
- protocol parsing, tool-result detection, shadow decisions, mutation, or rewind; or
- cache, latency, and long-session behavior on a beta corpus.

No model route is installed or active on this machine merely because this code exists. Live
provider smoke tests and sanitized captures remain required before M1's plan exit gate closes.
