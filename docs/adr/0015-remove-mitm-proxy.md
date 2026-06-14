# 0015. Remove the MITM proxy subsystem

- Status: accepted
- Date: 2026-06-13
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-29
- Supersedes: the proxy parts of ADR 0011 (reasoning capture, already reverted) and retires the proxy as a shipping component.

## Context

ctx grew up with an optional MITM proxy. It terminated TLS to `api.anthropic.com` with a
local CA, and could filter MCP tool schemas out of the request body or run the full gate
pipeline in flight. Over time the product moved hook-first:

- The proxy has been off by default for a while. In `config.rs`, `ProxyMode::Off` is the default,
  documented "Hooks + soft filter only; proxy not wired."
- ADR 0011 built reasoning capture on the proxy stream, measured it on a real session, found
  Anthropic encrypts extended thinking end to end, and reverted it. That removed the proxy's last
  differentiated capability.
- MCP filtering in the shipping default (soft mode) is enforced by writing Claude Code
  `permissions.deny` rules from the `UserPromptSubmit` hook (`filter_control::hook_sync_profile`),
  not by the proxy. Injection, coaching, budget guard, compression, and ingest are all hook-side
  or transcript-side.

So the proxy is dead code: nothing in the default path uses it, and its one unique value was
already disproven. Meanwhile it carries real cost. It ships a system-level MITM CA, which is the
heaviest trust ask in the product; it keeps a second request-editing path alive that has to be
reasoned about (it muddied the CTX-28 cache analysis by implying ctx edits the cached prefix in
flight when, by default, it does not); and it pulls in a TLS and certificate dependency stack.

## Decision

Remove the MITM proxy subsystem entirely.

- Delete `src/proxy.rs`, `src/filter.rs` (the proxy-only tools-array editor), `src/ca.rs` (the
  MITM TLS CA), and `tests/proxy_test.rs`.
- Remove `ProxyMode` and the `proxy_mode`, `proxy_port`, `proxy_upstream`, and `original_base_url`
  config fields, plus `migrate_proxy_mode`.
- Remove the `ctx proxy` CLI command and its dispatch, and `ensure_tls_crypto_provider`.
- Remove the proxy daemon functions and CA path references from `daemon.rs`, and the
  `com.ctx.proxy` launch agent.
- Remove proxy env wiring (`CLAUDE_CODE_HTTPS_PROXY`, `NODE_EXTRA_CA_CERTS`, the proxy
  `ANTHROPIC_BASE_URL`) from `claude_settings.rs` and the `proxy::uninstall()` call from setup
  teardown.
- Remove the proxy controls from the dashboard.
- Drop the now-unused TLS dependencies (rustls, tokio-rustls, rcgen, related hyper/reqwest TLS
  features) from `Cargo.toml`.
- Update `ARCHITECTURE.md`, `README.md`, and `INSTALL_PROMPT.md` to describe ctx as hook-first with
  no proxy.

ctx becomes purely hook-first and transcript-first: it influences agents through Claude Code
hooks and settings, and it learns from transcripts on disk. It never terminates TLS and never
edits the request body on the wire.

## What this deliberately keeps

- Soft-mode MCP filtering via Claude Code `permissions.deny`, and the `ctx filter` control that
  drives it. This is the real shipping filtering path and has nothing to do with the proxy.
- All hook-side behavior: profile selection, system-prefix injection, coaching, budget guard,
  read edit-intent guard, and tool-output compression.
- Ingest, the local model, the causal gate, the dashboard, and the A/B framework.

## Alternatives considered

- **Keep the proxy as an opt-in.** Rejected. It is unused, its unique capability was disproven, and
  a dormant MITM CA plus a second editing path is a trust and maintenance liability that earns
  nothing. Dead code that touches TLS and the request body is exactly the code to delete.
- **Hard-disable and mark deprecated, keep the code.** Rejected as a half measure. It leaves the CA
  machinery, the dependency stack, and the cognitive load in place. If we ever need on-the-wire
  editing again, the git history and this ADR are the record to revive it from.
- **Remove code but leave the TLS dependencies.** Rejected for the full change; the deps exist only
  for the proxy CA, so they go with it. Smaller diff was not worth leaving the stack behind.

## Consequences

- The trust story gets simpler and stronger: ctx never terminates TLS, never installs a CA, and
  never edits requests in flight. That is easier to explain to a skeptical buyer and easier to
  verify.
- The cache story gets simpler: by construction, ctx does not edit the cached request prefix in
  flight. The only prefix change ctx causes is the system-prefix injection it asks Claude Code to
  add through the hook, which is measurable via the A/B (see CTX-28).
- Less code, fewer dependencies, faster builds, and the flaky `ca::tests` cert test goes away.
- We lose the ability to filter or rewrite requests on the wire. That is acceptable: it was unused,
  and soft-mode deny-rule filtering covers the shipping need. Reviving it means reintroducing a CA,
  which should be its own deliberate decision.
- Existing user `config.toml` files may carry stale `proxy_*` keys. serde ignores unknown fields,
  so they keep loading; the keys become inert.
