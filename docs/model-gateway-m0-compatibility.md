# Model gateway M0 compatibility ledger

Status: implementation evidence in progress

Observed: 2026-07-21 on macOS

Scope: read-only probes and synthetic redaction contracts; no runtime route is enabled

## What M0 proves

M0 separates four facts that the product must never collapse into one support badge:

1. the client is installed;
2. a documented or explicit configuration boundary exists;
3. a request with client-side tool results was captured crossing that boundary; and
4. a mutation was accepted by the next model request.

The implementation in `src/model_gateway/` proves only the first two and provides a safe harness
for collecting the third. The three fixtures under `tests/fixtures/model_gateway/` are deliberately
marked `synthetic-redaction-contract` and `liveRouteProof: false`. They prove redaction and
correlation behavior, not platform compatibility.

## Local machine result

The default `ctx model-gateway probe --surface all --json` performs passive executable/config
inspection. A separately authorized `--run-client-version` pass reported:

| Surface | Installed client | Current route through CTX | M0 decision | Why |
| --- | --- | --- | --- | --- |
| Claude Code | 2.1.153 | none | **NARROW** | `ANTHROPIC_BASE_URL` is a documented user route, but it is not configured here and subscription/API-key/bearer paths need separate captures |
| Cursor | 3.9.16 macOS app; no CLI on `PATH` | none | **HOLD** | Cursor documents BYOK in the UI but also says requests pass through its backend for final prompt assembly; no local OpenAI-shaped boundary is proven |
| Codex | 0.145.0-alpha.18 | none | **NARROW** | user-level `openai_base_url` and custom providers are documented, but neither route is configured here and ChatGPT/API auth need separate captures |

Therefore, the honest answer today is **zero model-path-trimmed requests on this machine**. This is
not a regression in existing native hook, shell-wrapper, or MCP-gateway coverage.

## Route decisions

### Claude Code: NARROW to explicit gateway routes

Candidate: Anthropic Messages over HTTP/SSE when the user explicitly configures
`ANTHROPIC_BASE_URL`. Keep subscription, `ANTHROPIC_AUTH_TOKEN`, and `ANTHROPIC_API_KEY` as distinct
activation identities. Hosted tools and traffic that bypasses the configured endpoint are
unavailable.

Claude Code's Bedrock, Vertex, and Foundry modes are detected as separate held routes. They are not
treated as Anthropic Messages compatibility: provider dialect, credential acquisition, and
post-mutation signing require their own later threat model and evidence.

Next proof: capture a sanitized real request and stream for each auth mode, verify client-side tool
use/result correlation, upstream errors and cancellation, then restore the original settings
byte-for-byte. Anthropic's official LLM-gateway documentation describes the base-URL boundary:
<https://docs.anthropic.com/en/docs/claude-code/llm-gateway>.

### Cursor: HOLD; do not infer a direct provider route from BYOK

Cursor's official API-key documentation says BYOK works only for a subset of chat models and that
requests still pass through Cursor's backend for final prompt assembly. That does not establish a
local OpenAI or Anthropic request that CTX may safely mutate. Specialized features also continue to
use Cursor's built-in models: <https://docs.cursor.com/settings/api-keys>.

The Cursor CLI documents a `--endpoint` option, but the CLI is not installed on this machine and
the endpoint's request contract has not been captured. Treat it as a separate proprietary surface
spike, not as evidence for Cursor IDE or OpenAI Chat Completions:
<https://docs.cursor.com/en/cli/reference/authentication>.

Next proof: install/probe the supported CLI separately or use a guided IDE experiment, capture the
actual loopback protocol without credentials/content, and decide support or kill. Until then,
Cursor model-path trimming is unavailable; native Cursor hook and MCP coverage remain separate.

### Codex: NARROW to documented user-level provider configuration

Candidates: the built-in OpenAI provider with user-level `openai_base_url`, or a selected custom
`model_providers` entry with `base_url`. Project-local provider redirects are not candidates because
Codex intentionally ignores them. The probe reads only allowlisted key presence and reports whether
`auth.json` exists; it never reads credential contents or emits provider IDs/URLs. File presence
alone does not determine whether the auth mode is ChatGPT login or API key.

Next proof: capture HTTP/SSE and WebSocket separately for ChatGPT login, API-key, and custom-provider
auth; prove Responses tool-call/result correlation; identify requests that bypass the route; and
restore the user profile byte-for-byte. OpenAI's Codex configuration reference is:
<https://developers.openai.com/codex/config-reference>.

Hosted OpenAI tools whose results never return through the client request remain unavailable.

## Capture privacy contract

`ctx model-gateway sanitize-capture` accepts an explicitly supplied JSON envelope from a file or
stdin and prints a sanitized receipt. It does not persist the input or output. The sanitizer:

- strips URL scheme, authority, query, fragment, and unknown path segments;
- keeps allowlisted header names but no values;
- reduces JSON values to bounded type/size shapes and drops arbitrary field names;
- replaces tool-call IDs with per-capture ordinals while retaining call/result matching;
- records only allowlisted protocol item types and counts, never tool names;
- records explicit compaction/error/restoration observations without their content; and
- bounds depth, object fields, arrays, and stream events.

Seeded-secret tests cover all three protocol fixture families and fail if any sentinel survives the
serialized receipt.

The default compatibility probe does not execute client processes. `--run-client-version` is
explicit because Codex was observed attempting startup maintenance even during `codex --version`;
the receipt records whether a client process ran and marks client-side mutation as possible whenever
it did. Neither mode writes CTX state or reads credential contents.

## M0 exit state

M0 is complete only after sanitized **live** captures replace or accompany the synthetic contracts
for every candidate route. The current decision is enough to begin the provider-neutral M1 runtime
behind transformation-off gates, but not enough to market any Wave 1 surface as actively
model-path-trimmed.
