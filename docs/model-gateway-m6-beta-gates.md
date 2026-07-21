# Model gateway M6 — Wave 1 beta and commercial gates

Status: gate implementation and automated corpus complete; live/external exit evidence open
Date: 2026-07-21
Parent: `docs/model-gateway-implementation-plan.md`

## Outcome

M6 adds a fail-closed, content-free readiness report:

```bash
ctx model-gateway readiness
ctx model-gateway readiness --json
```

The stable JSON schema is `ctx.model-gateway-readiness.v1`. It joins the exact lifecycle receipt,
full route evidence identity, applied-decision/recovery integrity, local recovery-copy totals, and
external release gates. It cannot make an unsupported route commercially ready.

## Route-level private-beta gates

Each enabled route must pass all of these before the command calls it private-beta ready:

| Gate | Current threshold | Evidence source |
| --- | --- | --- |
| Exact lifecycle health | enabled + CTX-owned client field + nonce-bound healthy service + matching immutable registry | ownership and live health receipt |
| Captured client version | present | enable-time executable probe |
| Provider acceptance corpus | at least 20 accepted requests | HTTP 2xx or first complete SSE data event |
| Transport reliability | at least 20 attempts and no more than 0.1% provider, transport, or gateway-processing failures | content-free runtime events |
| CTX processing p95 | at most 200 ms | inspection/preparation time before upstream send; excludes provider latency |
| Transform deadline | exact original sent after 500 ms | bounded blocking worker with an async fail-open deadline |
| Exact recovery | zero applied decisions without rewind data and no applied receipt/decision mismatch | SQLite integrity join |
| Accepted trim, testing routes | at least one applied trim with a positive exact character delta | atomic apply and route receipt |

The readiness report separately exposes total time to provider acceptance. It does not compare that
number to CTX's added-latency limit. Cache-adjusted value remains an external live-corpus gate; exact
character savings are not presented as a provider-cost claim.

Prepared recovery copies are counted separately from applied trims. CTX persists the original before
sending a proposed trim. Provider rejection or an uncertain network result can therefore leave an
unapplied recovery copy; only provider acceptance creates the applied decision and receipt.

Inspection and request preparation run outside the async relay task. If they do not complete within
500 ms, the relay sends the exact original request and records `transform-deadline`; the detached
worker is cooperatively cancelled before observation and before every durable prepare. If a storage
transaction was already underway at the exact deadline, it may finish as a prepared but unapplied
recovery copy under the same local retention and purge controls.

## Automated corpus now covered

- OpenAI Responses and Anthropic Messages HTTP/SSE pass-through fidelity;
- synthetic model-visible trim on the narrow testing contract;
- evidence-authorized Shell behavior and held mutation/error/ambiguous shapes;
- direct and canonical MCP result shapes, large JSON bounds, repeated history, duplicate/missing call
  IDs, already-shortened guards, and exact JSON-leaf patching;
- provider rejection, redirects, transport failure, fragmented SSE, cancellation-safe streaming,
  browser-origin rejection, wrong paths/methods, oversized bodies, and ambient-proxy disablement;
- exact rewind, acceptance idempotency, rejected/unapplied recovery accounting, and seeded-secret
  storage rejection;
- clean/customized Codex and Claude configuration restore, fake listener, ownership identity,
  bypass/disable/uninstall, launchd, and systemd definitions; and
- Claude/Codex native post completion versus Claude transcript/Cursor pre attempts.

These are deterministic local/mock proofs. They do not substitute for a real provider or beta-user
corpus.

## Wave 1 compatibility decision

| Surface/auth/transport | Code state | Private-beta claim today | Remaining evidence |
| --- | --- | --- | --- |
| Codex API key, OpenAI Responses HTTP/SSE | implemented | experimental; readiness command must pass on the exact client version | real OpenAI multi-turn corpus, cache value, macOS/Linux lifecycle |
| Codex ChatGPT login | held | unavailable | fixed ChatGPT backend, refresh/account UI, HTTP/WS capture |
| Codex Responses WebSocket | held | unavailable | frame fidelity, reconnect, cancellation, auth, acceptance contract |
| Claude Code API key/bearer/subscription, Anthropic Messages HTTP/SSE | implemented | experimental; auth modes earn evidence independently | real Anthropic auth/refresh and fast/safety bypass corpus |
| Cursor model path | held | unavailable | documented programmable route and captured wire/auth contract |
| Provider-hosted tools | outside client boundary | unavailable and uncounted | cannot be made local by this gateway |
| macOS service lifecycle | implemented and isolated-tested | live beta pending | clean/customized install corpus |
| Linux systemd lifecycle | implemented and unit-tested | live beta pending | distro/service/provider corpus |
| Windows | no M4 service contract | unavailable for Wave 1 | service, routing, transport, install/uninstall proof |

Hosted results outside the routed client request are not observable. The product reports them as an
unavailable path, not a zero count.

## External commercial gates intentionally open

The readiness schema hard-fails commercial release until a release commit attaches and changes each
of these evidence decisions:

- clean and customized macOS live corpus;
- Linux provider and systemd lifecycle corpus;
- real-provider cache-adjusted value;
- signed/notarized release artifacts;
- release-bound SBOM and dependency-audit receipt;
- independent model-gateway security review; and
- beta cohort value plus first-time-user route/trust/recovery/bypass comprehension.

The current unsigned beta cannot return `commercialReady: true`. Completing an external activity
requires a reviewed code change to its release gate, so a local user cannot self-certify a build by
editing a mutable data file.

## Exit still pending

M6's implementation is complete, but its product exit is not. Run Codex API-key and Claude Code
routes with real providers, exercise the complete lifecycle on macOS and Linux, gather route-scoped
randomized outcomes and cache receipts, sign the artifacts, produce the release SBOM/audit, obtain
independent review, and run cohort/comprehension tests. Until then the honest state is experimental
with machine-visible blockers.
