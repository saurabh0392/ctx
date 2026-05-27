# ctx

ctx strips the MCP tool definitions Claude Code doesn't need for the current task, tracks what each session actually costs, and serves a local dashboard. Install with no Rust required — just `gh` authenticated to the goshippo org.

```bash
gh repo clone goshippo/ctx ~/Documents/ctx 2>/dev/null || git -C ~/Documents/ctx pull
bash ~/Documents/ctx/scripts/install.sh
ctx setup
```

After setup, run `ctx profile generate` once to build profiles tailored to your MCP stack, then `ctx use <profile>` to activate one.

## Compatibility

| Feature | Claude Code in an IDE | Terminal CLI | Claude Desktop |
| --- | --- | --- | --- |
| Native MCP allowlist (`allowedMcpServers` in `~/.claude/settings.json`) | Yes | Yes | No (Desktop uses its own config) |
| Claude Code hooks (`UserPromptSubmit`, `PostToolUse`, …) | Yes | Yes | No |
| Legacy in-process filter (`NODE_OPTIONS` + `filter.js`) | Deprecated (ignored by Bun-based `claude` binary) | Deprecated | No |
| Per-request tracing (dashboard Request Trace tab) | Yes (native hooks, no proxy needed) | Yes | No |
| Optional HTTPS MITM proxy (`ctx proxy`) | Yes | Yes | No |
| MCP tools (`ctx_spend`, `ctx_sessions`, …) | Yes | Yes | Yes (after MCP config + app restart) |
| Dashboard | Yes | Yes | Yes |
| Session ingest + analytics (`ctx ingest`) | Yes | Yes | Yes (Desktop `audit.jsonl` under local-agent sessions) |
| Real-time ingest (per turn) | Yes — `filter.js` or hooks hit `POST /api/trigger-ingest` / hook events | Yes | No (run `ctx ingest` manually) |
| Periodic background ingest | Yes on macOS and Linux via background services | No (run `ctx ingest` or cron) | Yes on macOS and Linux when periodic ingest is installed |
| Reload | Command Palette → Reload Window | New shell session | Quit Desktop fully, reopen |

“Claude Code in an IDE” includes Cursor, VS Code, Windsurf, or any editor where Claude Code runs in an integrated terminal. `ctx setup` picks Windsurf or Cursor MCP paths when those environments are detected.

### Claude Desktop: no Claude Code hooks path

Desktop is an Electron app. It does not load `~/.claude/settings.json` the way the Claude Code CLI does for MCP allowlists and hooks. Per-request tracing in the dashboard applies in **Claude Code** (CLI or IDE) via native hooks and SQLite, not in standalone Desktop chat. On Desktop, use MCP plus `ctx ingest` for session-level data when local-agent `audit.jsonl` logs exist.

## Install journey

`ctx setup` is the single entry point after you build or install the `ctx` binary. It detects your environment ([`host.rs`](src/host.rs)), writes files under `CTX_HOME` (default `~/.ctx`), starts background services where supported, merges MCP config, and merges **`allowedMcpServers` plus Claude Code `hooks`** into `~/.claude/settings.json` when Claude Code settings apply. Legacy `NODE_OPTIONS --require filter.js` is stripped when present; the shipped `claude` binary is Bun-based and does not honor Node preload.

```mermaid
flowchart TD
  Start["ctx setup"] --> Detect["Detect host host.rs"]
  Detect --> IsIDE{IDE detected?}
  IsIDE -->|Cursor VS Code Windsurf| IDE_Path["allowedMcpServers + hooks in settings.json\nMerge MCP into settings.json and IDE mcp.json\nInstall proxy dashboard and ingest services"]
  IsIDE -->|No| IsCLI{Claude Code CLI present?}
  IsCLI -->|Yes| CLI_Path["allowedMcpServers + hooks in settings.json\nMerge MCP into settings.json\nInstall proxy dashboard and ingest when host requests it"]
  IsCLI -->|No| Desktop_Path["Skip Claude Code hooks path\nMerge MCP into desktop config\nInstall dashboard and ingest services"]
  IDE_Path --> Done["Reload IDE window"]
  CLI_Path --> Done2["Open new terminal session"]
  Desktop_Path --> Done3["Quit and reopen Desktop"]
```

