Install ctx on this machine. Follow these steps exactly.

## Prerequisites

Confirm Rust is installed:
```
rustc --version
```
If missing, stop and tell the user to install Rust first: https://rustup.rs

## Get the source

Run these shell commands to check the user's system (not the current workspace):

```bash
which ctx 2>/dev/null && echo "BINARY_EXISTS" || echo "NO_BINARY"
ls ~/Documents/ctx/Cargo.toml 2>/dev/null && echo "SOURCE_EXISTS" || echo "NO_SOURCE"
cd ~/Documents/ctx 2>/dev/null && git remote get-url origin 2>/dev/null || echo "NOT_A_REPO"
```

Then act based on what the commands return:

- **SOURCE_EXISTS + remote contains `goshippo/ctx`**: `cd ~/Documents/ctx && git pull origin main`
- **SOURCE_EXISTS + NOT_A_REPO or different remote**: build from what's there, do not touch git
- **NO_SOURCE**: `git clone https://github.com/goshippo/ctx.git ~/Documents/ctx`

## Build and install

```bash
cd ~/Documents/ctx
cargo install --path .
```

Verify:
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
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8789/
```
Should return 200.

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | ctx mcp 2>/dev/null | head -c 100
```
Should return JSON with "protocolVersion".

```bash
ctx status
```

## What the user needs to do next

Explain this clearly to the user:

> ctx is now installed and running. Three things changed; two need a window reload in Cursor:
>
> 1. **Tool filtering**: ctx added `NODE_OPTIONS` to `~/.claude/settings.json`. When Claude Code starts a new process, it loads `~/.ctx/filter.js` which strips unused MCP tool schemas before each API request. This reduces token usage by ~40%. It does not affect your prompts or responses.
>
> 2. **MCP server**: ctx registered itself as an MCP server. After you reload the window, you can ask "what's my ctx spend?" or "show me cost tips" in any chat and Claude will call ctx directly.
>
> 3. **Dashboard**: Already running at http://127.0.0.1:8789. No reload needed for this.
>
> **Action required**: Press `Cmd+Shift+P` (macOS) or `Ctrl+Shift+P` (Windows/Linux), type `Reload Window`, and press Enter. This re-reads `NODE_OPTIONS` and MCP server config without quitting. If you only use Claude Code in a plain terminal, start a new session once instead.

Also mention:
- `ctx use carrier` or `ctx profile list` to switch MCP filter profiles
- `ctx setup --uninstall` reverses everything
- All data stays under ~/.ctx/. No telemetry.

## If something fails

- `ctx setup --uninstall` reverses everything
- `ctx setup --dry-run` shows what setup will do without changing anything
- LaunchAgents are at ~/Library/LaunchAgents/com.ctx.{proxy,dashboard,ingest}.plist
- All data lives under ~/.ctx/ (SQLite, config, logs)
- No data leaves the machine. Zero telemetry.
