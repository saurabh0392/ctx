# ADR 0040: Bounded MCP contract cache and output-schema validation

Status: proposed
Date: 2026-07-18
Tracking: CTX-75
Parent: `docs/tool-trimming-architecture-revamp.md`
Builds on: ADR 0038 and ADR 0039

## Context

ADR 0039 proves that a text-block proposal preserves the MCP result envelope, but it does not know
the server's declared result contract. MCP `tools/list` may advertise an `outputSchema`; a server
that advertises one must return matching `structuredContent`, and clients should validate it. MCP
defaults an undeclared schema dialect to JSON Schema 2020-12 and currently restricts the output
schema root to `type: "object"`.

Schema capture is also a trust-boundary decision. A contract from one server or negotiated protocol
version cannot authorize a result from another. A malicious schema must not make CTX fetch remote
references, read local files, run an unbounded validator, or store result payloads in a long-lived
cache.

## Decision

Add a lossless canonical `tools/list` result parser. It extracts a named tool's `inputSchema`,
optional `outputSchema`, and annotations while preserving descriptions, icons, execution metadata,
pagination, unknown definitions, malformed future fields, and vendor extensions for a
value-identical no-transform render.

Add a bounded in-memory contract cache keyed by:

1. configured server identity;
2. negotiated MCP protocol version; and
3. case-sensitive tool name.

The cache stores only the typed schemas and annotations needed for policy. It does not retain the
raw `tools/list` envelope, descriptions, icons, vendor metadata, tool results, arguments, or
credentials. Duplicate names fail a page capture atomically. Capacity eviction is deterministic,
and a server's entries can be invalidated together after `notifications/tools/list_changed` or a
reconnect. Persistence and full pagination refresh semantics remain gateway work in T4.

Validate an advertised output schema before strategy selection and again against the in-memory
candidate inside the proposal validator. The validation boundary:

- defaults a missing `$schema` to draft 2020-12 and accepts the bundled draft 4, 6, 7, 2019-09, and
  2020-12 dialect identifiers;
- requires the MCP object root;
- validates the schema against its bundled meta-schema;
- accepts same-document `$ref`, `$dynamicRef`, and `$recursiveRef` targets only;
- compiles `jsonschema` without network or file resolution features;
- uses the linear-time regular-expression engine;
- bounds schema and instance node count, approximate bytes, and nesting depth before compilation;
  and
- returns only stable, content-free reason codes.

Because this adds a required invariant to validated text-block proposals, `mcp-text-blocks` advances
from version 1 to version 2. Version 1 observations cannot silently authorize the stricter contract.

Tool errors and opaque error states pass through before schema or strategy selection. A malformed
schema, missing or non-object `structuredContent`, schema-invalid source, unsupported dialect,
external reference, or bounded-input violation prevents proposal selection and leaves the original
unchanged. Successful evidence records schema advertisement and source/candidate validation
separately from strategy eligibility, proposal validity, and permission to apply.

The new contract-aware shadow entry point is preparatory. Current native post-tool hooks do not
provide a trustworthy matching `tools/list` lifecycle, so they continue to record
`not-advertised`. The local MCP gateway will populate the cache from the actual negotiated server
connection. No model-visible renderer or apply-ready candidate leaves this boundary.

## Rejected alternatives

- **Infer a schema from observed results.** Observed shape is weaker evidence than the server's
  declared contract and cannot validate omitted required fields.
- **Key only by tool name.** Common names such as `search` and `list` collide across servers and
  protocol revisions.
- **Allow the JSON Schema library to retrieve external references.** That would let untrusted tool
  metadata create undeclared egress or local file access during trimming.
- **Cache raw tool definitions or results.** The validator needs schemas, not another payload store.
- **Validate only after transformation.** A schema-invalid source is a server/contract mismatch and
  must pass through before a strategy claims eligibility.
- **Move schema checks into each adapter.** Native hooks and the gateway would acquire different
  structural safety rules.

## Consequences

CTX can distinguish “no schema advertised,” “source matches,” and a precise schema pass-through
reason without changing what the model receives. Future structured strategies can use the same
before/after boundary, and the gateway can reuse the cache without importing native-adapter logic.

The JSON Schema dependency increases compile size and supports more dialects than the first MCP
gateway may encounter. Disabling all resolver features and bounding inputs keeps its runtime trust
footprint narrow. Schema compilation is not yet memoized; T4 may cache compiled validators behind
the same contract identity if measurements show that compilation latency matters.
