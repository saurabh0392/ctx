# Changelog

All notable CTX changes are recorded here. Versions follow semantic versioning while the product is
in beta.

## [Unreleased]

## [0.7.0] - 2026-07-21

### Added

- Add an opt-in, provider-neutral model gateway with fixed OpenAI and Anthropic destinations,
  loopback-only listeners, bounded HTTP/SSE relay behavior, and byte-identical shadow mode.
- Add isolated OpenAI Responses, OpenAI Chat Completions, and Anthropic Messages adapters with
  fail-closed tool-call/result correlation and exact JSON-leaf replacement.
- Add evidence-gated testing routes that durably retain the exact original before sending a
  proposed trim and count it as applied only after HTTP or SSE provider acceptance.
- Add reversible Codex and Claude Code configuration transactions, nonce-bound listener health,
  launchd/systemd service contracts, route doctor, immediate bypass, disable, and uninstall.
- Add route-scoped dashboard evidence and `ctx model-gateway readiness [--json]`, including exact
  client/version/auth/protocol identity, recovery integrity, local-processing p95, unsupported
  routes, and explicit external commercial-release blockers.

### Changed

- Separate time spent inside CTX from total provider-acceptance latency and isolate percentiles and
  pass-through reasons by the complete route identity.
- Distinguish attempted, accepted, applied, held-whole, already-shortened, unknown, rejected,
  transport-failed, and bypassed model-route outcomes without persisting request content.
- Treat native post-tool compaction receipts as completed while keeping transcript and Cursor
  pre-compaction signals attempt-only.

### Security and privacy

- Keep model routing explicitly configured and local: no CTX certificate authority, DNS rewriting,
  ambient proxying, generic CONNECT support, arbitrary upstream, or CTX-operated cloud relay.
- Enforce a 500 ms fail-open transform deadline that sends the exact original and cooperatively
  cancels late preparation; abandoned candidates are never counted as applied.
- Persist neither model requests nor credentials, keep exact originals under existing local
  retention/purge controls, and report prepared-but-unapplied recovery copies separately.
- Keep Cursor model routing, Codex ChatGPT login, Codex WebSocket, provider-hosted tools, and every
  commercial-readiness claim unavailable until their independent evidence gates are complete.

## [0.6.2] - 2026-07-20

### Fixed

- Derive every model-visible savings figure from the exact emitted result instead of the earlier
  shadow proposal, and backfill existing receipts from the local recovery store.
- Make `ctx_status` combine emitted output savings with input-menu savings rather than reporting
  output savings as zero, and remove duplicate platform limitations from capability receipts.
- Label transcript-derived dollar figures as API-equivalent estimates, separately from manually
  entered actual account spend.
- Keep Codex app operations distinct in dashboard labels so eligible reads and held mutations cannot
  collapse into one apparent tool.

## [0.6.1] - 2026-07-19

### Added

- Cross-platform compaction receipts that distinguish attempted, confirmed, inferred, mixed, and
  unknown states: Claude Code and Codex use native pre/post hooks, while Cursor remains honestly
  attempt-only until its public hook contract exposes completion.
- Retry-stable compaction event de-duplication plus separate native and historical Claude counts
  without storing prompts, transcripts, summaries, commands, output, or paths.
- Versioned per-path capability receipts for shell, built-in, MCP, and compaction behavior on every
  supported agent surface, including exact evidence counts and platform/setup limitations.
- Product-proof metrics for model-visible savings, recovery use, corrections and re-touches,
  gateway coverage, latency, failures, and exact pass-through reasons.
- Configurable original retention, immediate purge, owner-only storage receipts, exact outbound
  destination receipts, and an isolated byte-for-byte recovery self-test in the dashboard and MCP.

### Changed

- Keep historical Claude transcript compactions visible after native hooks are installed, while
  excluding sessions already represented by native events so the combined total is not double-counted.
