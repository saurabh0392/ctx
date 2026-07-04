# fitcheck: live Save surface (pass 3)

- Date: 2026-07-04
- Target: `http://127.0.0.1:8789/#save`, Tool report + Surfaces + Compaction merged, built into
  `src/dashboard.html`, on live data
- Compare: the old three pages (Tool report, Surfaces, Compaction), dark, separate tabs
- Evidence: rendered screenshot `save-live.png`, DOM dump confirming real tool cards and no console
  errors
- Rubric version: 2026-07-04

## Headline

- **Overall: 4.4 / 5** &nbsp; **Coherence: 5 / 5** &nbsp; **Verdict: Ship**
- Three dark pages are now one clean-light proof page. The ladder and harm read anchor it, each earned
  tool carries its real narrative and arms evidence, and the deletions (stat strip, sparkline,
  suspected-cost section, compaction grid, correlation footnote) are gone. The per-tool verdicts read
  exactly as the old Tool report because the same narrative helpers are reused, just restyled.

## Persona scores

| Persona | Score | One-line read |
|---|---|---|
| Sam (pragmatist)   | 4.0 | The ladder and "458 trimmed, 0 corrections" line are his whole read; he does not open a card. |
| Priya (power user) | 4.5 | Per-tool arms, deltas, and the put-on-trial / stop-the-trial control are all here. |
| Marcus (skeptic)   | 4.7 | His page. Harm read up front, real n and deltas per tool, honest "collecting proof", nothing dressed up. |
| Alex (first-run)   | 4.2 | The lede teaches the earn-it idea; every section has a built empty state. |
| Jordan (budget)    | 4.2 | Earned count and the zero-harm line give him a defensible "it is working and safe". |

## Dimension grid

| Dimension | Sam | Priya | Marcus | Alex | Jordan |
|---|---|---|---|---|---|
| Comprehension    | 4.0 | 4.5 | 4.5 | 4.5 | 4.0 |
| Time-to-value    | 4.5 | 4.5 | n/a | 4.0 | 4.5 |
| Trust and safety | n/a | 4.5 | 5.0 | 4.0 | 4.5 |
| Cognitive load   | 4.0 | 4.5 | 4.0 | 4.0 | 4.5 |
| Action clarity   | 4.0 | 4.5 | 4.5 | 4.0 | 4.0 |
| Journey coherence| 4.5 | 4.5 | 5.0 | 4.5 | 4.5 |
| Delight          | 4.0 | 4.5 | 4.0 | 4.5 | 4.0 |

## Walkthroughs

- **Sam:** reads the ladder (3 earned, 1 on trial) and "458 trimmed results checked, 0 corrections",
  concludes it is working and not breaking anything, and leaves. The cards below are there if he ever
  wants them.
- **Priya:** opens Read's evidence to see 37 trimmed against 194 left whole and the re-read delta,
  then stops TodoWrite's trial with one click. Full detail and real control.
- **Marcus:** the harm read leads, each earned tool shows its runs and its re-read delta with a
  "trimmed vs left whole" callout, and TodoWrite honestly says it does not have enough proof yet. The
  suspected-cost and correlation-footnote noise is gone, which he reads as the page not padding its
  case.
- **Alex (empty state):** the lede explains the earn-it idea before any number, and each section
  degrades to a built "nothing yet" rather than a broken zero.
- **Jordan:** the earned count plus the zero-correction harm line is the "safe and working" summary he
  can repeat.

## Friction, most costly first

1. **Densest of the three light surfaces** (Sam; cognitive load). By nature, it is the audit page.
   The ladder and harm read up top keep his read short.
2. **Evidence deltas can look odd out of context** (Marcus, minor). Read shows a large negative
   re-read delta (about -36 pts). It is real and honest (trimming did not raise re-reads), but a one
   line note on why a big negative is good would help. Worth a copy pass.
3. **Empty states built, not visually verified** (Alex; coherence). Same standing check as Home and
   See: confirm on a fresh or emptied DB.
4. **Evidence expand confirmed via DOM, not a screenshot** (verification). No console errors and the
   arms markup is present; worth one human click-through.

## Coherence check (the four tensions)

- Sam vs Priya: **resolved.** Ladder and harm read for Sam, expandable per-tool evidence and controls
  for Priya.
- Marcus vs Sam: **resolved.** The proof is the whole page for Marcus and summarized in two lines for
  Sam.
- Alex vs everyone: **resolved in code.** Each section teaches when empty; verify the visual.
- Jordan vs history (one framing): **resolved.** One harm read, one earned count, no competing
  numbers.

## Verdict and next move

Ship. Pass 3 clears the bar and completes the See / Save pair under the clean-light system. Remaining
across all passes: verify the empty states on a fresh DB, and the optional nav tidy (Activity folding
into Settings) to reach the four-group target.
