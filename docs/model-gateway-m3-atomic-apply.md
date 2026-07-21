# Model gateway M3 atomic apply ledger

Status: implementation complete against mock upstreams; live provider acceptance pending

Date: 2026-07-21

Scope: narrow testing-mode mutation with durable recovery and upstream-accepted receipts

## Activation boundary

The route registry is now version 2 and gives each route an explicit mode:

- `shadow` keeps M1 byte-identical forwarding and M2 observation only; and
- `testing` permits only M3's named contracts.

Version-1 registries migrate to `shadow` in memory. A legacy file with
`transformationsEnabled: true` is rejected rather than silently promoted. Adding a testing route is
an explicit CTX-owned state change:

```text
ctx model-gateway add-route codex-testing \
  --surface codex \
  --protocol openai-responses \
  --authentication api-key \
  --upstream openai \
  --port 8871 \
  --mode testing
```

M3 authorizes two narrow result contracts:

1. `ctx_synthetic_echo` with `{"contract":"ctx-synthetic-v1"}` for deterministic end-to-end
   validation; and
2. a `Shell` result classified by the existing test-runner strategy, only when the existing CTX
   controller authorizes it—for example, during an explicit `ctx context trial Shell --on` trial.

Other tools, mutation results, errors, multiple text leaves, unknown shapes, and non-identity
encodings remain whole. Chat protocol code remains unroutable on Cursor while M0's surface decision
is held.

## Atomic transaction

The transport-neutral transaction is:

1. verify the route mode, exact protocol-correlated result, contract, and existing controller gate;
2. compute a candidate with the existing strategy layer;
3. derive a deterministic route/protocol/tool/content rewind ID;
4. append the normal `ctx_expand` recovery marker;
5. durably store the exact original and marked replacement before returning prepared bytes;
6. replace only the exact JSON string leaf in the request;
7. forward the request to M1's fixed upstream; and
8. record `applied=true` only after a successful non-streaming response or the first complete SSE
   `data:` event.

Preparation is idempotent. Replaying the same route/protocol/tool/content produces the same rewind
ID and replacement. Acceptance is idempotent too, so retries cannot double-count a trim.

If persistence, strategy validation, JSON patching, transport, provider acceptance, or SSE event
validation fails, CTX does not record an applied trim. A recovery row prepared before a later
failure may remain, which favors recoverability and under-counting over a false savings claim.

## Cache-preserving patch

M3 does not parse and reserialize the whole request. For every candidate it:

- verifies the adapter's parsed path still contains the expected original string;
- finds one unique encoded occurrence of that string in the original request bytes;
- replaces only that JSON string literal;
- independently updates the parsed value at the exact path; and
- reparses the patched bytes and requires semantic equality with that expected value.

Duplicate encoded text, stale paths, overlapping spans, invalid JSON, or any unrelated semantic
change fails closed. Tests prove every byte before and after the replaced literal remains identical,
which protects stable prompt prefixes better than whole-body serialization.

## Evidence

Local tests prove:

- durable exact rewind exists before any request can be sent;
- a prepare/crash window contains no applied decision;
- deterministic prepare and acceptance retries do not duplicate evidence;
- unauthorized, already-shortened, empty, non-saving, ambiguous, and stale replacements make no
  recovery or applied claim;
- the synthetic contract patches Anthropic Messages, OpenAI Responses, and OpenAI Chat at their
  exact result leaf;
- an explicitly trialed Shell test result uses the shared test strategy on all three protocols;
- a fake upstream receives the shortened request and an HTTP 2xx records acceptance;
- HTTP rejection retains recovery but records zero applied trims;
- upstream connection failure records zero applied trims; and
- SSE keepalive bytes do not count as acceptance, while the first complete `data:` event does.

## Remaining evidence boundary

No live OpenAI or Anthropic request was sent while implementing this milestone. M3's code and mock
acceptance gates are complete, but the plan's live-upstream half remains open until an explicitly
authorized, paid smoke test proves provider acceptance for each supported auth/transport identity.
No existing client profile or route on this machine was switched to testing by this change.
