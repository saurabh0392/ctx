# 0027. Retire profile filtering as a default

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh (with CTO partner)

## Context

ctx shipped with three pillars: MCP profile filtering (strip tools the user rarely calls before
they reach the model), proof-gated output trimming (shorten long tool output only after a causal
before/after shows trimming did not raise corrections or re-reads on the user's own work), and the
cross-surface truth layer (watch Claude Code and Cursor side by side).

Profile filtering is the odd one out. It removes tools on a usage heuristic, not on a safety proof,
which contradicts the standard ctx sets for everything else: prove a change is safe on your own work
before making it. A heuristic can strip a tool the agent then needs, and there is no per-user proof
gating that risk. In practice on the dev corpus it saved ~0 tokens (0%), and the only outcome signal
available pointed the wrong way. It also dilutes the product narrative: two pillars done excellently
beat three when one is unproven.

We considered, and rejected for now, fully removing the filtering subsystem (see Alternatives).

## Decision

Ship with profile filtering off by default and stop presenting it as a core promise.

- Fresh-install defaults: `filter_mode = off`, `active_profile = all`, `auto_profile_enabled =
  false`. A new install strips no MCP tools and never turns filtering on by itself.
- `ctx setup` no longer enables soft filtering or writes ctx-managed `permissions.deny` rules by
  default. `FilterMode::Off` already strips all ctx-managed deny/allow rules, so a fresh setup ends
  with zero ctx filter rules.
- Filtering stays fully available as an opt-in: `ctx use <profile>` and `ctx filter mode soft|strict`
  work exactly as before. The CLI, config fields, ingest paths, and dashboard surface remain.
- Copy that framed filtering as a core promise is softened (the dashboard "how ctx decides" pillar
  and the setup output) to lead with proof-gated trimming and the cross-surface view, and to call
  filtering an opt-in.

## Alternatives considered

- **Full removal of the filtering subsystem** (CLI, dashboard tabs, config fields, ingest paths).
  Cleanest product, but a large, irreversible diff, and we are not yet certain no user segment
  (heavy-MCP setups with many servers) benefits. Deferred; revisit once usage data confirms nobody
  relies on it.
- **Keep filtering on by default, just fix the metrics.** Rejected: the issue is not the dashboard
  numbers, it is that the feature changes what the agent sees without proving it is safe.
- **Leave the dev machine switched off and change nothing in the product.** Rejected: that is a
  personal workaround, not a product decision, and new users would still get an unproven default.

## Consequences

- New users get a smaller, more honest promise: ctx shortens what the agent reads back once it has
  earned it, and shows the truth across agents. Nothing is stripped on a guess.
- Existing users are unaffected unless they reinstall fresh; their saved `filter_mode` is respected.
  Anyone who wants filtering turns it on in one command.
- The filtering code is now dormant-but-present. It must keep working (opt-in path is tested) but it
  is no longer on the default path, so it should not accrue new investment. A future ADR can decide
  full removal.
- Because the subsystem stays in the tree, there is a small ongoing maintenance cost and a risk of
  bit-rot in an opt-in path. The opt-in is covered by existing filter/profile tests; the new
  fresh-install test pins the default to off so it cannot silently flip back on.