Detection order in code: Windsurf markers, then Cursor, then VS Code integrated terminal, then **Desktop-only** if the Claude Desktop data directory exists **and** the Claude Code CLI is absent, otherwise plain terminal. If Desktop is installed alongside Claude Code, the primary host is IDE or terminal, but `ctx setup` still merges the `ctx` MCP entry into `claude_desktop_config.json` when that file exists.

**What each surface gets** is summarized in the [Compatibility](#compatibility) table above (no duplicate matrix here).

Quick paths:

- **Claude Code in an IDE (recommended):** Run these in a terminal, then reload the IDE window.
  ```bash
  gh repo clone goshippo/ctx ~/Documents/ctx 2>/dev/null || git -C ~/Documents/ctx pull
  bash ~/Documents/ctx/scripts/install.sh
  ctx setup
  ctx profile generate   # build profiles from your actual MCP stack
  ctx use <profile>
  ```
- **Claude Code in a terminal only:** Same install, then reload Claude Code so `~/.claude/settings.json` hooks and allowlist apply.
- **Claude Desktop:** Same install from an OS terminal, run `ctx setup`, then quit Desktop fully and reopen. Native Claude Code filtering is not available on Desktop (see table), but MCP tools and the dashboard work.
- **Build from source:** `gh repo clone goshippo/ctx ~/Documents/ctx` then `source "$HOME/.cargo/env" && cargo install --locked --path ~/Documents/ctx` (or follow [`INSTALL_PROMPT.md`](INSTALL_PROMPT.md)).

### What happens during `ctx setup`

1. Ensure `CTX_HOME`, generate local CA material for the proxy if needed, write `filter.js` from the embedded asset.
2. Install and start the proxy (default port `8788`), wait until it listens.
3. Create default `system_prefix.md` if missing; optionally download ONNX embedding weights when built with the `onnx` feature.
4. Open or create `ctx.db`, run an initial ingest when Claude Code project JSONL exists, pick a default profile, sync `filter-config.json`.
5. Install and start the dashboard (default port `8789`).
6. When `needs_periodic_ingest` is true, install a periodic `ctx ingest` job (macOS and Linux user services).
7. Unless `--no-install`, run `proxy::install` to merge **`allowedMcpServers`**, **hooks** (`ctx hook user-prompt-submit`, async `POST /api/hook/event`), and strip legacy `NODE_OPTIONS` filter preload when `supports_node_options` is true (not Desktop-only).
8. Register `ctx mcp` in Claude settings, IDE-specific MCP JSON when applicable, and Desktop config when present.
9. Open the dashboard URL in a browser when ready.

Other entry points: paste the prompt from [`INSTALL_PROMPT.md`](INSTALL_PROMPT.md) for a guided Claude Code install, or run [`scripts/install-desktop.sh`](scripts/install-desktop.sh) from an OS terminal for a Desktop-first flow.

### Post-install checks

- `curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8789/` should print `200` after services are up.
- `ctx status` should print the active profile and related info.
- In Claude Code or Desktop, reload or restart so MCP lists include `ctx_*` tools.

Contributor-level diagrams, module tables, and pipeline detail: [ARCHITECTURE.md](ARCHITECTURE.md).

## Teardown

- `ctx setup --uninstall` removes background services where supported, strips ctx from MCP JSON files (Claude settings, Cursor, Windsurf, Desktop), removes ctx native hooks and `allowedMcpServers` keys written by ctx, and runs `ctx proxy uninstall` for env cleanup.
- Reload the IDE window or restart Desktop so removed env and MCP entries apply.

---

## Install

**Pre-built binary (no Rust required) — requires `gh` authenticated to the goshippo org:**

```bash
gh repo clone goshippo/ctx ~/Documents/ctx 2>/dev/null || git -C ~/Documents/ctx pull
bash ~/Documents/ctx/scripts/install.sh
ctx setup
ctx profile generate
```

`install.sh` detects your platform (macOS arm64/x86_64 or Linux x86_64), downloads the matching binary from the [latest release](https://github.com/goshippo/ctx/releases/latest) via `gh release download`, and installs it to `/usr/local/bin`. Set `CTX_INSTALL_DIR` to override the destination.

No `gh` but have a PAT? `GITHUB_TOKEN=<pat> bash scripts/install.sh` works too.

**Build from source (requires Rust):**

```bash
gh repo clone goshippo/ctx ~/Documents/ctx
source "$HOME/.cargo/env" && cargo install --locked --path ~/Documents/ctx
ctx setup
```

After installing the binary, run setup once:

```bash
ctx setup
```

`setup` writes assets under `CTX_HOME` (default `~/.ctx`): `filter.js`, `filter-config.json` (legacy), optional CA material for the proxy, merges **`allowedMcpServers` and hooks** into `~/.claude/settings.json` where configured, and installs background services on macOS (launchd) and Linux (systemd user units). On other OS targets it starts `ctx proxy` / `ctx dashboard` as detached processes and prints how to schedule ingest yourself.

## Filtering paths

1. **Default (v2 — native Claude Code)**  
   `allowedMcpServers` in `~/.claude/settings.json` plus hooks. MCP servers outside the allowlist never attach tool schemas to the API request. `ctx hook user-prompt-submit` handles auto-profile, budget hard-stop, optional JSONL-based coaching (correction cascades and re-asks from `~/.claude/projects/**/*.jsonl`), and `additionalContext` injection from `~/.ctx/system_prefix.md`. Async hooks POST telemetry to `http://127.0.0.1:8789/api/hook/event`. The Request Trace tab shows full per-turn pipeline cards for hook rows (profile, inject, coach, savings) and enriches them with model, tokens, and cost after JSONL ingest on turn end.

2. **Legacy (`NODE_OPTIONS` + `filter.js`)**  
   Deprecated: the `claude` CLI is a Bun binary and ignores Node preload. Files remain under `CTX_HOME` for experiments only.

3. **HTTPS proxy**  
   Optional MITM path for the Anthropic API host. Same gate logic exists in Rust (`proxy::run_gates`) for parity tests and for teams who route traffic through the proxy.

Keep feature work aligned with the native path so dashboards stay populated for everyone who uses Claude Code with hooks.

## Profiles

Profiles live in the Rust side (`profiles` module), sync to **`allowedMcpServers`** in `~/.claude/settings.json`, and still export `filter-config.json` for legacy setups. Each profile lists MCP server prefixes (or display names) to **keep**; other servers are blocked by Claude Code before tool schemas enter the request.

Generate profiles from your actual MCP stack (no usage history required):

```bash
ctx profile generate
```

This inspects your configured MCP servers, groups them by category (data, design, comms, work, files, finance, infra), and writes named profiles to `~/.ctx/profiles.toml`. Each profile is named after its primary category and includes communication tools alongside it so common workflows stay intact. Run it once after `ctx setup`, then re-run whenever you add or remove MCP servers.

Switch the active profile:

```bash
ctx use <profile>
```

List available profiles and their per-request token cost estimates:

```bash
ctx profile list
```

Tighter profiles remove more tool schemas, which saves more tokens on each request.

## Dashboard

```bash
ctx dashboard
```

Serves HTML on localhost, reads `~/.ctx/analytics.jsonl`, spend snapshots from local Claude exports where present, and shows:

- Savings totals (cache-read dollars plus a worst-case Sonnet-input estimate)
- Per-folder aggregates (`working_directory` from the system prompt)
- MCP **tools sent** vs **tools used** (used counts need non-stream responses the hook parses)
- Prompt stats, budgets, pipeline gate cards
- SQLite-backed intelligence when `~/.ctx/ctx.db` has data: quality alerts, project health by week, similar sessions via 384-d embeddings
- Hook trace rows: per `UserPromptSubmit`, profile-derived tool savings flags; after ingest, rows gain turn cost, model, and token totals from the session JSONL

The dashboard server binds its HTTP port immediately on startup; Claude Code JSONL discovery and ingest run in the background so the UI stays responsive.

Session similarity uses a fast hash embedding by default. Build with `cargo build --release --features onnx` to enable all-MiniLM-L6-v2 via ONNX Runtime for semantic matching. `ctx setup` downloads the ~30 MB model automatically when built with the `onnx` feature. Next `ctx ingest` re-embeds all sessions with the better model.

Index Claude Code JSONL plus Desktop session logs into the DB:

```bash
ctx ingest
```

When running Claude Code in an IDE or terminal, the dashboard ingests hook payloads (`POST /api/hook/event`) and legacy paths still hit `POST /api/trigger-ingest` when `filter.js` runs under Node. Desktop sessions require a manual `ctx ingest` run (or the periodic background service where installed).

## A/B experiments (optional)

You can measure whether each gate (profile filter, system prefix, adaptive prefix, coaching) actually lowers cost per request. Add to `~/.ctx/config.toml`:

```toml
[ab_test]
profile_pct = 50
inject_pct = 100
adaptive_pct = 50
coaching_pct = 100
```

Each prompt gets an independent coin flip per feature. Control requests skip that gate but still appear in `hook_traces` with an `ab_group` label like `P:T I:C A:T C:T`. After ingest enriches rows with cost data, open the dashboard with `?dev=1` or enable `dev_mode = true` in config to use the Experiment tab. Settings also has sliders and Start/Stop 50/50 buttons.

Omit `[ab_test]` entirely for normal operation (all gates always on, no experiment metadata).

After enough enriched hook traces, ctx writes `~/.ctx/ab-results.json` with per-feature verdicts (beneficial, no benefit, harmful, insufficient data). Use the Settings recommendation card or:

```bash
ctx experiment status    # A/B config + recommendations
ctx experiment apply     # disable features with no benefit; clear experiment
ctx experiment reset     # remove ab-results.json
```

Set `auto_apply_recommendations = true` in config to apply recommendations automatically after each ingest.

## Context modes

Bundle profile + inject + coaching + adaptive into one named preset in `~/.ctx/config.toml`:

```toml
[modes.debug]
profile = "minimal"
inject_enabled = true
coaching_enabled = true
adaptive_prefix_enabled = false

[modes.review]
profile = "carrier"
inject_enabled = true
coaching_enabled = false
adaptive_prefix_enabled = true
```

```bash
ctx mode debug           # switch to a mode
ctx mode list
ctx mode show review
ctx mode save focus      # save current toggles as a mode
```

The dashboard Settings tab has a mode dropdown (`POST /api/settings/mode`). Request Trace rows show a mode chip when `active_mode` is set.

## Local event stream (Unix socket)

While `ctx dashboard` runs, a read-only socket listens at `~/.ctx/ctx.sock`. Newline-delimited JSON request/response (one line each, connection closes).

| Request | Response fields |
| --- | --- |
| `{"q":"profile"}` | `profile`, `mode` |
| `{"q":"budget"}` | `remaining_usd`, `used_usd`, `pct` |
| `{"q":"experiment"}` | `active`, `profile_pct`, … |
| `{"q":"last-trace"}` | `ts`, `profile`, `tokens_saved`, `cost_usd` |
| `{"q":"adaptive-status"}` | `enabled`, `chars`, `budget`, `stale` |

Shell prompt example (`~/.zshrc`):

```bash
ctx_prompt() {
  local p=$(echo '{"q":"profile"}' | nc -U ~/.ctx/ctx.sock 2>/dev/null | jq -r '.profile // empty')
  [[ -n "$p" ]] && echo " ctx:$p"
}
PROMPT='%~ $(ctx_prompt) %# '
```

tmux status bar:

```bash
set -g status-right '#(echo {"q":"budget"} | nc -U ~/.ctx/ctx.sock | jq -r "\"$\" + (.remaining_usd | tostring)")'
```

If the dashboard is not running, the socket file is absent. Consumers should fail silently.

## Simulate (dry-run)

Run a prompt through the full pipeline without consuming tokens:

```bash
ctx simulate --prompt "fix the bug" --cwd /path/to/project
ctx simulate --prompt "fix the bug" --all-profiles
ctx simulate --replay-last 5
ctx simulate --prompt "test" --json
```

Shows gates fired, tools stripped, tokens saved, injected context preview, and per-request cost estimate. `--all-profiles` compares every profile against the same prompt. `--replay-last N` re-runs recent hook traces to compare actual vs simulated results.

The dashboard has a Simulate tab under Dev mode (same gate as Experiment). `POST /api/simulate` returns `SimulateResult` JSON.

## Coaching (hook mode)

When `coaching_enabled` is true in `~/.ctx/config.toml` (default), `UserPromptSubmit` reads the tail of the session JSONL under `~/.claude/projects/` (matched by `session_id`), runs the same rule-based coach as the proxy path, and appends the suggestion to `hookSpecificOutput.additionalContext` so Claude sees it on the next model call. Severe correction fatigue (five or more correction-style turns in the last six user messages) returns `decision: "block"` with a `reason` so you start a fresh session instead of burning more context.

`additionalContext` from hooks is honored by the Claude Code CLI and Cursor. The VS Code Claude Code extension has a known limitation where `additionalContext` is not applied ([anthropics/claude-code#49063](https://github.com/anthropics/claude-code/issues/49063)); use the CLI or Cursor for coaching there, or rely on the visible block path for severe cases.

## Configuration

`~/.ctx/config.toml` holds `active_profile`, `monthly_budget_usd`, feature toggles, and proxy port. The session budget guard derives its alert threshold from `monthly_budget_usd` (see `budget_guard::session_threshold_usd`).

| Key | Default | Purpose |
| --- | --- | --- |
| `active_profile` | `all` | Currently active MCP filter profile |
| `inject_enabled` | `true` | When true, prepend `~/.ctx/system_prefix.md` via `additionalContext` on each prompt |
| `coaching_enabled` | `true` | When true, scan session JSONL for correction cascades and re-asks; optional hard block on severe fatigue |
| `monthly_budget_usd` | (none) | Triggers budget alerts when projected spend approaches this limit |
| `session_gap_minutes` | `30` | Idle minutes between turns before a new session boundary in analytics |
| `proxy_port` | `8788` | Local MITM proxy listen port |
| `dashboard_port` | `8789` | Dashboard HTTP listen port |
| `active_mode` | (none) | Last mode applied via `ctx mode` or dashboard |
| `auto_apply_recommendations` | `false` | Apply self-tuning after ingest when true |

## Repository layout (short)

| Path | Role |
| --- | --- |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Contributor architecture, data flows, pipeline, storage |
| `src/` | CLI, proxy, filters, analytics aggregation, dashboard server |
| `src/daemon.rs` | launchd / systemd / fallback background install |
| `src/host.rs` | IDE vs terminal detection for setup output and MCP paths |
| `assets/filter.js` | In-process request rewriting + JSONL append |
| `src/dashboard.html` | Embedded dashboard UI |
| [`scripts/install.sh`](scripts/install.sh) | One-liner binary installer (no Rust required) |
| [`.github/workflows/release.yml`](.github/workflows/release.yml) | CI release pipeline: builds macOS + Linux binaries, publishes GitHub release |

## Tests

```bash
cargo test
```
