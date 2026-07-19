# ADR 0042: Shadow-only schema-aware search-result proposals

Status: proposed
Date: 2026-07-18
Tracking: CTX-77
Parent: `docs/tool-trimming-architecture-revamp.md`
Builds on: ADR 0039, ADR 0040, and ADR 0041

## Context

ADR 0041 added the first structured-result proposal, but its head-and-tail selection is wrong for
ranked search results. Search servers commonly place the most relevant match first. Retaining the
last result merely because it is last can discard a more relevant prefix result, while sorting by a
score field would let CTX reinterpret server semantics it does not own.

Names such as `search`, `grep`, `query`, `results`, and `matches` are not enough to prove a safe
rewrite. A result array may lack stable identities, a score may describe something other than
ranking, a location may be optional, and an advertised schema may allow entries that omit the
fields CTX relies on. Search eligibility therefore needs a schema-proven identity and a required
match, ranking, or location field on every item.

## Decision

Add `mcp-search-results` version 1 before the paginated-collection and generic-text strategies. It
is eligible only when all of the following are true:

1. a valid advertised output schema and valid object `structuredContent` are available;
2. exactly one observed top-level array has a non-positional array schema whose item schema is an
   object;
3. every item is required to carry at least one schema-declared stable identity field, such as an
   ID, URI, path, file name, or key;
4. every item is required to carry at least one schema-declared match, rank, score, relevance,
   snippet, line, range, location, highlight, or offset field;
5. exactly one plain text block parses as JSON and is value-identical to the source structured
   object; and
6. the reserved text-projection field `_ctxOmission` does not collide with source data.

The collection field name and tool name cannot authorize a proposal. A conventional top-level name
such as `results`, `matches`, `hits`, or `findings` is used only to produce a precise fail-open
reason when its schema is absent or insufficient. Two independently schema-qualified arrays are
ambiguous and remain unchanged.

Extend the typed structured-edit enum with a search-result family. The validator independently
proves that:

- the expected structured source is not stale;
- every non-target structured sibling is value-identical;
- retained indices are exactly `0..n`, with no gaps, reordering, or synthesized results;
- every retained result is value-identical to its source index;
- content-block envelopes and all non-target blocks remain unchanged;
- the JSON text projection equals the structured candidate after removing its marker; and
- the candidate reparses and satisfies the advertised output schema.

The server's source order is the ranking contract. CTX never sorts by a score, rank, distance, or
location field. Selection retains the largest source prefix that fits the target budget, respects
advertised `minItems`, retains at least one result, and caps retained results at 64. A bounded binary
search constructs only logarithmically many candidates.

The model-readable JSON projection includes an exact `_ctxOmission` object with the target field,
original count, retained count, omitted count, and `ranked-prefix` selection label. The marker is
not inserted into `structuredContent`, so closed output schemas remain valid.

Validated evidence records the strategy/version, schema authorization, proposal outcome, text
character counts, and input/retained/omitted result counts. It stores no query, path, ID, URI,
score, line, snippet, result value, or marker content. The candidate is dropped inside the validator;
T3 remains the first phase allowed to make a validated proposal model-visible.

## Rejected alternatives

- **Reuse head-and-tail collection sampling.** The final result is not inherently relevant and can
  displace a higher-ranked prefix entry.
- **Sort locally by a score-looking field.** Score direction, ties, normalization, and ranking
  semantics belong to the server contract and are not consistently declared by JSON Schema.
- **Authorize by tool or array name.** Names are hints, not structural proof that every retained
  result has an identity and match location.
- **Accept optional identity or match fields.** Presence in `properties` does not guarantee every
  array item carries the field used to preserve meaning.
- **Compress only the JSON text.** That would make `content` disagree with `structuredContent`.
- **Expose the validated candidate to adapters now.** T2 proves transformation invariants only;
  recovery, rendering, activation, and model-visible replacement remain T3 responsibilities.

## Consequences

CTX can now recognize a conservative class of ranked search results and prove a useful prefix
reduction without interpreting server ranking semantics. Search results that use scalar entries,
optional identities, optional match evidence, positional schemas, prose projections, unsupported
references, or ambiguous arrays pass through unchanged with stable content-free reasons.

The strict required-field and JSON-mirror rules intentionally leave some real servers unsupported.
Future versions may add verified server contracts or declared projection templates, but each wider
shape needs its own versioned evidence identity and equally testable validator invariants.
