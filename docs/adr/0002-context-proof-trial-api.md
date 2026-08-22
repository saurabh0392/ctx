# 0002. Context proof and trial HTTP API

- Status: accepted
- Date: 2026-06-07
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-2

## Context

The causal before/after (SAU-150) is the product's differentiator, but it has lived only in the
CLI (`ctx context proof` and `ctx context trial`). The dashboard revamp (CTX-1) makes Proof a
first-class customer page, so the dashboard backend needs to serve the same numbers and let the
user start/stop a trim trial. We must not reinvent the statistics in JavaScript, and the page
must never show fabricated numbers.

## Decision

Add two HTTP endpoints to the dashboard server (`src/dashboard.rs`), alongside the existing
`/api/context` surface:

- `GET /api/context/proof` returns a `ProofView`: per-tool baseline vs trimmed counts, the
  correction and re-read rate with a 95% Wilson interval for each arm, the trimmed-minus-baseline
  delta with a 95% Newcombe interval, and a machine-token verdict per tool. It reuses
  `crate::db::causal_tool_outcomes`, `crate::stats` (Wilson/Newcombe), and
  `crate::compress::activation::{CausalThresholds, causal_clears_bar}` so the page, the CLI, and
  the live activation gate share one definition of "earned". The server computes all rates and
  intervals; the client only formats and writes copy.
- `POST /api/context/trial` with body `{ "tool": string, "on": bool }` reuses
  `crate::context_ctl::trial` to start/stop a single-tool trim trial.

Verdict tokens (client maps to humanized copy): `not_tested` (no trimmed runs), `collecting`
(some trimmed runs but either arm below the threshold), `safe` (clears the causal bar), `harmful`
(a delta interval is clearly above zero), `unclear` (otherwise). The view also carries the
thresholds (`min_baseline`, `min_trimmed`) and the active trial tools so the page can show an
honest "what unlocks the after" state.

## Alternatives considered

- **Reuse `ctx context proof --json` output shape directly.** Rejected: that JSON is the raw
  `CausalToolOutcome` counts with no intervals or verdict, which would force the stats math into
  JavaScript and risk the page disagreeing with the gate.
- **Compute rates/intervals client-side from raw counts.** Rejected for the same reason: one
  source of truth for Wilson/Newcombe must stay in Rust (`crate::stats`).
- **Shell out to the CLI from the server.** Rejected: slower, fragile, and the server already has
  direct DB access.

## Consequences

- The Proof page and the activation gate can never silently diverge, because both call
  `causal_clears_bar` over `causal_tool_outcomes`.
- Trial start/stop is now possible from the browser, which is a live intervention on real output.
  Each trial has to be asked for by name and stopped by name, and is surfaced on the page.

  Superseded 2026-08-21: this originally read "intentionally one tool at a time", enforced by
  `context_ctl::trial` replacing the whole list. That was wrong in practice. Each tool's comparison
  is scored on its own runs, so concurrent trials cannot contaminate each other, while replacing the
  list meant starting a trial silently cancelled the one already running and every "Put on trial"
  button but the last one clicked looked dead. Trials are additive.
- New response contract (`ProofView`) is now a public-ish surface the frontend depends on; changes
  to it must keep the client in sync. It is server-owned and versioned implicitly with the build.
- Read proof numbers remain "reference reads only" until CTX-9 lands the stratification (noted in
  ADR 0001); the API does not yet stratify.
