# ctx on Windows: getting started

Welcome. This guide takes you from a clean Windows machine to a working ctx install, shows you around, and answers the questions that come up first. It should take about five minutes.

## What ctx is

ctx is a self-learning context controller for Claude Code. It watches your real coding sessions, learns what each tool's output actually needs to keep for the next step in *this* repo, and trims the rest. It also hides MCP tools you never call and tracks what each session costs.

Two things worth knowing up front:

- Nothing leaves your machine. There is no cloud service and no LLM in the hook. Your sessions are indexed into a local database and shown on a dashboard that only your machine can reach.
- Trimming starts off. For a while ctx just observes: for every tool result it records what it *would* have dropped, then checks the next few turns to see whether dropping it would have caused a re-read or a correction. A tool only starts trimming for real once its own collected evidence clears the bar, and the original text always stays in your transcript.

## Before you start

You need three things:

1. **Windows 10 (build 1803 or newer) or Windows 11.** The installer uses the built-in `tar.exe`, which shipped in 1803.
2. **Claude Code**, in a terminal or in an IDE (VS Code, Cursor, Windsurf). ctx wires itself into Claude Code's hooks and MCP config.
3. **An alpha token and the install endpoint.** Ask the maintainer for both. The token gates the download; the endpoint is the URL you install from.

## Install

Open PowerShell and run one command, with your token in place:

```powershell
$env:CTX_TOKEN='<your-alpha-token>'; irm <endpoint>/install.ps1 | iex
```

That command does the whole thing:

1. Asks the endpoint for a short-lived, checksum-verified download of the Windows binary.
2. Verifies the SHA-256, then unpacks `ctx.exe` into `%LOCALAPPDATA%\ctx`.
3. Adds that folder to your user PATH so you can type `ctx` in a new terminal.
4. Runs `ctx setup` to wire everything up and open the dashboard.

If the endpoint rejects the request, your token is wrong, revoked, or there is no Windows build yet. If you see `tar.exe not found`, your Windows is older than build 1803.

### The SmartScreen prompt

The binary is not yet signed with an Authenticode certificate, so Windows may warn you. Two cases:

- Running the install command above does **not** trip SmartScreen, because PowerShell launches `ctx.exe` directly.
- If you later double-click `ctx.exe` in Explorer and see "Windows protected your PC", click **More info**, then **Run anyway**. Signing is on the roadmap and will remove this.

## What setup just did

After install, `ctx setup` has:

- Created `%USERPROFILE%\.ctx`, where the database, config, and logs live.
- Added ctx hooks to `%USERPROFILE%\.claude\settings.json` (these run on each prompt and tool result).
- Registered the ctx MCP server in `%USERPROFILE%\.claude.json`, so Claude can call the ctx tools.
- Registered two Scheduled Tasks: `ctx-dashboard` (starts at logon and keeps the dashboard up) and `ctx-ingest` (indexes new sessions every five minutes).
- Opened the dashboard at http://127.0.0.1:8789.

Filtering of MCP tools is **off** by default, so setup does not hide any tools. It starts by observing and learning.

## Guided tour

### 1. Reload Claude Code

The hooks and MCP server were written to config while Claude Code may have been open. Apply them:

- In an IDE: press `Ctrl+Shift+P`, type **Reload Window**, and press Enter.
- In a terminal: open a new terminal so the changes are picked up.

### 2. Open the dashboard

Go to http://127.0.0.1:8789 (setup opens it for you the first time). The home view has three panels:

- **Learning** shows what ctx is recording and confirms it has caused zero corrections so far.
- **Earning** shows which tools have turned on trimming and how many of your own runs stand behind each decision.
- **Improving** tracks the local model's version history as it retrains on your sessions.

The dashboard also has spend, recent sessions, and per-request traces.

### 3. Use Claude Code as usual

Work normally. ctx runs in the background: it indexes each session, records what it would trim, and tracks cost. For the first stretch it changes nothing about your output. That is by design.

### 4. Ask Claude to call the ctx tools

