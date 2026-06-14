# 0007. Wire the retention model in at the decision level, proven before it steers

- Status: accepted
- Date: 2026-06-11
- Deciders: Saurabh Sharan, ctx CTO partner
- Epic: CTX-15
- Phase 1 ticket: CTX-16

## Context

The learned retention model (`src/learn.rs`) is trained on every ingest and shown on the
Improving page, but it does nothing to live behavior. Its only caller is the offline benchmark
(`src/bench.rs`). Trims are decided by the heuristic (`compress::retain::plan_retention`), the
per-tool causal activation gate, and the edit/intent guards. So "ctx adapts to you" currently
tops out at whole-tool on/off switches, and the Improving page (after ADR-aligned reframing) can
only honestly call the model "research in progress".

The product decision (recorded here) is to make adaptation real at the level of the individual
decision: trim *this* read, not *that* one, learned from the user's own data. Two facts shape how:

1. **Today's features are not personal.** The 11 features in `feature_row` are all about the
   *shape* of the proposed trim (drop_ratio, risky_drops, focus_path, etc). Nothing about the
   file, tool, repo, or the user's working set. Personalization needs richer signals.
2. **A per-decision policy breaks the current proof.** The per-tool causal before/after is clean
   because the tool-level switch is exogenous. The moment the model chooses *which* decisions to
   trim, trimmed vs untrimmed decisions differ by the model's own choice, so any "it is safe"
   claim is selection bias. Honest measurement of a per-decision policy requires either randomized
   exploration or a construction that cannot do harm.

## Decision

Adopt Shape B: a per-decision model that steers trims, but only inside a strict honesty envelope,
delivered in gated phases. Each phase must pass before the next.

1. **See it (Phase 1, CTX-16).** Capture richer contextual/personal features at decision time and
   shadow-score every live decision with the current model, logging `model_score` and
   `would_model_apply` next to the real decision. Zero behavior change. This is the data the model
   is starving for today.
2. **Trust it (Phase 2).** Add small randomized exploration *inside the existing tool-level gate*,
   then run per-decision causal proof on that unbiased data. The model never expands what a tool is
   allowed to do.
3. **Wire it (Phase 3).** Only a tool whose model policy passes per-decision proof lets the model
   gate real trims, within the earned envelope, per tool and (where data volume allows) per repo,
   with a global fallback. The Improving page becomes true.
4. **Deeper personalization (Phase 4, stretch).** Edit-graph/symbol features, per-repo
   fine-tuning, cold-start transfer from the global model.

Invariant across all phases: the model may only ever *decline* a trim the existing gate already
permits until it has passed per-decision proof for that tool. It can never trim more than the
tool-level gate allows. This keeps harm bounded by construction and keeps every claim measurable.

### Design reference and locked decisions

The destination UI is prototyped in `docs/prototypes/improving-page.html` (approved). It defines the
Phase 1 data contract: what the proven Improving page renders is what we must start logging now.
Three product decisions are locked:

- **Personalization unit: per repo.** The page leads with "on this repo, ctx learned ...", so every
  decision records a stable repo key (Phase 1).
- **Interpretation: gated.** Plain-English "what it learned" only appears once a pattern is strong and
  stable; weaker signals are withheld, not narrated as fact.
- **Placement: split.** The Improving page holds the model's cross-cutting story and proof; per-tool
  model coverage and proof live in the Tools drawer, to avoid duplicating the gate story.

The Phase 2 exploration (deliberate spot-checks) is **approved in principle** by the product owner,
so the proof can be unbiased. It is still gated to Phase 2 and stays inside the tool-level envelope.

## Alternatives considered

- **Flip the model on once holdout AUC looks good.** Rejected outright. AUC is an offline accuracy
  score, not evidence that model-steered trims do not increase corrections on the user's sessions.
  This is the exact premature-success trap the Read trial taught us.
- **Keep it per-tool only (Shape A, safe veto).** Considered and deferred, not chosen as the
  destination. A pure veto can only trim less, so it is harm-neutral and trivially honest, and it
  would rescue tools stuck at "too close". But it does not deliver the "adapts to you" promise; it
  is a safety tweak, not personalization. We keep the veto property as the Phase 1-2 safety
  envelope and aim past it.
- **Retire the model entirely.** Rejected. The model is the only path to decision-level
  adaptation, which is the product's core differentiation versus a generic trimmer.

## Consequences

- Phase 2 carries a real product cost that needs explicit sign-off when we reach it: randomized
  exploration deliberately makes a small fraction of suboptimal trims on real sessions so the proof
  stays unbiased. We will not start Phase 2 without that sign-off, and exploration stays inside the
  tool-level gate so worst-case harm is bounded.
- The Improving page stays "research in progress" (ADR-aligned, honest) until a tool actually
  passes per-decision proof; only then does it claim the model decides real trims, with the proof
  shown.
- `compress_decisions` gains shadow-score and richer feature fields. Contributors changing the
  decision schema must keep these populated for later phases to train and prove honestly.
- Per-repo identity (a stable repo key) starts being recorded in Phase 1 so Phase 3 can train
  per-repo without a backfill.
