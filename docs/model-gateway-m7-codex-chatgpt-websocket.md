# M7 — Codex ChatGPT-login and Responses WebSocket route

Status: implemented with local protocol, lifecycle, rejection, exact-recovery, and first live
ChatGPT-account proof; commercial corpus pending
Date: 2026-07-21
Owner: Saurabh

## Product decision

Codex users should not have to leave their existing ChatGPT subscription login or add separate
API-key billing to try CTX. CTX therefore owns a second, explicit Codex route identity:

```text
route                  codex-chatgpt
client base            http://127.0.0.1:8873/backend-api/codex
fixed HTTP upstream    https://chatgpt.com/backend-api/codex/responses
fixed WebSocket        wss://chatgpt.com/backend-api/codex/responses
authentication         chatgpt-login, forwarded transiently by Codex
```

This remains an opt-in loopback reverse proxy. It is not a certificate-authority MITM, ambient
proxy, DNS override, user-selectable forward proxy, or CTX cloud relay.

## Implemented contract

- `ctx model-gateway setup codex --authentication chatgpt-login --mode shadow` registers
  `codex-chatgpt` without changing the user's login method.
- Activation installs a CTX-owned Codex provider with `wire_api = "responses"`,
  `requires_openai_auth = true`, and `supports_websockets = true`.
- HTTP POST, SSE, and WebSocket upgrades are accepted only on the exact compiled-in Responses path.
- Codex's read-only model catalog request is accepted only as `GET /backend-api/codex/models` and
  forwarded to that same fixed ChatGPT origin. Realtime calls, analytics, and arbitrary sibling
  endpoints remain denied.
- Codex account, usage, refresh, and workspace APIs continue to use Codex's separate
  `chatgpt_base_url` client directly. CTX does not rewrite that setting or interpose on those calls.
- Authorization and ChatGPT account headers exist only in relay memory and are forwarded to the
  fixed upstream. CTX does not read or persist the access token.
- Browser-origin WebSocket requests, alternate paths, redirects, forwarded-routing headers, CTX
  control headers, oversized messages, invalid upgrades, and unsupported route/auth pairings fail
  closed.
- WebSocket text, binary, ping, pong, close, selected subprotocol, query string, and Codex handshake
  metadata are relayed with bounded message, frame, and write buffers.
- Each `response.create` is observed independently. A prepared trim counts as applied only after the
  first non-error provider event; failure and disconnect paths retain recovery without claiming an
  apply.
- Bypass, disable, uninstall, ownership-conflict detection, and exact config restoration use the
  existing model-gateway lifecycle transaction.

## Automated proof

The test suite covers:

- fixed ChatGPT upstream and local base identities;
- auth/target mismatch rejection;
- comment-, MCP-, and model-preserving Codex config activation and restoration;
- ChatGPT-login versus forced API-login conflict refusal;
- multi-turn WebSocket ordering and exact text/binary frame forwarding;
- transient Authorization and account-header forwarding with route-header stripping;
- safe upstream handshake metadata forwarding;
- browser-origin denial before upstream contact;
- upstream upgrade status preservation without provider-body or seeded-secret leakage; and
- a testing-mode WebSocket trim accepted by the mock provider with exact rewind recovery.

## First live proof

Codex CLI `0.145.0-alpha.27` passed the first macOS smoke on 2026-07-21:

- `codex login status` remained `Logged in using ChatGPT` before enable, after bypass, and after
  re-enable;
- the first real turn exposed a denied `/models` request, which became the narrow fixed GET metadata
  route and received a regression test;
- the rerun selected `ctx-model-gateway`, completed through WebSocket, and returned the exact
  expected response without a model-catalog error;
- lifecycle doctor reported enabled, CTX-owned configuration, healthy service, and matching route;
- content-free evidence recorded four attempted and four accepted WebSocket requests, zero provider
  rejections, zero transport failures, and one explicit bypass; and
- no active model-gateway receipt contained the seeded prompt or credential material.

The gateway's no-raw-request receipt is scoped to model-gateway storage. CTX's separate local session
analytics and recovery stores are not content-free; the general database remains sensitive and is
covered by separate retention and purge controls.

## Remaining commercial gate

Refresh, account switching, logout, long-session cancellation/reconnect, concurrent agents, the
20-request beta corpus, and cross-platform service behavior remain commercial gates rather than
claims implied by the first live smoke.
