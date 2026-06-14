# 0026. Exclude ctx self-development from the learning corpus

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh (with CTO partner)

## Context

ctx learns whether trimming a tool is safe from the user's own sessions: it compares
correction and re-read rates on trimmed vs untrimmed decisions and only activates a tool once
the evidence clears the causal gate (ADR 0009, ADR 0012). The richer-signals work (ADR 0019,
CTX-32) adds candidate outcome signals and requires a hand-labeled precision spot-check before
any of them is allowed to vote.

While building CTX-32 we found the corpus was self-poisoned. On this machine the developer uses
Cursor almost entirely to build ctx itself, so the decision log was dominated by ctx development
activity: `cargo build`, `ctx setup`, re-editing the same source file, running `sqlite3 ~/.ctx`,
and terse dev replies ("yes", "merge 18") that the correction classifier reads as corrections.

The numbers were stark. Of 68 joined decisions carrying a candidate signal, 53 came from the ctx
source repo. Of the whole causal-gate corpus, the ctx repo was the single largest contributor
(476 decisions) and carried roughly half of all observed corrections. ctx was, in effect,
learning from the noise of being built. Any precision measurement on that corpus would be
meaningless, and worse, the gate that already ships was being fed false-positive corrections.

Building ctx is not user behavior. It must not influence what ctx trims for anyone.

## Decision

Tag every decision made inside ctx's own source repo with `self_dev` in `features_json`, and
exclude tagged rows from the learning corpus: the causal gate (`causal_tool_outcomes`), the
randomized experiment (`explore_tool_outcomes`), the learned model (`load_joined_decisions`),
the signal audit (`signal_audit_rows`), and the label-evidence view (`audit_labeled_decisions`).
Tagged rows are still recorded, so the Activity feed stays complete and honest about what ctx saw.

Identify the ctx repo by content, not by path: the nearest `.git` ancestor of the working
directory whose `Cargo.toml` declares `[package] name = "ctx"`. Content-based detection holds on
any machine and any checkout location, and a dependency or workspace member that merely mentions
ctx cannot trigger a false match. The flag is set at record time in `agent::decide`. A one-time,
guarded backfill tags historical rows by re-checking each stored `repo_key` against the same
content test, so the live gate de-biases immediately instead of waiting for old rows to age out.

The corpus-exclusion SQL keys off the exact compact serialization `"self_dev":true`. A unit test
pins both ends so a future serde change cannot silently turn the filter into a no-op.

## Alternatives considered

- **Filter by hardcoded repo path.** Simple, but path-specific, breaks on a different checkout or
  another developer's machine, and bakes one person's layout into the product. Rejected for the
  content test.
- **A `dev_mode` config flag the user sets.** Relies on the user remembering to flip it and keep
  it accurate. Silent failure when forgotten is exactly the case we are trying to fix. Rejected.
- **Exclude by command pattern (`cargo`, `ctx`, ...).** Brittle, never complete, and wrongly
  excludes a real user who legitimately runs those commands in their own project. Rejected.
- **Do nothing and let dev rows age out.** Leaves the shipping gate biased for weeks and makes the
  CTX-32 precision spot-check impossible now. Rejected; this is why we added the backfill.

## Consequences

- The causal gate, the experiment, the learned model, and the precision audit now reflect the
  user's real projects, not the churn of building ctx. On the dev machine the clean gate corpus
  dropped from ~727 decisions with ~22 corrections to ~251 decisions with 12 corrections, all from
  real projects (the-gaffer and others).
- The CTX-32 precision spot-check can finally be run honestly. Its verdict on this corpus: after
  exclusion only 15 signal rows remain, all of them residual ctx-dev activity that escaped the
  repo filter because their decision was recorded with no working directory (so no `repo_key`).
  There is effectively no clean, non-dev signal data yet, so no signal is promoted. The proof gate
  correctly blocks promotion. See `docs/cache-safety-spike.md` companion notes and CTX-32.
- Known limitation: decisions recorded with an empty cwd carry no `repo_key`, so neither the
  forward tag nor the backfill can attribute them to a repo. A few such ctx-dev rows remain in the
  corpus. They diminish as new, properly attributed data accrues; a follow-up can set `self_dev`
  from the session's workspace root when the per-decision cwd is missing.
- The filter is a column-free `features_json` token plus a shared SQL fragment, so adding a new
  corpus query means remembering to append the exclusion. The shared constant and the drift test
  keep this from going wrong silently, but it is a maintenance edge to watch.
