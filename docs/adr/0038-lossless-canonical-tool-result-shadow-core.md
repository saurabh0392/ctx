# ADR 0038: Lossless canonical tool-result core in shadow mode

Status: accepted
Date: 2026-07-18
For: CTX-71
Parent: `docs/tool-trimming-architecture-revamp.md`

## Context

CTX currently flattens native MCP results into one string before compression. The Cursor apply path
then rebuilds the response as one text block. That is safe only for the narrow live fixture it was
verified against: mixed content, `structuredContent`, annotations, metadata, and unknown extension
fields can be discarded if the same approach is generalized to more servers or the MCP gateway.

The gateway must not become a second implementation of this lossy rebuild. Native adapters and the
gateway need one result contract before either receives broader apply authority.

## Decision

Add a canonical exchange/result model and a lossless MCP `CallToolResult` adapter. Understood text,
image, audio, resource-link, embedded-text-resource, and embedded-blob-resource blocks are typed.
Each typed block retains its typed fields plus every extension field needed to reconstruct the
complete source map without duplicating large text or binary payloads; unknown or invalid blocks
remain opaque values.
The result separately preserves `structuredContent`, `isError`, `_meta`, and every vendor extension,
including the difference between an absent field and an opaque value CTX does not understand.

A no-transform parse/render must be value-identical. Malformed or unsupported envelopes fail open
to the original value. The exact original value remains in process memory as the recovery boundary;
T1 does not add persistence or telemetry for it.

Claude Code, Cursor, and Codex adapters parse this result alongside the existing flattened string.
The current MCP text compressor evaluates the typed text blocks and records only content-free
coverage/candidate metadata in shadow evidence. The controller, `CompressResult`, native wrappers,
activation decision, and model-visible result remain on the old path in this increment.

## Evidence required

- A checked-in coverage ledger names the source, platform version, OS, observation date,
  verification method, result selector, and replacement capability for every fixture.
- The corpus covers all MCP 2025-11-25 `CallToolResult` block types plus an unknown future block,
  structured content, errors, metadata, annotations, and vendor fields.
- Corpus and generated-input tests prove no-transform identity, opaque preservation, and fail-open
  behavior without panics.
- Native Claude Code 2.1.153, Cursor 3.7.19, and Codex 0.144.5 fixtures reach the same canonical
  parser through their platform adapters.
- A text-edit test proves that changing one typed text block preserves all non-text siblings and
  top-level fields, without granting that transform live authority.

## Consequences

This adds transient parse/memory work for MCP results while shadow evidence is enabled. In return,
CTX gets a single structural boundary that native hooks and the future local gateway can converge
on. No commercial claim broadens: Cursor remains MCP-only for clean replacement, Codex remains
observation-only for MCP results, and the gateway is still unimplemented.

The next increment may move protocol-aware strategies onto this contract. A later, separate apply
transaction must validate invariants, store the exact original, render once, and verify that the
model received the replacement before recording savings.
