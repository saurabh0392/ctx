# ctx

Rust CLI plus a Node `filter.js` hook for Claude Code. It strips MCP tool definitions you do not need, records one JSONL line per request, prepends optional coaching text, and ships a local dashboard.

## Install

Build from source with a stable Rust toolchain:

```bash
cargo build --release
# binary: target/release/ctx
```

First-time machine setup:

```bash
ctx setup
```

`setup` writes assets under `CTX_HOME` (default `~/.ctx`): `filter.js`, `filter-config.json`, optional CA material for the proxy, and merges Node `NODE_OPTIONS` plus Claude hook entries where configured.

## Two interception paths

1. **Default (`NODE_OPTIONS` + `filter.js`)**  
   Runs inside the Claude Code Node process. Tool filtering, analytics JSONL, auto-profile selection, inject / coach / behavior hints, and session budget prep all execute here. No TLS proxy required.

2. **HTTPS proxy**  
   Optional MITM path for the Anthropic API host. Same gates exist in Rust for parity tests and for teams who route traffic through the proxy. Start or stop it with the CLI commands you already use in this repo (`ctx` help output lists them).

Keep feature work aligned with the default path so dashboards stay populated for everyone.

## Profiles

Profiles live in the Rust side (`profiles` module) and export into `filter-config.json` for the hook. Each profile lists MCP server prefixes to **keep**; everything else is removed from the outbound tools array.

Switch the active profile:

```bash
ctx profile switch data
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

Session similarity uses a fast hash embedding by default. Build with `cargo build --release --features onnx` to enable all-MiniLM-L6-v2 via ONNX Runtime for semantic matching. `ctx setup` downloads the ~30 MB model automatically when built with the `onnx` feature. Next `ctx ingest` re-embeds all sessions with the better model.

Index Claude JSONL into the DB:

```bash
ctx ingest
```

## Configuration

`~/.ctx/config.toml` holds `active_profile`, `monthly_budget_usd`, feature toggles, and proxy port. The session budget guard derives its alert threshold from `monthly_budget_usd` (see `budget_guard::session_threshold_usd`).

## Repository layout (short)

| Path | Role |
|------|------|
| `src/` | CLI, proxy, filters, analytics aggregation, dashboard server |
| `assets/filter.js` | In-process request rewriting + JSONL append |
| `src/dashboard.html` | Embedded dashboard UI |

## Tests

```bash
cargo test
```
