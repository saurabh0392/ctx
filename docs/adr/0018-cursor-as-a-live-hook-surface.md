# 0018. Cursor as a live hook surface

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh, CTX
- Spike for: CTX-27

## Context

ctx's neutrality is the moat: one honest layer across Claude Code, Cursor, and later Codex, which
no platform vendor will build. Today ctx only *ingests* Cursor transcripts after the fact, so its
Cursor labels are lower-confidence and it cannot act there. CTX-27 asked whether ctx can run a real
Cursor hook and act, or whether transcript ingest is the ceiling.

It is not the ceiling. Cursor ships a first-class hooks system (`~/.cursor/hooks.json`, schema
version 1) with command hooks that exchange JSON over stdin/stdout, which is the exact model ctx
already uses for Claude Code (`hook user-prompt-submit`, `hook post-tool-use`). Verified against
Cursor's hooks documentation:

- `postToolUse` input includes `conversation_id` (stable per conversation = our session),
  `generation_id` (changes per user message = our turn), `workspace_roots` (our cwd),
  `tool_name`, `tool_input`, and `tool_output` (JSON-stringified result). That is the same payload
  ctx records for Claude tool calls.
- `postToolUse` output may return `additional_context` for any tool, and
  `updated_mcp_tool_output` **for MCP tools only**, which replaces the output the model sees.
- `preToolUse` can return `permission` (allow/deny) and `updated_input` to rewrite a call.

## Decision

Treat Cursor as a first-class live hook surface, not just a transcript source. Register a ctx
command hook in `~/.cursor/hooks.json`, mirroring how `claude_settings.rs` idempotently merges ctx
into `~/.claude/settings.json`, and add a hook entrypoint that parses Cursor's payload and feeds
the existing decision pipeline with `surface = "cursor"`.

Ship it in increments, each separately verifiable:

1. Live observation (foundation). A `postToolUse` hook that records each Cursor tool call as a ctx
   decision with real session/turn/cwd/tool data. This graduates Cursor from "ingest, lower
   confidence" to "observed live," gives the loop-health and compaction views real Cursor data,
   and directly unblocks CTX-31 (persist Cursor turns).
2. Act on MCP outputs. Use `updated_mcp_tool_output` to apply ctx trimming to Cursor MCP tool
   results live, behind the same causal gate as Claude. Shipped: CTX-33, ADR 0021.
3. Cross-surface view. One honest dashboard view across Claude and Cursor, with explicit
   "unknown" where a surface has no data.

## The asymmetry we are accepting

On Cursor, `postToolUse` can replace output only for **MCP tools** (`updated_mcp_tool_output`).
For built-in tools (Read, Shell, Grep) it can add context but cannot shorten the output the model
sees. So on Cursor, ctx can trim MCP results live but can only observe built-in tool results.
This fits ctx's headline (MCP schema and result trimming is the main value); built-in trimming
stays a Claude-Code capability for now, and the cross-surface view must state this honestly rather
than imply parity.

## Verified live (Cursor 3.7.19)

Confirmed by capturing real hook payloads from a running Cursor agent, not just the docs:

- `postToolUse` fires for every tool we care about, including `Shell`, `Read`, `Grep`, `Write`,
  `Delete`, and MCP (`MCP:<tool>`). There is no need for a separate `afterShellExecution` hook to
  see terminal output; `postToolUse` already carries it.
- Cursor's Shell `tool_output` is shaped `{"output":"...","exitCode":N}`. It uses `output`, not
  the `stdout` field Cursor's own docs example shows, so the parser reads `output` first.
- The top-level `cwd` (and `tool_input.cwd`) come back empty in practice; `workspace_roots[0]` is
  the reliable working directory, which is what the parser uses.
- The payload also carries `generation_id` (per user message / turn) and `transcript_path`, which
  later increments can use for turn-level joins and Cursor narration.

## One owner per tool call across hook systems (CTX-37)

Registering ctx on both `~/.cursor/hooks.json` and `~/.claude/settings.json` creates a real
collision: a Claude model running inside Cursor honors **both** hook systems, so a single tool
call fires the Cursor `postToolUse` hook *and* the Claude `post-tool-use` hook. Left alone, each
event is recorded twice, once as `surface = "cursor"` and once as `surface = null`, which doubled
every per-tool count and poisoned the signal corpus the precision work (CTX-32) depends on. A live
probe confirmed Cursor fires `postToolUse` exactly once per call, so this was ours to fix, not a
Cursor double-fire.

Rule: the Cursor hook owns Cursor tool calls. The Claude `post_tool_use` handler bails when the
payload is Cursor-shaped (it carries `cursor_version` / `conversation_id`, which a native Claude
Code CLI payload never does). This is source-based, not a time-window dedup, so it never suppresses
a genuine rapid re-read (which is itself a signal we measure). A real Claude Code session, in a
plain terminal or another repo, has no Cursor fields and keeps recording normally.

## Alternatives considered

- Stay ingest-only and just harden the transcript adapter. Rejected as the primary path: it keeps
  Cursor observe-only and lower-confidence, and concedes the "acts everywhere" moat. We keep the
  adapter as the fallback for environments where the hook is not installed.
- A Cursor extension/MITM instead of hooks. Rejected: heavier, more fragile, and redundant now
  that native hooks expose the same payload and an output-replacement path for MCP.

## Consequences

- ctx becomes genuinely cross-agent at the action layer, not just in analysis. The neutrality
  claim in the competitive docs becomes demonstrable.
- New install surface to own and tear down cleanly: `~/.cursor/hooks.json` must be merged and
  unmerged idempotently, exactly like the Claude settings, and covered by setup/uninstall.
- The MCP-vs-built-in asymmetry must be surfaced honestly in the UI and docs. Claiming parity
  would be the kind of misleading metric this project treats as a defect.
- Cursor data flowing through the live pipeline raises Cursor label confidence and feeds the
  loop-health, compaction, and per-tool proof views without waiting on transcript ingest.
