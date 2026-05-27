Install ctx on this machine. Follow these steps exactly.

## Prerequisites

Confirm Rust is installed:
```
rustc --version
```
If missing, stop and tell the user to install Rust first: https://rustup.rs

## Build and install

```bash
cd ~/Documents/ctx
cargo install --path .
```

Verify the binary:
```bash
ctx --version
```

## Run setup

```bash
ctx setup --yes
```

This does 6 things:
1. Creates ~/.ctx/ with filter.js, CA cert, config, system_prefix.md
2. Installs launchd agents for proxy (:8788), dashboard (:8789), and periodic ingest (Cursor only)
3. Wires NODE_OPTIONS into ~/.claude/settings.json so filter.js runs in-process
4. Indexes existing Claude sessions into ~/.ctx/ctx.db
5. Auto-generates an MCP profile from your tool usage history
6. Registers ctx as an MCP server in settings.json and ~/.cursor/mcp.json

## Verify

Run these checks:
```bash
# Proxy is up
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8788/
# Dashboard is up
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8789/
# Settings API responds
curl -s http://127.0.0.1:8789/api/settings | head -c 100
# MCP server works
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | ctx mcp 2>/dev/null | head -c 100
```

## Post-install

Tell the user:
- Restart Cursor/Claude Code to pick up NODE_OPTIONS and MCP server changes
- Open http://127.0.0.1:8789 for the dashboard
- The ctx MCP server is now available in chat. Ask "what's my ctx status?" or "show me cost tips" to use it.
- Run `ctx use carrier` to switch profiles, or `ctx profile list` to see options

## If something fails

- `ctx setup --uninstall` reverses everything
- `ctx setup --dry-run` shows what setup will do without changing anything
- LaunchAgents are at ~/Library/LaunchAgents/com.ctx.{proxy,dashboard,ingest}.plist
- All data lives under ~/.ctx/ (SQLite, config, logs)
- No data leaves the machine. Zero telemetry.
