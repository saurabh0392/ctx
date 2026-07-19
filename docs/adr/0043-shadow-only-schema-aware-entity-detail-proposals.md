# ADR 0043: Shadow-only schema-aware entity/detail proposals

Status: proposed
Date: 2026-07-18
Tracking: CTX-78
Parent: `docs/tool-trimming-architecture-revamp.md`
Builds on: ADR 0039, ADR 0040, ADR 0041, and ADR 0042

## Context

Collection and search strategies reduce repeated entries. A single entity/detail result has a
different safety problem: most of its value lives in individual fields, and deleting the wrong one
can remove the identifier, current status, requested projection, or recovery link even when the
remaining JSON still satisfies its schema.

JSON Schema proves which fields are required, but required fields are not the complete semantic
boundary. Servers often make `status`, `url`, or explicitly requested projection fields optional.
Tool names such as `get`, `fetch`, `read`, and `details` do not prove which output fields matter.
Likewise, a proposal cannot safely claim its own protected-field set; the validator must be able to
derive the same set from bounded source contracts and input.

Some object schemas also contain dependencies, conditionals, composition, or minimum-property
rules. Merely removing a field absent from `required` is unsafe when those constraints affect other
fields. The first entity strategy therefore needs a deliberately narrow schema subset.

## Decision

Add `mcp-entity-detail` version 1 after the more specific search and paginated-collection
strategies and before the generic text fallback. It is eligible only when all of the following are
true:

1. a valid advertised output schema and valid object `structuredContent` are available;
2. the resolved root schema is a direct object schema with declared properties and without
   composition, conditional, dependency, `minProperties`, pattern-property, or unevaluated-property
   semantics;
3. the object has at most 128 fields, every observed field is schema-declared, and at least one
   required string/integer field is a conventional stable identity such as an ID, key, slug,
   number, URI, or URL;
4. the stable identity is present and non-null;
5. bounded tool-input inspection can prove that any field selector is unambiguous and supported;
6. exactly one plain text block parses as JSON and is value-identical to the source structured
   object; and
7. the reserved text-projection field `_ctxOmission` does not collide with source data.

Tool input is inspected only as bounded semantic context: at most 256 values, depth eight, one
top-level selector, 64 requested fields, and 128 bytes per simple field name. Supported selectors
include `fields`, `properties`, `select`, `include`, `columns`, `projection`, and explicit return-
field variants. Nested, wildcard, object-shaped, malformed, conflicting, or schema-unknown
selectors reject the entity strategy. Input contents and requested field names never enter
telemetry.

The protected set is derived from:

- every schema-required field;
- the canonical stable identity field;
- every safely parsed requested field;
- observed status/state/lifecycle fields; and
- observed URI/link/permalink fields.

Version 1 may remove only a schema-optional top-level scalar that is either conventional verbose
prose or exactly duplicates a protected scalar value. Arrays, objects, nulls, protected fields, and
unknown non-duplicate fields are never candidates. Retained keys and values remain byte-for-value
identical; no string is summarized and no nested value is rebuilt selectively.

Removal order is deterministic: largest serialized values first, then duplicate proof, then field
name. CTX removes the smallest prefix of that order whose JSON projection fits the target budget,
with at most 64 omitted fields. The validator independently re-derives schema authorization,
requested and protected fields, candidate order, exact replacement, and the fact that the previous
prefix did not fit. A proposal cannot choose a more aggressive subset merely because it remains
schema-valid.

The model-readable text projection contains an exact `_ctxOmission` object with original, retained,
and omitted field counts, the omitted field names, and the
`schema-protected-entity-fields` selection label. The marker is not inserted into
`structuredContent`, so a closed output schema remains valid. Omitted names exist only in the
transient proposal and projection; validated evidence records content-free counts only.

The typed entity edit and candidate remain inside the validator. Evidence records
strategy/version, authorization, proposal result, schema outcomes, character counts, and
input/retained/omitted field counts. No native adapter, gateway, renderer, or apply path receives the
candidate. T3 remains the first phase allowed to make a validated proposal model-visible.

## Rejected alternatives

- **Prune every optional field.** Optional in JSON Schema does not mean semantically disposable.
- **Authorize by `get`/`details` tool names.** Tool names do not establish entity identity or output
  projection semantics.
- **Trust a protected-field list carried by the proposal.** A forged proposal could omit a requested
  or status field; the validator must derive protection independently.
- **Recursively prune nested objects or arrays.** That needs its own shape contract and would make
  field-level equality and recovery reasoning substantially harder.
- **Interpret arbitrary selector objects or dotted paths.** Their semantics vary by server and can
  describe relationships rather than output fields.
- **Support dependency/composition schemas by trial validation alone.** A first candidate may be
  invalid while another is valid, and trial-and-error selection would obscure the semantic reason
  for removal.
- **Summarize retained prose in place.** Keeping a field name while changing its value violates the
  exact-value invariant and can corrupt requested content.
- **Emit omitted field names as evidence.** Names can disclose user or server vocabulary and are not
  needed to measure strategy safety.

## Consequences

CTX can now prove a conservative reduction for schema-backed entity objects while preserving the
fields most likely to define identity, state, navigation, and caller intent. Exact duplicate scalars
and unrequested conventional prose can be studied without exposing a generic JSON-deletion escape
hatch.

The strict selector and schema subset intentionally leaves many real servers unsupported. That is
the correct T2 tradeoff: unsupported composition, nested projections, dependency rules, ambiguous
inputs, and non-mirrored prose remain unchanged with stable content-free reasons. Future versions
can widen coverage only with a new evidence identity and equally independent invariants.
