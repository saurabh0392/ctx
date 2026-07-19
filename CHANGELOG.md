# Changelog

All notable CTX changes are recorded here. Versions follow semantic versioning while the product is
in beta.

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