- Replace broad agent status labels with evidence-backed capability receipts and show every known
  agent even before it has produced local activity.
- Qualify the network promise as no background telemetry: explicit exports, reports, check-ins, and
  approved remote MCP destinations remain visible user-controlled egress paths.

### Fixed

- Make full uninstall with `--purge-data --yes` remove CTX-owned state only after validating its
  ownership marker, while preserving unrelated agent configuration.
- Keep compaction hooks fail-open even when stdin cannot be read, and surface retention/purge API
  failures instead of falsely reporting success.

### Security and privacy

- Protect the CTX state directory as owner-only and its config/database/marker files as private on
  Unix, while reporting the effective protection in Settings.
- Bound retained originals by both entry count and bytes, fail closed before applying a trim whose
  original cannot remain recoverable, and require exact confirmation before permanent purge.

## [0.6.0] - 2026-07-19

### Added

- One two-phase MCP apply transaction: exact recovery is durable before emission, while applied
  evidence is committed only after the adapter flushes the shortened result.
- A local `stdio` MCP gateway with explicit server registration, isolated direct process spawning,
  bounded JSON-RPC correlation, `tools/list` contract caching, and unknown-message pass-through.
- Opt-in Streamable HTTP transport with exact destination receipts, DNS pinning, redirect and proxy
  refusal, private-address blocking, MCP sessions, SSE parsing, and event-id redelivery suppression.
- OAuth 2.1 authorization-code support with PKCE S256, dynamic client registration, state checking,
  refresh rotation, and operating-system credential storage without a plaintext fallback.
- Reversible `ctx gateway codex-enable` / `codex-disable` commands that preserve Codex policy fields
  and restore the exact original MCP server table.
- POSIX, PowerShell, `cmd.exe`, and WSL shell execution contracts plus macOS CI coverage.

### Changed

- Migrate Claude Code and Cursor MCP output replacement to the canonical block-aware strategy and
  apply boundary; schema-dependent strategies activate through the gateway's captured contracts.
- Preserve shell stdout, stderr, exit/signal status, invalid encodings, ANSI streams, and terminal
  behavior separately; interactive and ambiguous commands bypass capture.
- Report Codex MCP trimming as available only for explicitly approved gateway servers. Built-in
  Read/search remains honestly observation-only because current Codex hooks cannot replace results.
- Replace the absolute local-only promise with a precise one: CTX operates no traffic relay, local
  evidence stays local, and remote MCP traffic goes only to the reviewed destination it already
  requires.

### Security and privacy

- Store no OAuth token in SQLite, logs, analytics, or CTX config; fail closed if the OS credential
  store is unavailable.
- Reject shell spawning, literal imported secret environments, URL credentials, redirects,
  ambient HTTP proxies, DNS rebinding to non-public addresses, and unapproved remote destinations.
- Prove byte-identical gateway pass-through with trimming disabled and exact recovery plus non-text
  block preservation with an authorized model-visible trim.

## [0.5.6] - 2026-07-19

### Fixed

- Read and rewrite Codex's live unified shell input field (`cmd`) as well as the legacy
  Claude-style field (`command`), preserving sibling tool input fields.
- Let the trusted Codex pre-tool hook route eligible safe shell commands through the existing
  evidence-gated CTX wrapper instead of remaining permanently observation-only.

### Security and privacy

- Keep the conservative read-only command allowlist, surface-isolated evidence gate, exact command
  exit status, fail-open passthrough, and local-only processing unchanged.

## [0.5.5] - 2026-07-19

### Changed

- Run the model-based Fitcheck locally on the exact PR head instead of spending GitHub Actions
  credits in PR and release workflows.
- Publish a required `Local Fitcheck` commit status with `make pr-fitcheck PR=<number>` so GitHub can
  enforce the local result without running the model.

### Security and privacy

- Restrict local Fitcheck to read-only file tools with MCP, browser, shell, and edit access disabled.
- Fail closed on setup, authentication, output parsing, worktree mutation, or a changed PR head.

