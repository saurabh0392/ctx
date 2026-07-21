# Model gateway M2 protocol adapter ledger

Status: implementation complete; M3 active mutation remains disabled

Date: 2026-07-21

Scope: independent wire-protocol adapters and content-local shadow decisions

## Shared shadow boundary

M2 adds a bounded canonical model exchange without changing M1's forwarding contract:

- raw request bytes remain on the relay stack and are never written to logs, SQLite, or files;
- one adapter is selected from the route's exact wire protocol—an adapter cannot fall through to a
  different dialect;
- tool identity comes only from a bounded call-id relationship, never from result text;
- duplicate, missing, excessive, or oversized identities and results remain whole with a specific
  coverage reason;
- each supported result carries its exact parsed JSON text-leaf path for M3, but M2 never writes
  through that path;
- the existing CTX strategy code computes the content-local would-do decision; and
- the health endpoint retains only counts and reason codes, with `rawRequestsPersisted: false`.

Content encoding is part of the gate. M2 inspects absent or explicit `identity` encoding only.
Other encodings still pass through byte-for-byte but record `unsupported-content-encoding`.

## Anthropic Messages pack

The `anthropic-messages-v1` pack recognizes request-local pairs between assistant `tool_use` blocks
and later `tool_result` blocks in the Messages `messages` array. It supports:

- parallel calls resolved by unique `id` / `tool_use_id`;
- object tool inputs and optional top-level tool input/output schemas;
- string results and arrays made entirely of text blocks;
- exact text-leaf paths, error classification, prior CTX-marker detection, and transient typed MCP
  parsing when a text result contains a canonical MCP result; and
- the same shared shell, read, search, edit, generic, and MCP shadow strategies used by native and
  MCP transports.

Mixed text/binary results, malformed inputs, missing/duplicate IDs, duplicate results, more than
512 correlated calls/results, more than 4,096 protocol items, call IDs over 512 bytes, and result
text over 2 MiB are observed as held-whole coverage rather than guessed.

## Fidelity evidence

Tests prove that:

- the original recorded Anthropic request remains byte-identical at the fake upstream after shadow
  parsing;
- authorization headers still follow M1's fixed-destination policy;
- the health receipt reports one correlated exchange and one strategy decision without content;
- out-of-order parallel results resolve to the correct call and tool identity;
- multiple text leaves preserve their exact structural paths;
- duplicate calls/results, missing calls, excessive IDs, multimodal results, and foreign protocol
  bodies produce explicit reasons and no canonical exchange; and
- a correlated Bash call is classified by the existing test-runner strategy rather than by adapter
  code.

## OpenAI Responses pack

The `openai-responses-v1` pack separately correlates these item families in the request `input`
history:

- `function_call` to `function_call_output`, with JSON-object arguments;
- `custom_tool_call` to `custom_tool_call_output`, with freeform input kept in a named field; and
- `local_shell_call` to `local_shell_call_output`, with a canonical Shell identity and bounded
  action object.

Each family has its own correlation scope, so an equal call ID cannot connect a function call to a
custom or local-shell output. Calls must occur before their result. Top-level function definitions
contribute only schema fields to the canonical contract; duplicate definitions provide no contract
authority. Text output retains the exact `input[index].output` path.

`apply_patch_call` and `apply_patch_call_output` are recognized but explicitly held as
`mutation-tool-held`; M2 does not run a candidate on mutation results. Non-string output remains
whole. String-only Responses input is a valid request with no local tool results, while Chat and
Anthropic bodies fail the Responses shape gate.

Tests cover all three eligible item families, schema capture, exact leaf location, shell strategy
reuse, family isolation, call ordering, malformed arguments, mutation holds, unsupported result
content, and cross-protocol rejection. The fake-upstream relay fixture proves the full recorded
Responses request remains byte-identical while the health receipt reports its content-free shadow
decision.

## OpenAI Chat Completions pack

The `openai-chat-completions-v1` pack correlates assistant `tool_calls[].id` with later
`role: tool` messages using a Chat-only correlation scope. Function arguments must be a JSON object.
Tool definitions contribute the nested function parameter schema. Result content may be a string or
an array made entirely of text parts, with exact `messages[index].content` text paths retained.

Responses input items and Anthropic `tool_use` / `tool_result` blocks do not activate Chat. Unknown
tool-call types, mixed-media result parts, malformed arguments, duplicate calls/results, missing
identities, and excessive shapes remain whole under the shared coverage reasons.

This protocol pack does not make Cursor routable. M0's Cursor route remains held; the independent
pack exists for a future captured Cursor boundary and the later Copilot BYOK wave.

## M2 exit evidence

Equivalent Anthropic Messages, OpenAI Responses, and OpenAI Chat fixtures normalize to the same
tool name, input object, output text, and shared strategy decision. A full 3-by-3 isolation matrix
proves each adapter produces an exchange only for its own body. Supported relay fixtures remain
byte-identical upstream, and unknown or ambiguous shapes produce content-free reason codes.

M2 does not persist shadow decisions as product proof, enable a client route, mutate a model
request, store rewind content, or claim model-visible savings. Those boundaries remain M3-M5 work.
