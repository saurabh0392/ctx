# ADR 0046: Atomic apply and explicit MCP gateway

Status: accepted
Date: 2026-07-19
Parent: `docs/tool-trimming-architecture-revamp.md`
Builds on: ADR 0038 through ADR 0045

## Context

T1 and T2 could parse MCP results losslessly and validate contentful proposals, but deliberately
discarded every candidate. Native hooks also had divergent output reconstruction and telemetry
paths. Codex's current PostToolUse hook can observe a result but cannot return a clean replacement,
so a plugin-only implementation could never make MCP trimming model-visible there.

Remote MCP support also changes the privacy statement. CTX can remain local software without
claiming that a remote tool's network traffic stays on-device. The relevant boundary is whether CTX
adds an operator relay or an unreviewed destination; it must not.

## Decision

Introduce a single two-phase MCP apply boundary. Phase one independently revalidates the proposal,
renders the canonical candidate, requires net savings, and commits the exact serialized original to
the bounded rewind store. Any failure returns the exact original. The adapter then emits and flushes
the prepared result. Only after that flush may phase two write an applied decision and savings event.
Under-counting after a receipt failure is preferable to a false applied claim.

Claude Code and Cursor MCP hooks use this boundary. Schema-dependent structured transformations are
available only when an adapter has a captured `tools/list` contract. Codex's plugin remains the
observer/control plane for built-ins; explicitly selected MCP servers may instead run through a
local CTX gateway, which is the model-visible transport boundary Codex's result hook does not offer.

The stdio gateway stores immutable, user-approved server definitions and spawns absolute
executables directly without a shell. It clears the child environment, restores only baseline and
named variables, bounds frames and in-flight requests, correlates arbitrary JSON-RPC IDs, captures
contracts, transforms only successful `tools/call` results, and passes every other message through.
Codex rewiring backs up the exact server table, changes only transport keys, preserves policy keys,
and restores the original table on disable.

Remote Streamable HTTP is a separate opt-in beta transport. CTX records an exact destination
receipt, disables ambient proxies and redirects, resolves and pins DNS, blocks non-public targets
except explicit loopback HTTP, preserves MCP sessions, and bounds JSON/SSE responses. OAuth uses
authorization code plus S256 PKCE and state validation. Tokens and refresh material exist only in
memory and the OS credential store; there is no plaintext fallback.

Shell wrapping keeps stdout and stderr separate and records an apply only after stdout flush.
Nonzero exits, signals, invalid UTF-8, ANSI output, interactive commands, background commands, and
ambiguous shell compositions remain exact pass-through. POSIX, PowerShell, cmd.exe, and WSL have
explicit launch contracts. Built-in Read/search activation remains limited to native contracts
that have been captured and proven model-visible.

## Consequences

Codex can now shorten MCP results, but only for servers the user explicitly routes through the
gateway. Direct Codex MCP servers and built-in Read/search remain observed only. The dashboard and
doctor report that distinction rather than presenting a platform-wide capability.

CTX still operates no traffic relay and does not terminate model-provider TLS. Local stdio traffic
never leaves the machine. A remote MCP request goes directly to its approved third-party endpoint,
which is an existing tool egress path rather than CTX-owned telemetry. This is a narrower and more
accurate promise than “everything stays local and secure.”

Remote transport remains beta until real-server coverage, latency/failure thresholds, signed
artifacts, dependency audit/SBOM, and independent security review satisfy T7/T8. Unsupported
protocol shapes and any validation, recovery, credential, or destination failure pass through or
fail closed according to whether the original transport can still be reached safely.
