# 0036. Dashboard redesign: guided story, clean light, Home first

- Status: accepted
- Date: 2026-07-04
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: (redesign epic, to file)

## Context

ctx is going to alpha users. The dashboard reads like an instrument panel built for its author: eight
top-nav destinations, a dark canvas running green, amber, red, and four greys of text at once, stat
strips and before/after grids stacking numbers three deep, and almost no motion (one pulsing dot, a
0.25s view fade). Every metric is individually load-bearing and defensible. Together they are a wall
for a first-run user, who is Alex in `docs/personas-ctx.md`: 30 seconds of goodwill, no concept of
what ctx is, deciding keep-or-uninstall. The substance is strong; the presentation is what stands
between it and a first impression that lands.

ADR 0003 already fixed the information architecture (reposition off cost, five screens not nine, tell
truth and safety not tokens). This decision is about the visual and narrative layer on top of that
IA, not a reversal of it.

Three directional forks were put to the user and answered on 2026-07-04.

## Decision

**Layout is a guided story.** Home becomes a scroll-driven narrative in three beats (See where your
context goes, Save only what's safe, Trust that it's local and reversible), each beat animating in on
scroll. Chosen over a minimal "one number one action" Home and over a "calm cockpit" of three cards
because the make-or-break persona (Alex, first run) needs teaching, and the story is already the real
product structure, currently buried in a pillar card near the bottom of Home.

**Palette is clean light.** White paper canvas, near-black ink, one green accent used only on the
number that matters and the live pip. Amber and red are reserved for genuine warnings and never
decorative. Chosen over "near-mono dark" and "calmer dark, same family" as the most direct answer to
the user's "too much color," and to make ctx read like a document rather than a control panel.

**Scope is Home first.** Rebuild Home end to end as the new design system (tokens, type, motion,
spacing, shared CSS), ship it, prove it with the `fitcheck` skill, then roll the same language across
the other seven views in later passes. Chosen over a full eight-view rewrite (too much to react to at
once) and over "Home plus immediate nav collapse" (the nav regroup is captured as intent and applied
when the views are actually restyled, per ADR 0003's keep-ids-stable principle).

**fitcheck gates the rollout.** Each pass ships only when the `fitcheck` skill
(`.claude/skills/fitcheck/`) shows it beats the prior version across all five personas with no persona
regressing. The current dashboard is baselined first; that score is the bar.

The plan is `docs/redesign-dashboard-2026.md`; the visual spec is
`docs/prototypes/home-2026.html`; the personas are `docs/personas-ctx.md`.

## Alternatives considered

- **Minimal "one number, one action" Home.** Rejected as the lead: it serves Sam (pragmatist) and
  starves Alex (evaluator) and Priya (power user), who need teaching and depth respectively. The
  minimal top survives as beat one inside the guided story, with the rest progressively disclosed
  below it.
- **Calm cockpit (three cards).** Rejected: better than today but still leads with metrics over
  meaning, so it doesn't move the comprehension needle for a first-run user.
- **Keep the dark palette, just mute it.** Rejected: muting reduces the symptom but "reads like a
  document" is the goal, and light paper gets there more honestly than a desaturated panel.
- **Rewrite all eight views at once.** Rejected: too large to review or react to, and it would set the
  design system and stress-test it in the same uncontrolled step. Home first lets the system prove out
  on one screen before it propagates.
- **Redesign by feel, ship when it looks done.** Rejected: the product's whole discipline is
  earn-it-with-evidence. The presentation layer gets the same gate. fitcheck is that gate.

## Consequences

- A new shared design system (clean-light tokens, type scale, motion primitives) is established by
  Home and inherited by later passes. Until then, old and new views coexist under the same nav.
- The `fitcheck` skill becomes a standing part of the product loop, not a one-off. Persona and rubric
  drift must be maintained in `docs/personas-ctx.md` and `.claude/skills/fitcheck/rubric.md`.
- The eight-tab nav is slated to collapse to four groups (Overview, Bill, Tools, Settings) as views
  are restyled, not in pass one. Tab ids stay stable where a view is moving rather than being rebuilt.
- Motion is now a design element with a rule: every animation marks a real event (arrival, a value,
  progress) and all of it respects `prefers-reduced-motion`.
- One cumulative "reclaimed" number, defined once as output plus input, appears on Home and is
  identical in every beat. This closes the "three different saved figures" problem for the customer
  surface.
- Dark mode is deferred but kept possible: the token structure allows a `prefers-color-scheme` theme
  later without a rebuild.
