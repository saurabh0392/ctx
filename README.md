# CTX

CTX shows where a coding agent's context goes, then reclaims noisy tool output without losing the original.

It runs locally beside coding agents, records a Context Bill from real tool results, and applies reversible output trims only after comparable randomized runs pass a plain-language safety check.

CTX v0.5 is a token-gated beta for small engineering teams. macOS with Claude Code is the supported path for this wave; Cursor and Windows are experimental.

## Beta install

Ask for a beta invite, then run the command provided with it:

```bash
curl -fsSL <distribution-endpoint>/install.sh | CTX_TOKEN=<one-time-invite> sh
```

The installer downloads a SHA-256-verified binary, stores a scoped and revocable 90-day beta capability, runs `ctx setup --beta --yes`, wires Claude Code hooks and recovery tools, installs the CTX plugin when Codex is present, and starts the dashboard at [http://127.0.0.1:8789](http://127.0.0.1:8789). Codex requires one explicit hook review: open `/hooks` in Codex and trust the CTX hooks.

The macOS beta is not yet Developer ID signed or notarized. The installer clears quarantine and adds an ad-hoc signature as a temporary beta bridge. Do not treat this release as production-grade software distribution.

After installation:

```bash
ctx doctor             # local install, DB, hooks, MCP, dashboard, capability
ctx doctor --json      # stable machine-readable diagnostics
ctx update --check     # check the token-gated beta channel
ctx update             # checksum-verify, atomically replace, and refresh setup
```

## The first-run contract

A fresh `--beta` install uses these defaults:

- Output autopilot: `full`, with bounded burn-in and the existing evidence gate.
- Reversibility: every applied trim keeps the verbatim original for `ctx expand` / `ctx_expand`.
- Read protection: editable project files stay whole under the edit-intent guard.
- Force activation: off. No tool bypasses the evidence gate.
- MCP filtering and server pruning: off. CTX does not hide capabilities by default.
- Background telemetry: none.

Existing installs keep their configuration when enrolled or upgraded. Normal `ctx setup` behavior is unchanged.

Pause or resume output control at any time:

```bash
ctx context off        # stop changing output; observation continues
ctx context preset full
ctx context status
```

## How the evidence gate works

For each supported tool, CTX records what its current transform would retain. Bounded testing randomly leaves some eligible runs unchanged and trims others. After at least 20 scored runs on each side, the tool earns ongoing activation only when the upper end of the 95% interval rules out more than 10 extra corrections or re-touches per 100 runs. If the result is still uncertain, testing continues and normal output stays unchanged.

Permission is isolated by agent surface, normalized tool, and transform version. Claude or Cursor evidence cannot authorize a Codex trim. The result supports a narrow claim about this transform on this machine; it is not a universal model evaluation.

The underlying tool always runs in full. CTX shortens only the result returned to the agent. Recover the original with:

```bash
ctx expand <rewind-id>
```

or let the agent call the registered `ctx_expand` MCP tool.

## Context Reports

Export a self-contained report for one repo:

```bash
ctx report --list
ctx report --repo my-project
ctx report --repo my-project --format json
ctx report --repo my-project --privacy detailed
```

Aggregate privacy is the default. It omits commands, paths, absolute repo paths, and tool/server names. Detailed mode retains the visible drill-down and prints a review-before-sharing warning. JSON exports use `ctx.context-report.v1`.

An “eligible” token in a report means CTX's current transform can remove it. Eligibility and evidence-gated activation are reported separately.

## Privacy and beta evidence

Prompts, tool output, commands, paths, repo names, and source code stay local. CTX has no background telemetry.

At 7 and 21 active days, a beta install may offer an optional check-in. The dashboard shows the exact `ctx.beta-checkin.v1` JSON before Send becomes available. The allowlist contains only installation/version metadata, activity counts, Context Bill totals, trim/recovery counts, tool-stage counts, and four short product questions. Dismissing it waits seven days. A failed send is preserved locally for retry.

Issue reports follow the same preview-and-send rule. The localhost dashboard adds the scoped capability server-side, so browser JavaScript never receives it. Screenshots are optional, limited to three files of 5 MB, stored privately, linked for seven days, and deleted after 30 days.

## Compatibility

| Surface | Beta status | Observation | Reversible output control | Tool-menu pruning |
| --- | --- | --- | --- | --- |
| Claude Code on macOS | Supported | Yes | Yes | Opt-in |
| Claude Code on Linux x86_64 | Best effort | Yes | Yes | Opt-in |
| Cursor | Experimental | Yes | Partial, hook-dependent | Experimental |
| Codex | Experimental | Live plugin hooks | Partial: eligible shell output | No |
| Windows x86_64 | Experimental | Yes | Hook-dependent | Experimental |
| Claude Desktop | Insight only | Session ingest | No | No |

Claude Desktop does not load Claude Code's hook path. CTX can register its MCP server and ingest supported local session logs, but it cannot apply `updatedToolOutput` trims there.

## Useful commands

```bash
ctx dashboard
ctx ingest
ctx context status
ctx context proof
ctx context labels --limit 20
ctx context off
ctx expand <rewind-id>
ctx report --list
ctx doctor
ctx update --check
ctx setup --uninstall
```

MCP tool-menu filtering remains an explicit advanced control:

```bash
ctx filter mode soft
ctx filter mode strict
ctx filter mode off
ctx filter expand <server-or-tool>
ctx filter clear-expansion
```

`soft` uses Claude Code permission denies while keeping servers connected. `strict` uses an allowlist and can disconnect non-listed servers. `off` is the shipped default.

## Build from source

Requires a current Rust toolchain:

```bash
cargo build --release --locked
./target/release/ctx setup --yes
```

Optional ONNX support enables the local MiniLM embedder:

```bash
cargo build --release --locked --features onnx
```

The default build uses the lightweight local fallback and downloads no model.

## Development checks

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked

cd services/ctx-dist && npx tsc --noEmit && npm run synth
cd services/report-intake && npx tsc --noEmit && npm run synth
```

Release tags, `Cargo.toml`, and the binary version must agree. v0.5 release CI treats formatting, Clippy, all-target tests, coherence checks, and fitcheck as blocking.

Architecture and decision history:

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [Evidence-driven pivot](docs/honest-pivot-writeup.md)
- [Strategy](docs/strategy-context-truth-layer.md)
- [ADRs](docs/adr)

## Beta services

- [`services/ctx-dist`](services/ctx-dist): private artifacts, invite-to-capability exchange, revocation, and presigned downloads.
- [`services/report-intake`](services/report-intake): capability-authenticated private issue reports and aggregate check-ins.

Operator data is not committed. Cohort labels remain in the encrypted SSM roster; summaries use pseudonymous participant IDs and aggregate counts.

## Uninstall

```bash
ctx setup --uninstall
```

This removes CTX-managed services, hooks, MCP registrations, the CTX-owned Codex plugin/marketplace, filter rules, and the stored beta capability. Unrelated Codex configuration is preserved. Indexed local data remains under `~/.ctx` so uninstall is not a destructive data wipe.
