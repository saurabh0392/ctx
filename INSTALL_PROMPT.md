# ctx Installation Prompt

Paste this into Claude Code or Cursor chat. Claude will install ctx for you.

---

**Prompt:**

```
Install ctx, a local cost-optimization tool for Claude Code. It strips unused MCP tool schemas from API requests, tracks spend, and serves a dashboard. All data stays on this machine. The repo is https://github.com/goshippo/ctx.

Steps:

1. Check that Rust is installed (`rustc --version`). If missing, tell me to install it from rustup.rs and stop.

2. Get the source (check these paths on the filesystem, not the current workspace):
   - Run: ls ~/Documents/ctx/Cargo.toml (does the source exist?)
   - Run: cd ~/Documents/ctx && git remote get-url origin (is it the right repo?)
   - If source exists and remote contains goshippo/ctx: git pull origin main
   - If source exists but not a git repo or different remote: skip git, build from what's there
   - If ~/Documents/ctx does not exist at all: git clone https://github.com/goshippo/ctx.git ~/Documents/ctx

3. Build: cd ~/Documents/ctx && cargo install --path .

4. Run setup: ctx setup --yes

5. Verify:
   - curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8789/ should return 200
   - ctx status should print profile info

6. Explain to me what changed and what I need to do:
   - NODE_OPTIONS was added to ~/.claude/settings.json. I need to restart Cursor so Claude Code picks up filter.js.
   - ctx registered as an MCP server. After restart I can ask about spend, tips, patterns in chat.
   - Dashboard is at http://127.0.0.1:8789 and works immediately.
   - Tell me to restart Cursor and explain why (new process needed for NODE_OPTIONS and MCP).

If any step fails, show the error and suggest a fix. Do not skip verification.
```

---

## What ctx does after install

- **Saves tokens**: strips MCP tool definitions your sessions don't use (~40% reduction with auto-generated personal profile)
- **Tracks spend**: dashboard at http://127.0.0.1:8789 with savings, spend charts, efficiency scoring
- **LLM-accessible**: registered as an MCP server so you can ask about your ctx data in any chat
- **Privacy**: all data under ~/.ctx/, zero telemetry, no network calls beyond your normal Anthropic API traffic

## Available MCP tools (accessible from chat after restart)

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
