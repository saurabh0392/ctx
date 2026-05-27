Completely remove ctx from this machine. Follow these steps exactly.

## Step 1: Run the built-in uninstaller

If the ctx binary exists, use it to cleanly remove hooks, launchd agents, and MCP registrations:

```bash
which ctx && ctx setup --uninstall
```

This removes:
- NODE_OPTIONS from ~/.claude/settings.json
- PreToolUse hooks from ~/.claude/settings.json
- ctx MCP server from ~/.claude/settings.json and ~/.cursor/mcp.json
- LaunchAgents: com.ctx.proxy, com.ctx.dashboard, com.ctx.ingest

## Step 2: Kill any running ctx processes

```bash
pkill -f "ctx proxy" 2>/dev/null; pkill -f "ctx dashboard" 2>/dev/null; echo "done"
```

## Step 3: Remove the data directory

```bash
rm -rf ~/.ctx
```

This deletes: ctx.db, config.toml, filter.js, filter-config.json, profiles.toml, system_prefix.md, CA certs, analytics.jsonl, behavior-hints.json, all logs.

## Step 4: Remove leftover launchd plists (if uninstaller missed any)

```bash
rm -f ~/Library/LaunchAgents/com.ctx.proxy.plist
rm -f ~/Library/LaunchAgents/com.ctx.dashboard.plist
rm -f ~/Library/LaunchAgents/com.ctx.ingest.plist
```

## Step 5: Clean up MCP registrations (if uninstaller missed)

Check and remove ctx from both config files:

```bash
python3 -c "
import json, os
for path in [os.path.expanduser('~/.claude/settings.json'), os.path.expanduser('~/.cursor/mcp.json')]:
    if not os.path.exists(path): continue
    with open(path) as f: d = json.load(f)
    if 'mcpServers' in d and 'ctx' in d['mcpServers']:
        del d['mcpServers']['ctx']
        with open(path, 'w') as f: json.dump(d, f, indent=2)
        print(f'Removed ctx from {path}')
    if 'env' in d and 'NODE_OPTIONS' in d.get('env', {}):
        val = d['env']['NODE_OPTIONS']
        if '.ctx/filter.js' in val:
            d['env']['NODE_OPTIONS'] = ' '.join(p for p in val.split() if '.ctx/filter.js' not in p).strip()
            if not d['env']['NODE_OPTIONS']: del d['env']['NODE_OPTIONS']
            if not d['env']: del d['env']
            with open(path, 'w') as f: json.dump(d, f, indent=2)
            print(f'Removed NODE_OPTIONS filter.js from {path}')
"
```

## Step 6: Optionally remove the binary

```bash
rm -f ~/.cargo/bin/ctx
```

Skip this if the user wants to keep the binary for a quick reinstall.

## Step 7: Verify

```bash
which ctx 2>/dev/null && echo "WARN: binary still exists" || echo "Binary removed"
ls ~/.ctx 2>/dev/null && echo "WARN: data dir still exists" || echo "Data dir clean"
ls ~/Library/LaunchAgents/com.ctx.* 2>/dev/null && echo "WARN: plists remain" || echo "LaunchAgents clean"
python3 -c "import json; d=json.load(open('$HOME/.claude/settings.json')); assert 'ctx' not in d.get('mcpServers',{}); assert '.ctx/filter.js' not in d.get('env',{}).get('NODE_OPTIONS',''); print('settings.json clean')" 2>&1
```

## Tell the user

> ctx has been fully removed. Everything it installed is gone:
> - LaunchAgents (proxy, dashboard, ingest) stopped and deleted
> - ~/.ctx/ directory deleted (database, config, logs, certs, filter.js)
> - NODE_OPTIONS hook removed from ~/.claude/settings.json
> - ctx MCP server removed from settings.json and ~/.cursor/mcp.json
>
> **Reload the window**: `Cmd+Shift+P` (macOS) or `Ctrl+Shift+P` (Windows/Linux) → type `Reload Window` → Enter. This drops the old `NODE_OPTIONS` and MCP server state from the editor process without a full quit.
>
> To reinstall later: paste the install prompt or run `/install-ctx` in a fresh chat.
