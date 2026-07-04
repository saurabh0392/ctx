# Dashboard redesign 2026: guided story, clean light

Status: draft for build
Date: 2026-07-04
Owner: Saurabh Sharan
Companions: `docs/personas-ctx.md` (who this is for), `docs/adr/0036-dashboard-redesign-guided-story-clean-light.md`
(the decision), `docs/prototypes/home-2026.html` (the visual spec), `.claude/skills/fitcheck/` (the
coherence gate). Supersedes the visual layer of ADR 0003's IA, keeps its information architecture
intent.

## The one paragraph

The dashboard works and reads like an instrument panel: eight top-nav destinations, a dark canvas
running green, amber, red, and four greys of text at once, stat strips and before/after grids
stacking numbers three deep, and almost no motion. It is dense the way a thing built by adding one
honest metric at a time gets dense. For the person who built it, every number is load-bearing. For
an alpha user landing cold, it is a wall. This redesign trades the panel for a story. The new Home
is a clean, light, scroll-driven narrative in three beats (See where your context goes, Save only
what's safe, Trust that it's local and reversible), each beat animating in as you reach it, leading
with one number and one status and pushing every other figure down or behind a click. We build Home
first as the new design system, prove it beats the current dashboard with the `fitcheck` skill
across all five personas, then roll the same language across the other seven views. Nothing about
the data model or the safety discipline changes. This is a presentation-layer rebuild, gated by
evidence, exactly like everything else ctx ships.

## Why now

ctx is going to alpha users. The current dashboard was built for an audience of one (the author),
who knows what every metric means and why it earned its place. An alpha user is Alex from the
persona doc: 30 seconds of goodwill, no concept of what ctx is, deciding keep-or-uninstall. The
product's substance is strong and the presentation is the thing standing between that substance and
a first impression that lands. This is the cheapest high-leverage work left before handing it to a
friend.

## The diagnosis (what's actually wrong)

Read against `src/dashboard.html` as it stands:

- **Too many destinations.** Eight top-nav tabs (Home, Context bill, Tool tax, Tool report,
  Compaction, Surfaces, Activity, Settings). Sam and Alex can't tell which one answers their
  question, so they pick none.
- **Too many numbers per screen.** Home alone stacks a status line, a savings figure, the both-taxes
  console (two big numbers plus per-tax rows), the WNAD scoreboard (verdict plus a 2x2 grid plus a
  conditions grid), and a three-pillar explainer. Every one is defensible. Together they are noise.
- **Too much color.** Green as accent is fine. Green plus amber plus red plus `--t1..--t4` plus
  gradient-filled savings cards plus colored top-borders on ladder cards means nothing stands out
  because everything is trying to.
- **Almost no motion.** One pulsing dot and a 0.25s view fade. Nothing guides the eye, nothing
  rewards scrolling, nothing feels alive. The product is doing continuous work in the background and
  the UI looks static.
- **The narrative is implicit.** See / Save / Trust is the actual product story and it's buried in a
  three-pillar card near the bottom of Home instead of being the structure of the page.

## The locked direction

Decided with the user on 2026-07-04:

- **Layout: guided story.** Home is a scroll-driven narrative, See then Save then Trust, each
  section animating in on scroll. Most motion, most teaching, best for a first-run alpha.
- **Palette: clean light.** White canvas, near-black ink, one accent. Reads like a document, not a
  control panel. The biggest departure from today and the most direct answer to "too much color."
- **Scope: nail Home first.** Rebuild Home end to end as the new design system (tokens, type, motion,
  spacing), ship it, prove it with fitcheck, then roll the language across the other seven views.

## The new Home, beat by beat

The page is three full-height-ish beats plus a quiet header and footer. Each beat has one job and
one dominant element. Data bindings map to existing endpoints; nothing new is required server-side.

### Header (quiet, persistent)

`ctx` wordmark left, a single live health pip and one-word status right (`safe`, `watching`,
`paused`). No nav tabs on first paint. The nav reveals as a slim secondary bar once the user scrolls
past beat one, or via a single menu affordance. Sam never needs it; Priya finds it the moment she
looks.

### Beat 1: See (the hook)

The first thing on the page. One sentence naming what ctx does, then the single number that matters:
cumulative tokens reclaimed, defined once as output-plus-input (the `total_reclaimed_tokens` +
output savings reconciliation from CTX-69, the "all reclaimed" decision). One supporting line of
plain language ("Autopilot is trimming N tools it proved safe on your own work"). One primary action
that scrolls or links onward ("See where it went").

