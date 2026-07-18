# CTX v0.5 claim ledger

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

## Historical evidence labels

- About 475,000 output tokens removed: single-machine historical result.
- Needed-whole holdout AUC 0.89, later 0.95: single-machine observational model result.
- Zero corrections across 2,731 decisions / 25 days: invalidation of a sparse correction signal, not evidence of no harm.

## Commercial hypothesis

The v0.5 wave tests interest at `$25/developer/month`; it does not collect payment. Do not describe
pricing, demand, team adoption, retention, or product-market fit as validated until the beta scorecard
gate is met.

`node scripts/coherence/claims.mjs` blocks a small set of superseded phrases on active surfaces.
