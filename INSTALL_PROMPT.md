# ctx Installation Prompt

Paste this into Claude Code or Cursor chat. Claude will install ctx for you.

---

**Prompt:**

```
Install ctx, a local cost-optimization tool for Claude Code. It strips unused MCP tool schemas from API requests, tracks spend, and serves a dashboard. All data stays on this machine.

Steps:

1. Check that Rust is installed (`rustc --version`). If missing, tell me to install it from rustup.rs and stop.

2. Clone and install:
   If ~/Documents/ctx does not exist: git clone https://github.com/goshippo/ctx.git ~/Documents/ctx
   If it already exists: cd ~/Documents/ctx && git pull
   Then: cd ~/Documents/ctx && cargo install --path .

3. Run first-time setup:
   ctx setup --yes

4. Verify everything is running:
   - `curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8789/` should return 200
   - `echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | ctx mcp 2>/dev/null` should return JSON

5. Show me the output of `ctx status` when done.

If any step fails, show the error and suggest a fix. Do not skip verification.
```

---

## What ctx does after install

- **Saves tokens**: strips MCP tool definitions your sessions don't use (42% reduction with auto-generated personal profile)
- **Tracks spend**: dashboard at http://127.0.0.1:8789 with savings, spend charts, efficiency scoring
- **LLM-accessible**: registered as an MCP server so you can ask about your ctx data in any chat
- **Privacy**: all data under ~/.ctx/, zero telemetry, no network calls beyond your normal Anthropic API traffic

## Available MCP tools (accessible from chat after install)

| Tool | What you can ask |
|------|-----------------|
| ctx_status | "What's my ctx status?" |
| ctx_spend | "How much did I spend this month?" |
| ctx_sessions | "Show me my recent sessions" |
| ctx_tips | "How can I reduce my Claude costs?" |
| ctx_patterns | "Any repeat patterns in my usage?" |
| ctx_settings | "What's ctx storing on my machine?" |
| ctx_profiles | "What profiles are available?" |

## Uninstall

```
ctx setup --uninstall
```
