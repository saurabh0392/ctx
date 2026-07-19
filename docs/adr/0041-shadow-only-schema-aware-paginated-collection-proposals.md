# ADR 0041: Shadow-only schema-aware paginated collection proposals

Status: proposed
Date: 2026-07-18
Tracking: CTX-76
Parent: `docs/tool-trimming-architecture-revamp.md`
Builds on: ADR 0039 and ADR 0040

## Context

ADR 0040 established a local-only output-schema boundary, but the only registered transform still
operates on plain text blocks. Treating a structured list as generic text can break the relationship
between the model-readable `content` projection and `structuredContent`, drop a continuation cursor,
or change item order without recording what was omitted.

“List-like” is not enough evidence to authorize a collection transform. A result may contain
multiple arrays, an array may be an unpaginated detail field, tuple schemas can assign meaning by
position, and prose in a text block may not be a faithful serialization of the structured value.
The first collection strategy therefore needs deliberately narrow eligibility and an independently
validated structured-edit contract.

## Decision

Add `mcp-paginated-collection` version 1 ahead of the generic text strategy. It is eligible only
when all of the following are true:

1. a valid advertised output schema and valid object `structuredContent` are available;
2. the observed object and schema identify exactly one top-level array;
3. a schema-declared cursor, page-info, continuation, or total-count sibling provides pagination
   evidence;
4. the array does not use positional `prefixItems` semantics;
5. exactly one plain text block parses as JSON and is value-identical to the source structured
   object; and
6. the reserved text-projection field `_ctxOmission` does not collide with source data.

Schema-less, ambiguous, unpaginated, positional, non-mirrored, and colliding shapes pass through
with stable content-free reasons. Tool names cannot authorize the strategy.

Extend `McpTransformProposal` with an optional typed structured replacement. The replacement is an
enum of known edit families rather than a generic JSON patch. The first family records the target
collection field and retained source indices. The validator independently proves that:

- the expected structured source is not stale;
- every non-target structured sibling is value-identical;
- retained indices are unique, increasing, and include the first and last source item;
- each retained item is value-identical to its source index;
- content-block envelopes and all non-target blocks remain unchanged; and
- the candidate reparses and satisfies the advertised output schema.

Selection is deterministic head-and-tail sampling. It retains at least two items and respects an
advertised `minItems`, caps retained items at 64, preserves source order, and chooses the largest
candidate that fits the text budget. A bounded binary search constructs only logarithmically many
candidates instead of repeatedly cloning a hostile maximum-size result. Constraints such as
`contains` are enforced by candidate schema validation; a candidate that no longer satisfies them
fails open.

The model-readable text projection remains valid JSON. It contains the candidate's structured
fields plus a deterministic `_ctxOmission` object with the collection field, original count,
retained count, omitted count, and `first-and-last` selection label. The marker is not inserted into
`structuredContent`, so a closed output schema remains valid. The validator removes and verifies the
marker, then requires the remaining text JSON to equal the structured candidate exactly.

Validated evidence records shape authorization, input/retained/omitted counts, text character
counts, and source/candidate schema outcomes. It stores no item values, IDs, cursors, or marker
content. The candidate remains inside the validator and is dropped; T3 is still the first phase
allowed to make a validated proposal model-visible.

## Rejected alternatives

- **Compress the JSON-looking text and leave `structuredContent` whole.** That creates two
  conflicting representations of one tool result.
- **Add the omission marker to `structuredContent`.** Closed schemas with
  `additionalProperties: false` correctly reject undeclared fields.
- **Infer pagination from the tool name.** Names such as `list`, `search`, and `get` do not prove
  result semantics.
- **Choose one array when several are present.** A wrong target can discard a secondary collection
  whose meaning CTX does not understand.
- **Accept prose as a “close enough” mirror.** CTX cannot prove that regenerated prose preserves the
  server's semantics.
- **Expose a generic structured JSON patch.** That would let new strategies bypass family-specific
  invariants and turn the validator into an apply surface prematurely.

## Consequences

CTX gains its first schema-aware structured strategy without weakening the shadow-only boundary.
The strategy supports a conservative but common MCP result shape and produces an explicit omission
count while preserving cursor, totals, order statements, and retained identities.

The strict JSON-mirror requirement intentionally passes through many current servers that return a
prose summary alongside structured data. Future strategy versions may support declared projection
templates or gateway-native client capability negotiation, but they must establish an equally
testable text/structured coherence rule and collect new activation evidence.