- **Empty state (Alex):** the number is replaced by a warming-up line ("ctx is watching your
  sessions. The moment it can prove a trim is safe, it starts reclaiming room here.") No `n/a`, no
  zeros presented as failure.
- **Motion:** the number counts up from zero once on first paint (respecting
  `prefers-reduced-motion`). The health pip breathes.
- **Data:** `/api/context/tool-bill` total, WNAD reclaimed totals, home status.

### Beat 2: Save (the proof of discipline)

The See/Save/Trust middle beat, animating in on scroll. This is where the earn-it gate becomes a
picture instead of a paragraph. The four-stage ladder (Watching, Trial, Proving, Earned) rendered as
a calm horizontal progression, each stage a node that fills as tools move along it. One line of copy:
ctx only trims a tool after your own sessions show trimming it didn't make the agent re-read or
re-edit. At most two numbers here (tools earned, room saved), the rest is the visual.

- **For Priya:** the ladder nodes are clickable and expand into the per-tool detail and evidence that
  currently lives on the Tool report page. Depth on demand, not on landing.
- **For Marcus:** the reversibility line sits right here, next to the mechanism ("A trimmed detail
  still ran in full and is one command back"), not as a caveat wall up top.
- **Motion:** the ladder draws left to right as it enters the viewport; a tool "advancing a stage"
  animates when data updates.
- **Data:** the causal ladder counts (`causal_tool_outcomes`), tool stages.

### Beat 3: Trust (the close)

The reassurance beat. Local, no telemetry, reversible, neutral across agents. One consistent payoff
restatement (same number as beat 1, never a different one), the weekly net-ahead verdict as a single
calm line with a small trend, and the honest-limitation posture ctx is known for. This is the beat
that earns Marcus and gives Jordan her defensible figure.

- **Data:** WNAD verdict + trend, privacy facts (static), surfaces watched.

### Footer

Local-only reassurance, and the full nav as plain links for anyone who scrolled all the way and
wants to go deep. This is the on-ramp to the other seven views until they're redesigned.

## The clean-light design system

This is the part Home establishes and the other views inherit. Full token values live in the
prototype (`docs/prototypes/home-2026.html`); the intent:

- **Canvas:** white / near-white (`#fbfbfa` warm paper, not clinical `#fff`). Ink near-black
  (`#16181d`), not pure black. Three text weights max (ink, muted, faint), down from four-plus.
- **One accent.** A single green used only on the number that matters and the live pip. Amber and
  red are reserved for genuine warnings and never appear decoratively. This is the direct fix for
  "too much color."
- **Hairlines over fills.** Structure comes from thin 1px borders and whitespace, not filled colored
  cards and gradients. The gradient savings cards and colored top-borders go away.
- **Type scale.** One display size (the hero number), one heading, one body, one small/caption.
  Tabular numerals for every figure. Generous line-height and a narrower measure (roughly 60ch) so
  prose reads like prose.
- **Space.** Beats breathe. The current 1120px dense grid becomes a calmer single-column narrative
  with cards only where comparison genuinely needs them.
- **Motion primitives.** A small, shared set: fade-and-rise on scroll-in (IntersectionObserver), a
  one-time count-up for the hero number, the ladder draw-in, and the breathing pip. All gated behind
  `prefers-reduced-motion: reduce`. No motion for motion's sake; every animation marks a real event
  (arrival, a value, progress).
- **Dark mode later.** Ship clean-light first. The token structure keeps a dark theme possible via
  `prefers-color-scheme` without a rebuild, but it is out of scope for pass one.

## Information architecture (the nav future)

Home first does not touch the other views' internals, but the redesign's endpoint is a smaller nav.
The eight tabs collapse to four groups, to be applied when the views are restyled:

- **Overview** (today's Home).
- **Bill** (Context bill + Tool tax: the two taxes, one place, the See surface in depth).
- **Tools** (Tool report + Compaction + Surfaces: the Save/proof surfaces).
- **Settings** (Settings + Activity as a sub-view).

This is captured as intent, not built in pass one. ADR 0003's principle holds: reframe and regroup,
keep tab ids stable where a view is moving rather than being rebuilt, so deep links and wiring
survive.

## Rollout, gated by fitcheck

Each pass ships only when `fitcheck` shows it beats the version before it, across all five personas,
with no persona regressing.

1. **Pass 0 (this doc + prototype + skill).** Baseline the current dashboard with fitcheck. That
   score is the bar.
2. **Pass 1: Home.** Rebuild Home in the real `src/dashboard.html` from the prototype. Establish the
   clean-light tokens and motion primitives as shared CSS the other views will use. Gate: fitcheck
   Home beats baseline Home, Sam and Alex improve most, no persona regresses, Jordan's number is
   consistent across every beat.
3. **Pass 2: the See surface (Bill + Tool tax).** Restyle to the new system, collapse into one
   destination. Gate: Priya's depth score holds or rises while cognitive-load drops.
4. **Pass 3: the Save surface (Tool report + Compaction + Surfaces).** Same. Gate: Marcus's
   trust-and-safety score holds while density drops.
5. **Pass 4: Settings + Activity + final nav collapse to four groups.** Gate: overall fitcheck
   coherence score at its high, no persona below "iterate."

Between passes, the old and new views coexist under the same nav; nothing goes dark mid-rollout.

## Explicitly out of scope for pass one

- Any change to the data model, endpoints, or the safety/earn-it logic.
- Dark mode.
- The other seven views' internals (they inherit the system in later passes).
- New metrics. This pass removes and reorganizes; it does not invent numbers.

## Success criteria

- fitcheck overall score for Home clears the baseline, with the largest gains on Sam (time-to-value,
  cognitive-load) and Alex (comprehension, first-run).
- No persona regresses on any dimension versus baseline.
- Exactly one cumulative "reclaimed" number on Home, identical in every beat it appears (kills the
  three-figures bug for good).
- A cold, dataless Home reads as "warming up," never as broken.
- The user's own three complaints are visibly answered: motion is present and purposeful, numbers per
  screen are cut by more than half, and the palette is one accent on paper.