## [0.5.4] - 2026-07-18

### Added

- Lossless canonical MCP result and tool-contract capture behind the shadow-only evidence gate.
- Independently validated proposal strategies for plain text blocks, schema-backed paginated
  collections, ranked search results, entity/detail records, rooted flat tree listings, and
  rectangular scalar tables.
- Sanitized contract fixtures and adversarial tests for stale, forged, schema-hostile, oversized,
  ambiguous, and content-leaking proposals.

### Changed

- Make every structured proposal prove source round-trip identity, advertised output-schema
  validity, deterministic largest-fitting selection, and exact preservation of protected fields.
- Tighten raw delimited-table detection so weak two-line comma or tab patterns continue through the
  strategy registry instead of being misclassified.
- Make dashboard coherence mutation checks deterministic and remove a stale Stop trial control.

### Security and privacy

- Keep proposal edits, retained indices, structured candidates, text projections, tool input, and
  result content inside the transient validator boundary; this release remains shadow-only.
- Emit only bounded content-free strategy, schema, character-count, and item-count evidence.
- Fail closed on unsupported schemas and raw table dialects rather than guessing semantics.

## [0.5.3] - 2026-07-18

### Added

- Installable CTX Codex plugin with local lifecycle hooks, native compaction observation, and the
  existing local CTX recovery/status MCP server.
- Codex-specific shell wrapping behind the surface-isolated evidence gate.
- Codex capability and activity cards in the dashboard, including an honest not-seen-yet state.
- Codex plugin, heartbeat, and activation diagnostics in `ctx doctor`.

### Changed

- Isolate permission to trim by agent surface, normalized tool, and transform version so evidence
  from Claude Code or Cursor cannot activate Codex.
- Describe Codex as partially active: eligible shell output is controllable; hosted tools and
  unsupported built-in result paths remain observation-only.

### Security and privacy

- Codex hooks require explicit review and trust in Codex before they execute.
- The plugin uses the installed local CTX binary and does not add a CTX-hosted data path.

## [0.5.2] - 2026-07-18

### Fixed

- Remove the rollback binary after a successful authenticated update and refresh setup explicitly
  as a beta install.

## [0.5.1] - 2026-07-18

### Fixed

- Start beta active-day and check-in timing at enrollment instead of counting historical Claude
  sessions ingested during fresh setup.

## [0.5.0] - 2026-07-17

### Added

- Fresh-install `ctx setup --beta --yes` with full, evidence-gated output autopilot and MCP filtering off.
- `ctx doctor [--json]` diagnostics and capability-authenticated `ctx update [--check]` for macOS/Linux.
- Revocable 90-day `download`/`feedback` capabilities; one-time invite tokens are not stored locally.
- Local allowlisted product events, first-run onboarding, and explicit-preview beta check-ins.
- Aggregate-by-default Context Reports plus `ctx.context-report.v1` JSON export.
- Private screenshot intake, seven-day issue links, 30-day screenshot retention, and one-year aggregate check-in retention.
- Claim-consistency, formatting, Clippy, version/tag, changelog, coherence, and fitcheck release gates.

### Changed

- Product language now distinguishes transform eligibility from evidence-gated activation.
- Safety language describes observed re-read/re-edit evidence and no longer presents it as causal proof.
- v0.5 supports macOS + Claude Code as the primary beta path; Cursor and Windows are experimental.

### Security and privacy

- Feedback intake now rejects missing, expired, wrong-scope, or roster-revoked capabilities.
- Browser JavaScript never receives the stored beta capability; localhost proxies authenticated sends.
- Aggregate snapshots exclude prompts, output, commands, paths, repos, tool/MCP names, source, costs, and arbitrary JSON.
- Screenshots are private S3 objects rather than public-read assets.

## [0.4.1] - 2026-07-07

- Prior alpha release.
