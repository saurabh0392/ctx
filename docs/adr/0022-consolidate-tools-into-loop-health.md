# 0022. One per-tool surface: fold Tools into Loop health

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh, CTX
- For: CTX-35

## Context

The dashboard had grown three surfaces that all showed per-tool trimming stage at different zoom
levels: the Home board (a teaser that linked into Tools), Loop health (ADR 0017: accrual over time
plus a light per-tool stage list), and Tools (the full per-tool before/after evidence with
confidence intervals, plus the only UI controls for trials). Loop health and Tools told nearly the
same story in two tabs, and the nav carried seven top-level tabs. That is the tab bloat and
concept drift our design bar warns against: the same idea, trimming proof per tool, scattered across
places that can drift apart.

## Decision

Keep Loop health as the single per-tool surface and fold the Tools content into it; remove the
standalone Tools tab. Loop health keeps its accrual lead and the arriving-over-time sparkline on
top (how much the loop has learned), then renders the rich per-tool cards below: the rough tally
vs the clean random-sample arms with confidence intervals, and the put-on-trial / stop / re-run
controls. The Home board now links to Loop health.

This is a pure dashboard information-architecture change. The proof math, the causal gate, the
trial mechanics, and every `/api/context*` endpoint are untouched; the same `mergeTools` +
`toolCard` render that powered Tools now runs inside `loadLoop`, and the lighter `loopStageRow`
list it replaced (and its `toolLabel` helper) were deleted.

## Alternatives considered

- Keep both tabs. Rejected: two tabs for one concept is exactly the drift this removes, and it
  forces the user to learn where "the real evidence" lives.
- Delete the Tools page and keep only Loop health's light stage list. Rejected: that would lose the
  before/after evidence and the only UI trial controls, which are the most defensible part of the
  product. The evidence is the keeper; the redundant tab is what goes.
- Keep Tools as the tab and drop Loop health. Rejected: "Loop health" is the more honest name for
  the combined story (how much has the loop learned, and where does each tool stand), and the
  accrual sparkline is unique to it.

## Consequences

- One place answers "what is ctx doing to each of my tools, and can I trust it": accrual on top,
  evidence and controls below. Six top-level tabs instead of seven.
- The Tools-tab IA from ADR 0017's era is gone; ADR 0017 is marked superseded in part. The
  loop-health data contract (`loop_health` in `/api/context`) is unchanged, so nothing server-side
  moved.
- Some `lh-*` CSS that styled the old stage rows is now unused; left in place to keep this change
  display-only and low-risk, to be swept later.
