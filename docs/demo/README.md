# Making the ctx demo with NotebookLM

NotebookLM builds a narrated overview (audio or video) from sources you give it. It does not screen
record your app. It reads your material and generates hosts or narration over generated visuals. So the
quality of the demo comes almost entirely from the source doc and the steering prompt below.

## Steps

1. Open NotebookLM and create a new notebook.
2. Add `ctx-notebooklm-source.md` as a source (paste the text, or upload the file).
3. Add 2 or 3 real dashboard screenshots as image sources. Good ones to include:
   - the Home view with the cumulative reclaimed figure,
   - the See page showing input tax reclaimed per server,
   - the Save page showing a tool on the earn-it ladder.
   Screenshots make the video concrete instead of abstract.
4. Generate a Video Overview (for a narrated explainer) or Audio Overview (for a podcast-style
   two-host version). Paste the steering prompt below into the customization box before generating.

## Steering prompt (paste into NotebookLM's customization box)

> Make a 3 to 4 minute product explainer for developers who use AI coding agents like Claude Code.
> Audience: technical, skeptical, cost-aware. Tone: plain and direct, not salesy. Open with the hidden
> cost problem (tool output and tool menus resent every turn). Explain the three things ctx does: trim
> tool output reversibly, prune unused MCP tools without ever cutting one in use, and show the bill on a
> local dashboard. Use the real numbers from the source: 97 percent cut on trimmed output, 8.7 million
> tokens of tool-menu tax reclaimed, about eight thousand dollars a month of observed spend. Stress that
> everything is reversible and local-first with no telemetry. End with the alpha ask: install it, use
> your agent normally for a week, and press the Report button in the dashboard when something feels off.
> Do not overclaim. Do not use hype words.

## If you want a real screen-recorded demo too

NotebookLM cannot record the dashboard. If you want footage of ctx actually running, record it yourself
and use the source doc as your script. A tight 90 second cut:

1. A busy agent session with raw tool output, then the same with ctx trimming it, marker visible.
2. `ctx_expand` bringing the full output back, to prove reversibility.
3. The dashboard Home number, then the See page input-tax breakdown per server.
4. A pruned tool, then `ctx_restore` bringing it back next session.
5. The Report button filing an issue.

You can drop that recording into NotebookLM as a video source, or keep the two separate: the NotebookLM
overview for the story, your screen capture for the proof.

## Refresh the numbers before recording

The figures in the source doc were measured on one install. Pull current numbers so the demo matches
what an alpha user would see:

```
# tool-output trimming
sqlite3 ~/.ctx/ctx.db "SELECT SUM(chars_in), SUM(chars_out) FROM compress_events;"
# MCP menu input tax reclaimed
sqlite3 ~/.ctx/ctx.db "SELECT COUNT(*), SUM(tokens_saved) FROM requests WHERE tokens_saved>0;"
```
