# ctx: keep your coding agent's context lean

This is the source material for a NotebookLM overview aimed at alpha users. It is written to be read
aloud and turned into a short product explainer. Numbers are real, measured on a working install.

## The problem

Coding agents like Claude Code are expensive because they resend a lot of the same context on every
turn. Two costs dominate and both are invisible until the bill arrives.

The first is tool output. When the agent runs `git status`, reads a file, or greps the tree, the full
result gets pushed back into the context window and paid for again on later turns. A single noisy
command can carry tens of thousands of characters the agent glanced at once and never needed in full.

The second is the tool menu itself. Every connected MCP server (Linear, Figma, Notion, and the rest)
ships its whole catalog of tool definitions on every request, whether the agent uses one tool or none.
That fixed tax rides along turn after turn.

On one real developer's machine, ctx watched about eight thousand dollars a month of Claude spend flow
past. Most teams never see where it goes.

## What ctx is

ctx is a small local tool that trims what your coding agent sends and receives, and shows you the bill.
It runs on your machine, keeps your data on your machine, and sends no telemetry. Nothing about your
code or prompts leaves the laptop.

It does three things.

## One: trim tool output, reversibly

When a tool returns more than the agent needs, ctx shortens it in place and leaves a short marker. The
agent keeps working on the trimmed version. If it turns out the full text was needed, the agent calls
`ctx_expand` with the id in the marker and gets the verbatim original back. Nothing is lost, it is just
not paid for until it matters.

Measured on that same machine: across roughly eleven hundred trims, 67.5 million characters of tool
output came in and 2.1 million went to the model. That is a 97 percent cut on trimmed output, and every
byte is one `ctx_expand` call away.

## Two: prune the MCP tool menu, and earn it

ctx watches which MCP tools you actually use. A server that ships forty tools but only ever sees ten
invoked is carrying thirty tools of dead weight on every request. ctx prunes the ones that go unused
and keeps the ones that work.

The safety rule is strict: a tool that is in use is never cut. A server with live usage is never
disconnected wholesale, it only sheds its idle tools. If the agent later reaches for something that was
pruned, `ctx_restore` brings it back for the next session and carries a note of what you were doing, so
the work resumes with the tool present.

Measured: 8.7 million tokens of tool-menu input tax reclaimed across 357 requests, without ever cutting
a tool that was in use.

## Three: show the bill

The dashboard turns all of this into one honest number. What you spent, what ctx reclaimed, which tools
earned their place and which are on trial. No vanity metrics. If two numbers would disagree, that is a
bug ctx treats as a bug.

## Why it is safe to try

Everything is reversible. Trims expand on demand. Prunes restore on demand. The tool learns from your
own behavior, so it gets more accurate the more you use it, and it fails closed: when it is unsure, it
leaves your context alone rather than risk cutting something you need. It is local-first with no account
and no data leaving your machine.

## The alpha ask

Install ctx and use your coding agent exactly as you normally would. Do not change how you work. Let it
run for a week.

Then tell us two things. First, did anything feel wrong: a trim that hid something you needed, a tool
that went missing, a number on the dashboard you did not trust. Second, did the spend on your dashboard
move in a direction you liked.

There is a Report button in the dashboard. It files an issue for us directly, and you can attach a
screenshot. You do not need repo access and you do not need a GitHub account. That is the whole loop:
use it, and when something is off, press Report.

That is ctx. It keeps the context lean, keeps you in control, and shows you the bill.
