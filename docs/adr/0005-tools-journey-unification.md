# 0005. Unify Proof and Earning into one Tools journey

- Status: accepted
- Date: 2026-06-11
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-13

## Context

ctx gates trimming on two separate ideas, and the dashboard showed them on two separate surfaces
in two different vocabularies:

- Earning: has a tool been watched on enough judged runs to have a verdict at all? Lived as the
  "What ctx is still studying" progress bars under the Home tab's Learning view, and as the
  Earning sub-view that only ever listed tools that had already finished.
- Proof: when ctx did trim a tool, did that make the user re-read or correct more often? Lived on
  the standalone Proof tab as a wall of Wilson/Newcombe intervals ("corrections changed by
  +3.2 pts [-1.1 pts, +7.5 pts]"), with insider phrases like "the after" and "would-trim runs".

A user cannot hold two gates on two pages in two dialects in their head. The Earning sub-view was
also misnamed: the act of earning (the climb) was on Learning, while Earning showed only the end
state, so it read as empty and meaningless until something had already won. The clickable prototype
(`docs/prototypes/tools-page.html`) was approved as the visual spec.

## Decision

Collapse both gates into a single per-tool journey on one page, labeled "Tools". Each tool is a
clickable row with one of five plain-English states, sorted wins-first:

- On (trimming now, or earned and waiting on the preset)
- On trial (ctx is trimming it live to collect the after)
- Too close (tested, intervals still cross zero)
- Watching (still on the climb; shows judged-run progress)
- Kept off (tested and trimming measurably hurt)

Clicking a row expands the story in plain English. The exact rates and confidence intervals stay
hidden behind a "Show the numbers" toggle, and even then are relabeled for humans ("You had to fix
it", "You re-read it", range read as "3.3% to 25.0%" with a one-line explanation).

The page is built entirely on the frontend by merging the two existing endpoints: `/api/context`
supplies the climb for watch-stage tools, and `/api/context/proof` supplies the causal before/after
for any tool ctx has wanted to trim. There is no backend, stats, or activation-gate change, so the
page can never disagree with what the engine actually does.

The standalone Proof tab is reused as this Tools page: the tab id stays `tab-proof` and the list
container stays `proof-list` so navigation, the stitch-test DOM contract, and the realtime loader
keep working. The Home tab's Earning sub-view is retired; its loop step now links to the Tools page,
and the "still studying" climb moves to Tools as the Watching rows.

## Alternatives considered

- **Keep Proof and Earning as two pages, just rewrite the copy.** Rejected: the two-gates-two-pages
  split is the core confusion; better wording on each page does not fix the mental model.
- **Show the full statistics by default with clearer labels.** Rejected: leading with intervals is
  what made the page unreadable. Plain English leads; the numbers are one click away for anyone who
  wants to audit them, which keeps the honesty without the wall.
- **Rename the tab id from `proof` to `tools`.** Rejected for now: renaming churns navigation, the
  realtime loader, and the stitch test for no user benefit. The UI label is "Tools"; the id stays
  `proof` (label/id mapping documented here, consistent with ADR 0003).

## Consequences

- One destination answers "which tools does ctx trim, and why", instead of two half-answers.
- `proof.js` now fetches both `/api/context` and `/api/context/proof` and derives state client-side;
  contributors changing either endpoint should keep the merged shape in mind.
- The Earning sub-view, `renderContextEarning`, the studying grid, and their DOM ids are removed;
  any code referencing them must be cleaned up (done in this change).
- Tab label "Tools" maps to id `proof` in code, the same kind of label/id indirection as Home
  (`context`) and Activity (`trace`).
