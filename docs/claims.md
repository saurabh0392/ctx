# CTX claim ledger

This is the canonical language for active product, portfolio, release, and interview surfaces.

## Product claim

> CTX shows where a coding agent's context goes, then reclaims noisy tool output without losing the original.

## Evidence claim

> A tool earns activation when comparable randomized runs pass CTX's safety check on this machine.

This is behavioral evidence. It is not a causal proof, universal safety guarantee, or model benchmark.

## Eligibility claim

> Eligible tokens are removable under CTX's current transform.

Eligibility does not mean a tool has earned activation. Reports and the dashboard keep those concepts
separate.

## Privacy claim

> CTX has no background telemetry. Reports and beta check-ins leave the machine only after the user reviews the payload and chooses Send.

## Model-path claim

> When a named model route is explicitly enabled, requests pass through a CTX process on this
> device before CTX forwards them to the displayed model provider. CTX operates no cloud relay for
> this traffic. Only supported client-side tool-result fields may change. Exact originals are kept
> locally before a proposed trim is sent; only provider-accepted trims count as applied.

“Local” describes the CTX process, not the final model destination. Do not say prompts stay local,
that every request from a supported agent crosses CTX, that hosted tools are controllable, or that a
registered/shadow route is actively trimming. The exact surface, client version, authentication,
protocol, mode, lifecycle state, and fixed upstream are the claim boundary.

## Compaction claim

> Only a native post-compaction event proves completion. Pre hooks and historical transcript
> `pre_compact` markers are attempts; absent or unmatched completion evidence remains unknown.

Do not infer completed compaction from context-size or token drops.

## Historical evidence labels

- About 475,000 output tokens removed: single-machine historical result.
- Needed-whole holdout AUC 0.89, later 0.95: single-machine observational model result.
- Zero corrections across 2,731 decisions / 25 days: invalidation of a sparse correction signal, not evidence of no harm.

## Commercial hypothesis

The v0.5 wave tests interest at `$25/developer/month`; it does not collect payment. Do not describe
pricing, demand, team adoption, retention, or product-market fit as validated until the beta scorecard
gate is met.

`node scripts/coherence/claims.mjs` blocks a small set of superseded phrases on active surfaces.
