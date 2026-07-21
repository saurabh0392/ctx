# ADR 0048: Permit an explicit loopback model gateway without reviving TLS interception

- Status: accepted
- Date: 2026-07-21
- Parent: `docs/model-gateway-implementation-plan.md`
- Narrows: ADR 0015

## Context

ADR 0015 removed CTX's dormant certificate-authority MITM proxy. That decision was correct: a
system-trusted CA, generic TLS interception, ambient proxy variables, and an unrestricted second
request-editing path were disproportionate risks for an unused feature.

Hooks and the MCP gateway still cannot make built-in or directly connected tool results
model-visible on every coding client. Some clients expose a supported base-URL or custom-provider
setting that can send an application protocol to a user-chosen loopback endpoint. That is a
different trust boundary from silently intercepting traffic addressed to a provider.

## Decision

CTX may build an opt-in application-layer model gateway with all of these constraints:

- the user explicitly enables one named surface/auth/protocol/upstream route;
- the client is configured through a supported user setting or a clearly guided manual setting;
- CTX listens only on loopback and owns no public relay for model traffic;
- every listener has one fixed, displayed upstream; callers cannot choose an arbitrary target;
- CTX uses ordinary provider TLS validation when it connects to that upstream;
- no CTX CA, certificate installation, DNS rewriting, generic `CONNECT`, system-wide proxy, or
  arbitrary TLS interception is permitted;
- credentials are passed through or obtained through a separately reviewed native credential
  chain, never logged, hashed, exported, or persisted by the gateway;
- unknown protocols, encodings, versions, item types, or correlation states pass through
  unchanged; and
- enable, bypass, disable, uninstall, recovery, and destination receipts are route-scoped and
  reversible.

The model gateway must remain a separate mode from the existing MCP gateway. Standard CTX remains
hook-first and MCP-gateway-first. A platform logo never proves model-path coverage: activation is
bound to the exact surface version, authentication mode, protocol, transport, upstream class,
tool identity, result shape, and transform version that earned evidence.

M0 is more restrictive still. Its probe and capture sanitizer may read documented configuration
structure and sanitize an explicitly supplied offline envelope. It may not start a listener, edit a
client profile, read credential contents, forward traffic, or mutate a request.

## Security and product promise

"Local" describes where the CTX process executes, not where the user's prompt ultimately goes. A
configured coding client still sends the request through CTX to the displayed model service, and a
surface may involve its own backend. The UI must state the verified route and unavailable paths.

The permitted claim is:

> For an enabled and verified route, model requests pass through a CTX process on this device
> before CTX forwards them to the displayed destination. CTX operates no cloud relay for that
> traffic. Only eligible client-side tool results may change, and exact recovery data follows the
> user's local retention and purge controls.

CTX must not say prompts "stay local," that every request from a supported surface crosses CTX, or
that hosted/server-side tools can be trimmed.

## Consequences

ADR 0015's certificate and transparent-interception prohibition remains intact. Its broader
statement that CTX never edits a request on the wire is narrowed only for explicit application
routes satisfying this ADR.

The design accepts lower surface coverage in exchange for an inspectable boundary. If a client
does not expose a supported or explicit route, CTX reports that route as held or unavailable rather
than installing a CA or relying on an undocumented credential/configuration edit.
