---
name: fitcheck
description: Estimate the usage experience of any version of the ctx dashboard by role-playing ctx's five customer personas through it and scoring the result, so the product journey can be coherence-checked version over version. Use when the user asks to fitcheck a design, "run fitcheck", check whether a dashboard/screen/prototype works for users, compare two versions of the UI, gut-check a redesign against real personas, or asks "how does this land for alpha users". Targets can be a local HTML file (src/dashboard.html or a prototype), a URL (the running dashboard or an Artifact), or a screenshot image.
---

# fitcheck: persona-based coherence check for the ctx dashboard

Estimate how a real ctx customer experiences a given version of the product, per persona, and turn
that into a comparable score. The point is not a vanity grade. It is a repeatable coherence gate:
run it on every version, keep the number honest, and only ship a change when it beats the version
before it with no persona left behind. This is the presentation-layer equivalent of ctx's own
earn-it discipline.

You are role-playing five specific people, not reviewing the UI in the abstract. A screen can be a 5
for one of them and a 2 for another. Hold that tension; it is the whole value.

## Inputs (read the args)

- **target** (required): what to evaluate. One of:
  - a local file path (`src/dashboard.html`, `docs/prototypes/home-2026.html`)
  - a URL (`http://127.0.0.1:8789` for the live dashboard, or an Artifact link)
  - a screenshot image path
- **compare** (optional): a second target to diff against, or a prior report file under
  `docs/fitcheck/`. When present, produce the version-over-version delta, not just an absolute score.
- **scope** (optional): which screen(s). Default is Home. Accepts a view id (`home`, `bill`,
  `toolbill`, `loop`, `compaction`, `surfaces`, `activity`, `settings`) or `all`.
- **mode** (optional): `full` (default, all five personas, full report) or `brief` (headline score,
  per-persona one-liner, the single biggest fix).

No args means: render the live dashboard, scope all, full mode.

## Procedure

1. **Look at the rendered page first.** Source is not the target. `src/dashboard.html` shows what
   the author meant; only the built DOM shows what the user gets, and the gap between them is where
   the worst defects live (a row and the card inside it both rendering a header, a button sitting
   loose in a paragraph, five different left edges). Render before you score:

   ```
   node scripts/coherence/shoot.mjs http://127.0.0.1:<port> <outDir> home save see settings
   ```

   `scripts/coherence/fitcheck-local.sh` does this for you and hands you the PNGs. Read every image
   with the Read tool. Then read `src/dashboard.html` for structure, copy, and the empty states the
   screenshots cannot show. If you were handed only a file path or only a screenshot, say so in the
   report's Evidence line and cap Visual execution at 3, because you could not verify the render.
   Always evaluate at least two states of any data-driven screen: cold/empty (Alex's first run) and
   populated.

2. **Load the yardsticks.** Read `docs/personas-ctx.md` (canonical persona detail) and
   `rubric.md` in this skill folder (dimensions, 1 to 5 anchors, per-persona weights, verdict bands).
   The compact persona table below is enough to start; the doc has the depth.

3. **Walk each persona through it.** For each of the five, write a short first-impression walkthrough
   in their voice and patience window: what they see first, what they do, where they stall or bounce.
   Ground every claim in something actually on the screen (a specific number, label, color, motion,
   or its absence). Do not invent UI that isn't there.

4. **Score.** Rate each of the eight dimensions 1 to 5 for each persona using the rubric anchors.
   Compute each persona's weighted score with that persona's weights. Then the overall (mean of the
   five) and the coherence score (rubric defines it: are the four known tensions resolved or is one
   persona served by starving another).

5. **Name the friction.** List the concrete friction points, most costly first, each tied to a
   persona and a dimension, each with a specific fix. This is the payload the user acts on.

6. **Verdict.** Ship / Iterate / Rework, per the rubric bands. Be willing to say Rework.

7. **Diff (if compare given).** Show each persona's and each dimension's movement versus the prior
   version. Flag any regression loudly; a regression on any persona blocks a ship even if the overall
   rose.

8. **Write the report.** Follow `report-template.md`. Save it to
   `docs/fitcheck/<yyyy-mm-dd>-<target-slug>.md` so the journey has a paper trail, and give the user
   the headline in chat.

## The five personas (compact; full detail in docs/personas-ctx.md)

- **Sam, ship-it pragmatist.** ~10s patience. Wants one number and "nothing to do". Bounces on
  jargon, multiple actions, walls of text.
- **Priya, connector maximalist.** Minutes of patience if depth is real. Wants the itemized bill,
  per-tool proof, real controls. Bounces on toys and hand-waving.
- **Marcus, trust-but-verify skeptic.** Adversarial. Wants the fail-closed story and the undo path,
  with real n behind claims. Bounces on casual irreversible actions and unbacked confidence.
- **Alex, first-run evaluator.** ~30s of goodwill on the empty state. Wants to understand what ctx is
  and whether to keep it. Bounces on `n/a`, numbers with no story, empty states that look broken.
- **Jordan, budget watcher.** Goal-directed. Wants one consistent payoff figure and the weekly trend.
  Bounces the instant two "saved" numbers disagree.

## Rules

- **Cite the screen.** Every score and friction point references something concrete in the target.
  "Feels cluttered" is useless; "beat one stacks five numbers before any of them is explained" is a
  finding.
- **Score the empty state too.** Alex lives there. A screen that only works when full has already
  failed its most important persona.
- **Honesty over flattery.** This gate exists to catch problems. A soft score that lets a weak
  version ship defeats the purpose. Reward honest limitation in the UI; penalize confident overclaim.
- **One number, one name.** Journey-coherence hunts both "figures disagree" and "the same state is
  called two things". If one concept shows two values, or one state shows two labels, that is a hard
  hit, not a nitpick.
- **Score the render, not the intent.** Visual execution comes from the screenshots. If you did not
  look at an image of the screen, you cannot score it, and you must say so.
- **No em dashes** and none of the humanizer avoid-list words in the report (workspace rule).
- **Keep it comparable.** Use the same rubric and weights every run so scores mean something over
  time. If the rubric or personas change, note it in the report so a score jump isn't misread as a
  design change.
