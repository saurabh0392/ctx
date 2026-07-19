# ADR 0044: Shadow-only schema-aware tree/file-listing proposals

Status: proposed
Date: 2026-07-18
Tracking: CTX-79
Parent: `docs/tool-trimming-architecture-revamp.md`
Builds on: ADR 0039, ADR 0040, ADR 0041, ADR 0042, and ADR 0043

## Context

Tree and file-listing results are structurally different from generic collections. Their order can
describe traversal, paths carry identity, a shallow directory entry can be the only evidence that a
large branch exists, and a caller-provided root or depth changes which entries are relevant. A
generic head/tail or ranked-prefix reduction can therefore erase the project skeleton even when the
remaining JSON is schema-valid.

Generated and vendored branches are also the most repetitive part of many repository listings, but
names alone are not enough to authorize an arbitrary rewrite. `target` can be an authored directory,
an isolated child path can lack its parent anchor, path separators differ by platform, and a tool
may return nested recursive nodes rather than one ordered flat listing. The first strategy needs a
small contract whose protections can be independently re-derived.

## Decision

Add `mcp-tree-listing` version 1 before the generic text fallback. Ranked search continues to run
first. The tree recognizer explicitly declines any schema-backed shape carrying pagination evidence,
so the existing paginated-collection strategy still wins that contract. The strategy is eligible
only when all of the following are true:

1. a valid advertised output schema and schema-valid object `structuredContent` are available;
2. the resolved root schema is a direct object schema without composition, conditional,
   dependency, pattern-property, or minimum-property semantics;
3. exactly one required conventional root field is a string;
4. exactly one source array has a non-positional object-item schema with one required string path
   identity and one required string kind field;
5. the kind schema has a closed enum containing only `file` and `directory`/`dir` semantics;
6. every source entry is an object with a bounded relative path, a supported kind, and a unique
   normalized identity;
7. bounded tool-input inspection can prove any requested root and depth are unambiguous, supported,
   and consistent with the result root;
8. exactly one plain text block parses as JSON and is value-identical to the source structured
   object; and
9. the reserved text-projection field `_ctxOmission` does not collide with source data.

Version 1 is intentionally a rooted **flat listing**: nested paths may appear in an ordered top-level
array, but recursive `children` objects are unsupported. Source listings are capped at 2,048 entries,
omissions at 512, paths at 4,096 bytes, input inspection at 256 values and depth eight, and requested
depth at 64. Absolute entry paths, drive-qualified entry paths, empty or dot segments, path identity
collisions, positional arrays, `contains` rules, and unknown kind values fail open.

Input selectors are accepted only at the top level. Conventional root selectors include `root`,
`rootPath`, `basePath`, `directory`, `cwd`, and `path`; depth selectors include `depth`, `maxDepth`,
`levels`, and `maxLevels`. Multiple selectors, nested selectors, non-string roots, non-integral or
out-of-range depths, oversized inputs, and a root that does not match the result reject the strategy.
The requested root value remains transient and telemetry records only whether root/depth context was
present.

CTX never removes a normal source entry. An entry becomes an omission candidate only when:

- its normalized relative path has an exact segment from the version-1 generated/vendor allowlist;
- a corresponding directory entry for that segment exists in the source listing;
- the entry is a descendant of that directory rather than the directory anchor itself; and
- its path depth is greater than the caller's requested depth, or greater than one when no depth was
  supplied.

The allowlist is deliberately explicit and versioned: `node_modules`, `vendor`, `target`, `.next`,
`.nuxt`, `.svelte-kit`, `.cache`, `__pycache__`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`,
`.gradle`, and `coverage`. Similar substrings such as `vendorized` or `vendor.rs` are not matches.
The directory anchor remains visible, as do every non-generated entry and every entry at or inside
the protected requested depth.

Candidate order is deterministic: deepest eligible paths first, then normalized path, then source
index. CTX removes the smallest prefix that fits the character budget while preserving schema
`minItems`. Retained entries remain value-identical and in source order. The validator independently
re-derives the schema authorization, normalized identities, generated anchors, requested context,
eligible order, exact candidate, output-schema validity, and proof that the previous prefix did not
fit. A proposal cannot choose normal source files or over-trim a valid earlier prefix.

The text projection contains `_ctxOmission` with the array field, original/retained/omitted entry
counts, requested depth (or null), and the stable selection label
`generated-vendor-descendants-outside-requested-depth`. It contains no omitted paths or root value.
The marker is not inserted into `structuredContent`, so closed output schemas remain valid.

The typed edit, requested root, omitted indices, structured candidate, and text projection stay
inside the transient proposal/validator boundary. Evidence records only strategy/version,
authorization, schema outcomes, character and entry counts, requested-context presence, and stable
rejection reasons. No adapter, gateway, renderer, recovery path, or apply path receives the
candidate. T3 remains the first phase allowed to make a validated proposal model-visible.

## Rejected alternatives

- **Use the collection head/tail strategy.** Traversal order is not representative sampling and can
  remove the only visible project skeleton.
- **Drop every path containing a generated-looking substring.** Substrings misclassify authored
  names, and an unanchored child does not prove a generated branch.
- **Remove the generated directory anchor with its descendants.** The anchor is important evidence
  that a branch exists and explains the omission marker.
- **Ignore requested depth.** A caller that asks for a shallow or exact-depth view has made those
  entries part of the explicit result contract.
- **Collapse recursive `children` arrays in version 1.** Recursive schemas, mixed node layouts, and
  partial subtree markers require a separate invariant and evidence identity.
- **Accept absolute or parent-traversing entry paths.** Cross-platform normalization and containment
  would otherwise become part of the semantic trust boundary.
- **Trust proposal-supplied path classifications.** The validator must derive the same allowlist,
  anchors, depths, and order from the source contract.
- **Log omitted paths for debugging.** Paths can expose repository structure and are unnecessary for
  measuring proposal safety.

## Consequences

CTX can now study a useful, conservative reduction for schema-backed repository listings while
preserving the root, project skeleton, caller depth, source order, and authored files. The boundary
is narrow enough for exact independent validation and produces content-free evidence for future
activation decisions.

Many real tree tools remain unsupported: nested trees, absolute-path results, symlinks, open-ended
kind schemas, missing parent anchors, ambiguous selectors, and listings entirely inside the
requested depth pass through unchanged. Widening any of those boundaries requires a new strategy
version and new evidence rather than silently inheriting version-1 confidence.