Inside a Claude Code session, ask for any of these. Claude calls them through the MCP server:

- `ctx_status` gives your active profile, session count, and savings so far.
- `ctx_spend` breaks down this month's API-equivalent usage estimate by token type and shows
  user-entered actual account spend separately when it is available.
- `ctx_sessions` lists your recent sessions with cost, duration, and model.
- `ctx_waste` lists MCP servers that loaded on every request but were never called in the last 30 days, so you can see what to prune.
- `ctx_tips` and `ctx_patterns` surface what is driving spend and where it repeats.

For example: "Call ctx_spend for this month" or "Use ctx_waste to show me unused MCP servers."

### 5. See the trim-and-recover loop

Once a tool has earned trimming, ctx shortens its output and leaves a marker like `[ctx trimmed ... id: X]`. If Claude needs the full text, it calls `ctx_expand` with that id and gets the original back, verbatim. The full output also stays in your transcript, so nothing is ever lost. This is the safety net that lets trimming be aggressive without risk.

### 6. Handy commands

From any terminal (after opening a new one so PATH is set):

```powershell
ctx status           # active profile and per-turn cost estimate
ctx gain             # cumulative token and cost savings
ctx context status   # collection progress and which tools have earned activation
ctx ingest           # index new sessions now, instead of waiting for the 5-minute task
ctx dashboard        # open the dashboard again
```

## FAQ

**Where does my data go?**
Nowhere off your machine. Sessions are indexed into `%USERPROFILE%\.ctx\ctx.db`, and the dashboard binds to `127.0.0.1` only. There is no account and no upload.

**A console window opened and stays around. Is that normal?**
The `ctx-dashboard` Scheduled Task runs the dashboard server, and on Windows that can show a console window. It is harmless. You can minimize it. A quieter launcher is a known follow-up.

**`ctx` is not recognized in my terminal.**
The installer adds `%LOCALAPPDATA%\ctx` to your user PATH, but an open terminal keeps its old PATH. Open a new terminal. The hooks and dashboard call ctx by full path, so they work regardless.

**Does the dashboard survive a reboot?**
Yes. `ctx-dashboard` is triggered at logon, so it comes back after you sign in. You do not need to keep a terminal open.

**The dashboard did not open, or the page will not load.**
Give it a few seconds after setup and refresh http://127.0.0.1:8789. If it still fails, run `ctx dashboard` in a terminal to start it in the foreground and see any error. You can also check the `ctx-dashboard` task in Task Scheduler.

**I ran setup while Claude Code was open and hooks did not apply.**
Reload the window (`Ctrl+Shift+P`, Reload Window) or start a new terminal. If you want to redo the wiring cleanly, close Claude Code and run `ctx setup` again.

**Will ctx hide tools I need?**
Not by default. MCP filtering ships off. If you later turn it on and a tool goes missing, ask Claude to call `ctx_tools` to see what was pruned, then `ctx_restore <name>` to bring it back for your next session.

**How do I change how much it trims?**
Trimming is earned per tool, but you can steer it. `ctx context on` opts into the safe preset (git, test, grep first). `ctx context off` keeps observing but stops user-facing trimming. `ctx context status` shows where each tool stands.

**How do I uninstall?**
Run:

```powershell
ctx setup --uninstall
```

That removes the ctx hooks and MCP entries from your Claude config, deletes the ctx Scheduled Tasks, and clears the statusline. To finish removing it, delete `%LOCALAPPDATA%\ctx` (the binary) and `%USERPROFILE%\.ctx` (the data), and remove `%LOCALAPPDATA%\ctx` from your user PATH.

**Which Claude surfaces work?**
Claude Code in an IDE and in the terminal get the full set: hooks, MCP filter, per-request tracing, the dashboard, and the ctx MCP tools. Claude Desktop gets the MCP tools and the dashboard after you restart the app, but not the hooks.

**It is asking for a token I do not have.**
The alpha is gated. Ask the maintainer for a token and the install endpoint. Without both, the installer will refuse to run.
