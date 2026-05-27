# ctx Installation Prompt

For a compatibility matrix and per-environment setup (IDE, terminal, Claude Desktop), read the [README](README.md).

Paste this into Claude Code or Cursor chat. Claude will install ctx for you.

---

**Prompt:**

```
Install ctx, a local cost-optimization tool for Claude Code. It strips unused MCP tool schemas from API requests, tracks spend, and serves a dashboard. All data stays on this machine. The repo is https://github.com/goshippo/ctx.

Important: before every shell command, source the Rust/Cargo environment so the tools are on PATH:
  source "$HOME/.cargo/env" 2>/dev/null || true

Steps:

1. Check that Rust is installed. Run: source "$HOME/.cargo/env" 2>/dev/null; rustc --version
   If missing, tell me to install it from rustup.rs and stop.

2. Get the source (check these paths on the filesystem, not the current workspace):
   - Run: ls ~/Documents/ctx/Cargo.toml (does the source exist?)
   - Run: cd ~/Documents/ctx && git remote get-url origin (is it the right repo?)
   - If source exists and remote contains goshippo/ctx: git pull origin main
   - If source exists but not a git repo or different remote: skip git, build from what's there
   - If ~/Documents/ctx does not exist at all: git clone https://github.com/goshippo/ctx.git ~/Documents/ctx

3. Build: source "$HOME/.cargo/env" && cd ~/Documents/ctx && cargo install --locked --path .

4. Run setup: source "$HOME/.cargo/env" && ctx setup --yes

5. Install the GitHub MCP server so I can work with PRs, issues, and repos from chat (no Docker; uses the official pre-built binary).
   - Detect OS and CPU: uname -s and uname -m. Map to one of: Darwin_arm64, Darwin_x86_64, Linux_arm64, Linux_x86_64 (arm64 and aarch64 count as arm64; amd64 counts as x86_64).
   - Download the matching tarball from:
     https://github.com/github/github-mcp-server/releases/latest/download/github-mcp-server_<SUFFIX>.tar.gz
   - Extract so the executable ends up at ~/.local/bin/github-mcp-server (create ~/.local/bin if needed). chmod +x that file.
   - Use the absolute path to that binary in MCP config (Claude Desktop uses a minimal PATH; a bare command name often fails).
   - Read my MCP config file. For Cursor: ~/.cursor/mcp.json. For VS Code: check ~/.vscode/mcp.json or the workspace .vscode/mcp.json. Create the file if it does not exist.
   - Add a "github" entry under mcpServers with this shape (replace BIN_PATH with the real absolute path):
     {
       "command": "BIN_PATH",
       "args": ["stdio"],
       "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "<TOKEN>" }
     }
   - For the token: run `gh auth token` to get the current GitHub CLI token. Use that value. If gh is not installed or not authenticated, ask me for a GitHub Personal Access Token and stop.
   - If claude_desktop_config.json exists (check ~/Library/Application Support/Claude/ on macOS, ~/.config/Claude/ on Linux), add the same "github" entry there too so Desktop also gets it.
   - Do not overwrite existing mcpServers entries. Merge the new "github" key alongside any existing servers.

6. Verify:
   - curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8789/ should return 200
   - ctx status should print profile info

7. Explain to me what changed and what I need to do:
   - NODE_OPTIONS was added to ~/.claude/settings.json. I should use Command Palette → Reload Window (Cmd+Shift+P on macOS, Ctrl+Shift+P on Windows/Linux) so Claude Code picks up filter.js without quitting.
   - ctx registered as an MCP server. After reload I can ask about spend, tips, patterns in chat.
   - The GitHub MCP server was added to my MCP config. After reload I can work with GitHub PRs, issues, and repos from chat. It uses ~/.local/bin/github-mcp-server (no Docker).
   - Dashboard is at http://127.0.0.1:8789 and works immediately.
   - Tell me to run Reload Window and explain why (re-reads NODE_OPTIONS and MCP server config).

If any step fails, show the error and suggest a fix. Do not skip verification.
```

---

## What ctx does after install

- **Saves tokens**: strips MCP tool definitions your sessions don't use (~40% reduction with auto-generated personal profile)
- **Tracks spend**: dashboard at http://127.0.0.1:8789 with savings, spend charts, efficiency scoring
- **LLM-accessible**: registered as an MCP server so you can ask about your ctx data in any chat
- **Privacy**: all data under ~/.ctx/, zero telemetry, no network calls beyond your normal Anthropic API traffic

## Available MCP tools (accessible from chat after Reload Window or Desktop restart)

| Tool | What you can ask |
|------|-----------------|
| ctx_status | "What's my ctx status?" |
| ctx_spend | "How much did I spend this month?" |
| ctx_sessions | "Show me my recent sessions" |
| ctx_tips | "How can I reduce my Claude costs?" |
| ctx_patterns | "Any repeat patterns in my usage?" |
| ctx_settings | "What's ctx storing on my machine?" |
| ctx_profiles | "What profiles are available?" |

---

## Claude Desktop

Claude Desktop's bash tool runs in a cloud sandbox, not on your local machine. It cannot install software or run `ctx setup` for you. Use the one-liner below from a normal OS terminal (Terminal.app, Windows Terminal, etc.) instead.

**Prerequisite:** Rust must be installed. If `rustc --version` fails, install from https://rustup.rs first.

**One-liner install:**

```bash
bash ~/Documents/ctx/scripts/install-desktop.sh
```

Non-interactive (accept all GitHub MCP prompts):

```bash
bash ~/Documents/ctx/scripts/install-desktop.sh --yes
```

If you do not have the repo cloned yet:

```bash
git clone https://github.com/goshippo/ctx.git ~/Documents/ctx && bash ~/Documents/ctx/scripts/install-desktop.sh
```

The script clones/pulls the repo, builds ctx, runs `ctx setup --yes`, verifies the dashboard, runs initial ingest, then interactively offers the GitHub MCP server (pre-built binary into ~/.local/bin, optional gh token, optional merges into Desktop and Cursor configs). Pass `--yes` to accept all prompts without asking. Afterward, quit + reopen Desktop.

**What you get:** MCP tools in Desktop, the local dashboard, and session ingest/analytics.

**What you do not get:** Per-request tracing, tool filtering, and savings tracking from the hook. Those need `filter.js` (`NODE_OPTIONS`) or traffic through `ctx proxy`, neither of which applies to standalone Desktop chat. Desktop does not expose a configurable API base URL, so its traffic cannot be pointed at ctx. For full features, use Claude Code (CLI or IDE). Desktop session data is available via `ctx ingest` when local-agent logs exist.

---

## Teardown

For a clean removal, paste this into Claude Code or Cursor chat (Desktop users: run these commands manually in a terminal):

```
Completely remove ctx from this machine. Run `ctx setup --uninstall`, then delete ~/.ctx/, remove any leftover LaunchAgents at ~/Library/LaunchAgents/com.ctx.*, clean ctx from ~/.claude/settings.json mcpServers and NODE_OPTIONS, clean ctx from ~/.cursor/mcp.json, and clean ctx from claude_desktop_config.json. Verify nothing remains. If this is an IDE, tell me to use Command Palette → Reload Window.
```

Desktop users: after running the teardown commands, fully quit and reopen Desktop so removed MCP config takes effect.

Or use the slash command `/teardown-ctx` if the repo is in your workspace.
