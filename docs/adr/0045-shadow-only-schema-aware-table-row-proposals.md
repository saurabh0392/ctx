# ADR 0045: Shadow-only schema-aware table-row proposals

Status: proposed
Date: 2026-07-18
Tracking: CTX-80
Parent: `docs/tool-trimming-architecture-revamp.md`
Builds on: ADR 0039, ADR 0040, ADR 0041, ADR 0042, ADR 0043, and ADR 0044

## Context

Table results are not ordinary collections. A header gives every cell its meaning, rows must remain
rectangular, source order may be significant, and a summary count can describe more rows than are
present. Flattening a table into generic text can drop the header, splice cells from different rows,
or leave a model-visible sample that looks complete without stating what was omitted.

“CSV-like” is also not a sufficient parsing contract. Real delimited output varies in delimiter,
quoting, escaped quotes, line endings, comments, preambles, encodings, null conventions, and whether
newlines may occur inside cells. CSV has no standard comment or omission-row syntax. Guessing those
rules would make the parser and a synthetic marker part of the semantic trust boundary without any
server-advertised evidence.

The first table strategy therefore needs a narrow structured shape whose header, rows, schema, and
model-readable projection can all be checked independently. Raw CSV, TSV, Markdown tables, and
object-row tables remain separate future contracts.

## Decision

Add `mcp-table-rows` version 1 before the paginated-collection, entity/detail, and generic text
strategies. Ranked search and rooted tree remain earlier in the registry. The table recognizer
declines any payload carrying pagination evidence, including undeclared additional properties, so a
proven paginated shape may continue through the registry.

Version 1 accepts only a schema-backed object with:

1. one required conventional columns field named `columns` or `headers`;
2. one required conventional rows field named `rows` or `data`;
3. a columns array whose item schema is string-only;
4. a rows array whose item schema is a non-positional array with a homogeneous scalar-only item
   schema;
5. non-empty, trimmed, unique column names and a source row width exactly equal to the column count;
6. only string, number, boolean, or null cell values, with no nested arrays or objects;
7. a valid advertised output schema and schema-valid source `structuredContent`;
8. exactly one plain text block that parses as JSON and is value-identical to the structured
   source; and
9. no collision with the reserved text-projection field `_ctxOmission`.

Same-document schema references are followed through the existing bounded resolver. Composition,
conditionals, dependencies, positional `prefixItems`, `contains`, and unevaluated-item semantics are
unsupported in v1. Columns are capped at 128, source rows at 2,048, total cells at 65,536, column
names at 256 bytes, and retained rows at 64. A schema `minItems` above the retained-row cap is
unsupported rather than silently violated.

The strategy never rewrites a header or cell. It retains a deterministic first-and-last sample in
source order, including both endpoint rows, and respects the row array's `minItems`. Candidate sets
are nested by retained count. A bounded binary search chooses the largest allowed retained count
whose text projection fits the character budget. The validator independently re-derives schema
authorization, header uniqueness and width, rectangular scalar rows, schema minimum, exact
head-and-tail indices, candidate value identity, and proof that the next larger allowed sample does
not fit. A forged proposal cannot reorder rows or over-trim a sample that would have fit.

Every non-row structured sibling remains value-identical. That includes declared row counts, order
statements, types, units, and unknown extension fields. CTX does not decrement a server's total or
reinterpret it as the retained count.

The model-readable JSON projection contains the marker `_ctxOmission` with only the columns-field
and rows-field identifiers, column count, original/retained/omitted row counts, and the stable
selection label `first-and-last-source-order`. It contains no header names or cell values beyond the
retained candidate itself. The marker is not inserted into `structuredContent`, so a closed output
schema remains valid.

The typed edit, retained indices, structured candidate, and text projection remain inside the
transient proposal/validator boundary. Evidence records only strategy/version, authorization,
schema outcomes, character counts, column count, row counts, and stable rejection reasons. It never
records column names, cells, rows, tool input, or the candidate. No adapter, gateway, renderer,
recovery path, or apply path receives the proposal; T3 remains the first phase allowed to make a
validated candidate model-visible.

An obvious comma- or tab-delimited text block without the structured contract is detected only when
at least three non-empty lines have one consistent delimiter count across a bounded eight-line
sample, then fails closed with `table-raw-delimited-text-unsupported`. Two-line text is too weak a
signal and continues through the registry. CTX does not parse, normalize, or compress a detected
raw table through the generic fallback.

## Rejected alternatives

- **Infer a CSV dialect and trim lines.** Counting delimiters does not handle quoting, embedded
  newlines, preambles, comments, or encoding, and a line is not reliably a row.
- **Insert an omission row into CSV.** CSV has no reserved metadata row; a synthetic row can be
  mistaken for data and may violate column types.
- **Allow ragged rows and preserve whatever cells happen to exist.** Missing positions change which
  header applies to later values and cannot be repaired without inventing semantics.
- **Support object rows in the same version.** Object rows need required/identity-field rules and
  have different width, ordering, and omission semantics.
- **Use positional `prefixItems` to learn per-column types.** Draft-specific tuple semantics and
  additional-item rules deserve a separate version and evidence identity.
- **Recompute row-count siblings after trimming.** A count may mean the server-side total rather
  than the returned array length; changing it would be an unsupported semantic rewrite.
- **Select evenly spaced or random rows.** That loses deterministic nesting and makes exact
  largest-fitting validation harder without proven representativeness.
- **Log headers for debugging.** Column names can expose customer or domain data and are unnecessary
  for measuring structural safety.

## Consequences

CTX can now study a useful table reduction while proving that the complete header, exact retained
cells, source order, count metadata, schema validity, and omission disclosure remain coherent. The
selection and telemetry are deterministic and content-free enough to accumulate independent
evidence for a future T3 apply decision.

Many common outputs remain unsupported by design: raw CSV/TSV, Markdown, object rows, nested cells,
positional or heterogeneous schemas, duplicate headers, and paginated tables pass through. Each
widening requires its own explicit projection and validator contract rather than inheriting version
1 confidence.
