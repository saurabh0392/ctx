# fitcheck: current dashboard, Home (baseline)

- Date: 2026-07-04
- Target: `src/dashboard.html`, view `home` (shipped version)
- Compare: none (this is the baseline the redesign must beat)
- Rubric version: 2026-07-04

## Headline

- **Overall: 2.9 / 5** &nbsp; **Coherence: 2 / 5** &nbsp; **Verdict: Rework**
- The substance is all here and honest, but Home leads with density. It serves the two personas who
  tolerate depth (Priya, Marcus) and loses the three who don't (Sam, Alex, Jordan), which is exactly
  the wrong split for an alpha where first-run and pragmatist users decide keep-or-uninstall.

## Persona scores

| Persona | Score | One-line read |
|---|---|---|
| Sam (pragmatist)   | 2.4 | A headline number exists, then a both-taxes console and a WNAD scoreboard bury it. Bounces. |
| Priya (power user) | 3.5 | Happy: real depth, taxes, conditions, links into the bill and report. |
| Marcus (skeptic)   | 3.4 | Earn-it gate and harm numbers are visible, but the trust story is loud and multiple figures make him check consistency. |
| Alex (first-run)   | 2.5 | Lands on a dark, jargon-dense screen; the See/Save/Trust that would teach him is a card near the bottom. |
| Jordan (budget)    | 2.9 | Sees a savings card, a taxes number, and a WNAD number and can't tell which is "the" figure. |

## Dimension grid

| Dimension | Sam | Priya | Marcus | Alex | Jordan |
|---|---|---|---|---|---|
| Comprehension    | 3.0 | 4.0 | 3.5 | 3.0 | 3.5 |
| Time-to-value    | 2.5 | 3.0 | n/a | 2.5 | 3.0 |
| Trust and safety | n/a | 3.5 | 4.0 | 2.5 | 3.0 |
| Cognitive load   | 2.0 | 3.0 | 2.5 | 2.0 | 2.5 |
| Action clarity   | 2.5 | 3.5 | 3.0 | 2.5 | 3.0 |
| Journey coherence| 3.0 | 3.0 | 3.0 | 3.0 | 2.5 |
| Delight          | 2.0 | 2.5 | 2.0 | 2.0 | 2.0 |

## Walkthroughs

- **Sam:** reads the one-line headline (good), sees a savings number (good), then hits the both-taxes
  console with two more big numbers and rows, then the WNAD block with a verdict, a 2x2 grid, and a
  conditions grid. Three screens of numbers, words like "tax" and "net ahead" and "harm". He came for
  "am I better off, yes or no". He closes the tab. ctx keeps working; the dashboard lost him.
- **Priya:** this is her kind of page. She reads the taxes breakdown, notices the harm figure, follows
  the link into the Context bill. The density is a feature for her. Her only complaint is that it took
  a beat to find where the depth started.
- **Marcus:** finds the earn-it conditions and the harm numbers, which he respects. But he counts a
  savings figure, a reclaimed figure in the taxes console, and a reclaimed figure in WNAD, and spends
  his attention checking whether they agree instead of trusting them. Loud where it should be calm.
- **Alex (empty state):** first run, little data. The status reads "Getting your numbers", the console
  and scoreboard are near-empty, and the three-pillar explainer that would tell him what ctx even is
  sits below all of it. His 30 seconds of goodwill run out before he reaches the part that teaches.
- **Jordan:** wants one number to repeat to a teammate. The page offers several, each framed slightly
  differently. She can't tell which is the honest cumulative figure, so she trusts none of them.

## Friction, most costly first

1. **No calm entry point** (Sam, Alex; cognitive load). Home turns everything on at once. There is no
   single first thing to land on. Fix: one hero number and one line, everything else below or behind.
2. **The teaching is at the bottom** (Alex; comprehension). See/Save/Trust is the product story and
   it's the last thing on the page. Fix: make it the structure of the page, not a footer card.
3. **Several "saved" numbers** (Jordan, Marcus; journey coherence). Savings card, taxes console, WNAD
   each show a reclaimed figure. Fix: one cumulative number, defined once, repeated identically.
4. **Palette overload** (all; delight, load). Green, amber, red, four text greys, gradient cards, and
   colored card borders compete. Fix: one accent, hairlines over fills.
5. **Static** (all; delight). One pulsing dot and a fade. Nothing marks the continuous work ctx does.

## Coherence check (the four tensions)

- Sam vs Priya: **broken.** Depth exists but there is no calm top, so Priya is served by starving Sam.
- Marcus vs Sam: **partial.** The trust story is findable but loud, so it costs Sam attention.
- Alex vs everyone: **broken.** The screen informs when full and does not teach when empty.
- Jordan vs history (one number): **broken.** Multiple reclaimed figures on one page.

## Verdict and next move

Rework, as expected. This baseline is the bar. The single most important change is the one the
redesign is built around: replace the all-on instrument panel with a calm, teaching, one-number entry
and push depth below and behind it.
