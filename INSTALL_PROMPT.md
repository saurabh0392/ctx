# Install ctx by pasting a prompt

ctx installs itself when you paste one prompt into your coding agent. This works in Claude Code (CLI
or IDE) and Cursor, where the agent can run shell commands. Claude Desktop cannot run shell commands,
so see the Desktop note at the bottom.

## Claude Code / Cursor: paste this

Replace `YOUR_TOKEN` with the alpha token you were given, then paste the whole block into the chat:

```
Install ctx for me. It is a local tool that trims my coding agent's context, the tool output it reads
back and the MCP tool menus it carries, reversibly, and shows a dashboard of what it saved. Everything
stays on this machine, no account, no telemetry.

Run exactly this, nothing else:

  curl -fsSL https://lkj2hle2qarv4liyggqpqtarr40fkhtw.lambda-url.us-east-1.on.aws/install.sh | CTX_TOKEN=YOUR_TOKEN sh

Then verify and report back:
1. Run: curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8789   (expect 200)
2. Give me the dashboard URL, and remind me to reload this window (Cmd+Shift+P or Ctrl+Shift+P, type
   Reload Window, Enter) so the new hooks and the ctx MCP server take effect.

Rules: do not clone any repo or build from source. If the token is rejected or the endpoint errors,
show me the exact error and tell me to get a fresh token. Do not retry with a different method.
```

That is the whole thing. The agent downloads a checksum-verified binary to `~/.local/bin`, wires ctx
into Claude Code (hooks plus the ctx MCP server), starts the dashboard at http://127.0.0.1:8789, and
adds `~/.local/bin` to your PATH.

## What you get

- Tool output is trimmed in place and is one `ctx_expand` call from whole again.
- Unused MCP tools are pruned from the menu, and a used server is never disconnected.
- A dashboard at http://127.0.0.1:8789 showing what ctx reclaimed, per tool and per server.
- The ctx MCP tools in your agent: `ctx_status`, `ctx_spend`, `ctx_waste`, `ctx_expand`,
  `ctx_tools`, `ctx_restore`, and more.

## Claude Desktop

Desktop chat cannot run shell commands, so the agent cannot install ctx for you there. Run the one
command above in a Terminal yourself, then fully quit and reopen Claude Desktop. The ctx MCP tools
(cost insight and recovery) then appear in Desktop. Note that per-turn trimming needs Claude Code
hooks, which Desktop does not have, so on Desktop ctx gives you the MCP tools and the dashboard, not
automatic trimming. See the README for the details.
