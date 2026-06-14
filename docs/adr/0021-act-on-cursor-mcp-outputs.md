# 0021. Act on Cursor MCP tool outputs live

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh, CTX
- For: CTX-33 (CTX-27 increment 2)

## Context

CTX-27 increment 1 (ADR 0018) shipped a live Cursor `postToolUse` hook that only observed: it
recorded a `surface = "cursor"` decision per tool result and never changed what the model read.
CTX-34 (ADR 0020) then made "observe" honest by forcing `applied = false` on that path.

Cursor's `postToolUse` can replace tool output, but only for MCP tools, via `updated_mcp_tool_output`.
Built-in Read, Shell, and Grep output cannot be rewritten by a `postToolUse` hook (for non-MCP tools
Cursor discards the hook response fire-and-forget; confirmed by the Cursor docs and Cursor staff).
So the next honest step is to act on the one *output* channel Cursor exposes (MCP), behind the same
per-tool causal gate ctx uses on Claude (ADR 0012), and keep built-ins observe-only on this path
rather than fake parity.

> **Later correction (CTX-39).** The original wording below said built-ins "cannot be rewritten by a
> hook" and "stay observe-only on Cursor no matter what." That is accurate for the **`postToolUse`
> output** path this ADR is about, but it overstates the platform limit. The **`preToolUse` input**
> path can rewrite a Shell command before it runs (`git status` -> a ctx-wrapped compacted run), the
> way RTK does, so the compacted result returns as Shell's own output. So Shell *is* reachable on
> Cursor via input rewrite, just not via output rewrite. Read/Grep built-ins remain unreachable
> either way. Whether ctx adopts the input path is a separate spike (CTX-39); the MCP-output
> decision recorded here still stands unchanged.

Two facts had to be confirmed against a real payload, because the docs alone misled us once already
(the Shell `output` vs `stdout` discovery in ADR 0018):

- Cursor names MCP tools `MCP:<tool>` (e.g. `MCP:get_issue`), not Claude's `mcp__server__tool`.
- A Cursor MCP `tool_output` is a JSON-stringified `{"content":[{"type":"text","text":...}],
  "isError":false}`. Captured live from Cursor 3.7.19.

## Decision

Teach ctx that an MCP tool is `mcp__…` (Claude) **or** `MCP:…` (Cursor) in one place
(`classify::is_mcp_tool`), so classification, the compressor allow-list, and the apply path all
agree. The Cursor hook then, when the surface-agnostic controller returns `apply` for an MCP tool
and the compressor actually shortens the result, returns `updated_mcp_tool_output` rebuilt in the
exact envelope Cursor sent (text content replaced, `isError` preserved) and records a **real**
apply (decision `applied = 1`, a compress_event, and the analytics counter), so Cursor savings show
up in the cross-surface view.

Built-in tools stay observe-only on Cursor no matter what the gate says: the apply path is guarded
by `is_mcp_tool`, so Read/Shell/Grep are never rewritten and never recorded as applied. The gate,
its thresholds, and the learned model are unchanged; this only routes an already-made decision to
the one output channel Cursor exposes.

## Alternatives considered

- Rewrite built-in output too (best-effort). Rejected: Cursor ignores `updated_mcp_tool_output` for
  built-ins, so it would silently do nothing while we recorded an apply. That is exactly the
  overstatement ADR 0020 fixed.
- Normalize Cursor's `MCP:get_issue` to a `mcp__` name before recording. Rejected: it would fork the
  provenance from the 16 Cursor MCP rows already recorded and from Cursor's own matcher format.
  Keep the name Cursor uses; only the MCP check is widened.
- Trust the docs' placeholder shape for `updated_mcp_tool_output`. Rejected: verified the real
  envelope on a live payload first, same discipline as the Shell field.

## Consequences

- ctx now trims live on both agents it supports, with a stated, non-parity asymmetry: Claude trims
  MCP and built-in output, Cursor trims MCP only. The Surfaces view flips Cursor to "acting" the
  first time a real MCP trim lands.
- `applied = 1` keeps its single meaning across surfaces: ctx actually shortened what the agent read.
- Proven live (Cursor 3.7.19): a real `MCP:get_issue` result was trimmed 2952 to 1875 chars through
  the installed hook, emitting `updated_mcp_tool_output` with the trimmed text and `isError` intact;
  a Shell result under a forced trial returned `{}` and recorded `applied = 0`. The offline-replay
  rows used to verify this were deleted so the corpus reflects only genuine sessions.
- Acting requires the MCP `kind` to be enabled (the `full` preset, a per-tool trial, or earned
  activation/burn-in), so trimming still only happens once a tool has earned it, exactly as on Claude.
