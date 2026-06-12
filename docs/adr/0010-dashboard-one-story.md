# 0010. Dashboard: one page, one story

- Status: accepted
- Date: 2026-06-12
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-19
- Supersedes: 0003

## Context

ADR 0003 cut the dashboard from nine tabs to five and moved the framing off cost. That was the
right direction, but the Home screen kept growing competing stories on top of each other: a
Learning / Earning / Improving loop, a model-research log with retrain history, a Phase 2
experiment panel, a proof headline, a savings line, an efficiency gauge, and a preset control.
Each piece was individually honest, but together there was no single thread. The owner read the
page and could not follow it, which means a first-time customer cannot either. For a product whose
whole pitch is clarity and trust, a confusing front door is a defect.

The prototype `docs/prototypes/dashboard-v2.html` was built as the visual spec for the fix, with
its day-one (empty), proving (in progress), and earned states.

## Decision

The customer-facing Home is **one page that tells one story**, read top to bottom in plain
language, in four beats:

1. **What it is, and the honest status right now.** ctx makes your agent leaner without losing the
   thread, and a live status line that says exactly what it is doing (watching, testing, or
   trimming) and what it has not changed.
2. **Why you can trust it.** Three short pillars: learns you not the average, proves before it
   acts, never hides anything.
3. **What it is doing for you right now.** The real counts (decisions watched, judged, today), one
   plain sentence describing the safe randomized experiment, and a compact per-tool status list.
4. **The payoff.** Real earned savings when a tool has cleared its proof, or an honest "nothing has
   earned it yet, here is what we are proving next" with no invented numbers.

Honesty is the pitch. For a truth layer, "we have not changed anything until your data says it is
safe" is the most persuasive thing the page can say, so the empty and in-progress states are
designed to read as the product working, not as a gap.

Everything else becomes a quiet drill-down, off the main story:

- Per-tool receipts (the full causal before/after) stay on the **Tools** screen (`tab-proof`),
  reachable from a link at the bottom of the story, not surfaced as a primary metaphor.
- Profiles and data controls live under **Settings**.
- Model research, retrain history, and experiment internals are removed from the customer path.
  The model is real and still trains in the background, but it does not get its own customer
  screen and is not described to the user as steering trims, because it does not.
- The efficiency gauge and the Learning / Earning / Improving loop are removed.
- Activity (`tab-trace`) stays as a quiet link, not a headline.

Tab ids stay stable (Home is still `tab-context`, Tools `tab-proof`, Activity `tab-trace`). The
redesign rewrites the Home panel and demotes the rest in navigation; it does not delete the
drill-down panels, so deep links, the request-trace loader, and the stitch contract keep working.

## Alternatives considered

- **Three primary screens (Home, Tools, Activity).** This was the working plan, but the owner's
  feedback was explicit: still too much to follow. One story beats three screens for a first-time
  reader. Tools and Activity remain, but as drill-downs, not co-equal destinations.
- **Delete the other tabs entirely to force a single page.** Rejected: the proof receipts, profiles,
  and activity are genuinely useful for users who want them, and the stitch test pins those panels.
  Demoting beats deleting, and keeps a clean path to detail.
- **Keep the model-research and retrain log visible as proof of sophistication.** Rejected: it
  implies the model is doing something to the user's output that it is not. Showing it on the
  customer path trades honesty for the appearance of intelligence. It moves off the customer path.
- **Lead with tokens and dollars saved.** Rejected, consistent with 0003 and 0006: savings are the
  payoff at the end of the story, only counted once a tool has earned it, never the opening claim.

## Consequences

- The Home panel (`tab-context`) is rewritten end to end; `context.js` loses the loop, the model
  research view, and the standalone Phase 2 panel, and gains the four-beat story driven by
  `/api/context`, `/api/context/proof`, and `/api/context/model-progress`.
- The Phase 2 experiment is described in one plain sentence inside beat 3 instead of its own panel.
  The detailed control-vs-treatment evidence stays available on the Tools drill-down.
- Navigation is simplified so Home is the clear primary; Tools, Activity, Profiles, and Settings
  are secondary. The efficiency gauge markup is removed from the shell.
- The stitch test keeps passing: the drill-down tabs and their symbols still exist, so only Home's
  markup and copy change. Any assertion that pinned removed Home sub-views (the loop, the research
  log) is updated to match the one-story page.
- Spend/budget stays off the customer story; it is not reintroduced to the main page.
