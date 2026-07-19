# ADR 0039: Shadow-only MCP strategy proposals and invariant validation

Status: proposed
Date: 2026-07-18
Tracking: CTX-72
Parent: `docs/tool-trimming-architecture-revamp.md`
Builds on: ADR 0038

## Context

ADR 0038 gave CTX a lossless MCP result model, but its first shadow candidate still joined every
text block into one string. That can estimate savings, but it cannot say which block changed or
prove that non-target blocks, annotations, structured content, error state, metadata, and vendor
extensions remain intact.

Moving directly from that candidate to a native hook or gateway apply would combine three risks at
once: strategy selection, structural mutation, and model-visible replacement. The architecture
requires eligibility and structural validity to be proven before either can influence permission to
apply.

## Decision

Add a deterministic registry of block-aware MCP result strategies. Every registered strategy has a
stable ID, version, eligible result shape, invariant manifest, and maximum per-target expansion.
Changing behavior requires a new strategy version so earlier observations cannot silently authorize
it.

The first registry entry, `mcp-text-blocks` version `1`, may propose replacements only for plain MCP
`text` content blocks. It gives each text block a proportional share of the result budget and runs
the existing MCP text compressor independently inside that boundary. It never concatenates sibling
blocks and does not target embedded resources, structured content, images, audio, links, or unknown
blocks.

A proposal carries the target block index, expected source text, and replacement. It exists only in
process memory. Before CTX records a candidate, the validator must prove:

1. the strategy ID and version match the manifest;
2. the parsed source still round-trips to its exact raw value;
3. `isError: true` and opaque error states pass through without a proposal;
4. every target exists, is a unique plain-text block, and still has the expected source text;
5. no target exceeds the strategy expansion limit, total text stays within the proposal budget,
   and the proposal saves characters overall;
6. every top-level field except `content` is unchanged;
7. content-block count and every non-target block are unchanged;
8. each target keeps its complete envelope and changes only `text`; and
9. the rendered candidate reparses and renders identically.

Successful validation returns only content-free counts. The candidate value is dropped inside the
validator. Shadow evidence records eligibility, strategy version, whether a proposal was attempted,
validation status, replacement count, character counts, and a bounded reason code. None of those
fields grant apply authority.

## Rejected alternatives

- **Keep joining text blocks.** This loses block identity and cannot support structural invariants.
- **Let each platform adapter validate its own mutation.** Native hooks and the gateway would drift
  into different safety contracts.
- **Return an apply-ready rendered value now.** That would make an accidental live-path integration
  easier before recovery and transaction guarantees exist.
- **Trim error results when `compress_preserve_errors` is disabled.** Error-result transforms need
  their own strategy and evidence; the protocol-aware path preserves all errors in this increment.

## Consequences

CTX can now measure a transform separately from whether the current platform or evidence gate may
apply it. Unsupported shapes, invalid proposals, and errors remain unchanged with an inspectable
reason. The extra canonical clone/render work happens only while shadow collection evaluates a
proposal.

This increment does not add output-schema capture or validation, structured strategies, an atomic
apply transaction, recovery persistence, native adapter migration, or the MCP gateway. Those remain
separate gates in T2 through T4.
