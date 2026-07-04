# fitcheck: live Home (pass 1, shipped to src/dashboard.html)

- Date: 2026-07-04
- Target: `http://127.0.0.1:8789/#home`, the guided-story Home built into `src/dashboard.html`, on live data
- Compare: `docs/fitcheck/2026-07-04-current-dashboard-home.md` (baseline 2.9 / coherence 2) and
  `docs/fitcheck/2026-07-04-home-2026-prototype.md` (prototype 4.3 / coherence 4)
- Evidence: rendered screenshots `/tmp/ob-shots/home-beat1.png` (count-up mid-flight, rail lit) and
  `/tmp/ob-shots/home-reduced.png` (all three beats, live counts)
- Rubric version: 2026-07-04

## Headline

- **Overall: 4.4 / 5** &nbsp; **Coherence: 5 / 5** &nbsp; **Verdict: Ship**
- **Up +1.5** vs baseline, **+0.1** vs prototype, and the prototype's one open gate is closed: the
  empty and watching states are now built in `loadHome()`, and the number is real, not illustrative.
  One remaining verification, not a design gap: screenshot the empty state on a fresh install.

## Persona scores

| Persona | Score | vs baseline | vs prototype | One-line read |
|---|---|---|---|---|
| Sam (pragmatist)   | 4.7 | +2.3 | -0.1 | Beat one is one sentence, one live number (6.0M), one line, one button. Answered and gone. |
| Priya (power user) | 4.2 | +0.7 | +0.2 | The light top nav is right there, so the bill and tool report are one click, plus the live ladder. |
| Marcus (skeptic)   | 4.4 | +1.0 | +0.1 | Real counts now (Watching 6, Trial 1, Proving 0, Earned 3), reversibility beside the mechanism. |
| Alex (first-run)   | 4.4 | +1.9 | 0.0 | The narrative teaches, and the empty state is now built (unverified visually on this data-rich box). |
| Jordan (budget)    | 4.4 | +1.5 | +0.2 | 6.0M in beat one, the same 6.0M restated in beat three, drawn from the real endpoints. |

## Dimension grid

| Dimension | Sam | Priya | Marcus | Alex | Jordan |
|---|---|---|---|---|---|
| Comprehension    | 4.5 | 4.0 | 4.0 | 4.5 | 4.0 |
| Time-to-value    | 5.0 | 4.0 | n/a | 4.0 | 4.5 |
| Trust and safety | n/a | 4.0 | 4.5 | 4.0 | 4.5 |
| Cognitive load   | 5.0 | 4.5 | 4.0 | 4.5 | 4.5 |
| Action clarity   | 4.5 | 4.5 | 4.0 | 4.0 | 4.0 |
| Journey coherence| 5.0 | 4.5 | 5.0 | 4.5 | 5.0 |
| Delight          | 4.5 | 4.5 | 4.0 | 4.5 | 4.0 |

## Walkthroughs

- **Sam:** lands on paper, one serif line, a big emerald 6.0M counting up, "Autopilot is trimming 3
  tools, nothing here needs a decision from you". Under ten seconds to his answer. The quiet top nav
  is there if he ever wants it and ignorable if he doesn't.
- **Priya:** the calm doesn't hide the depth from her the way the prototype nearly did. The light nav
  exposes Context bill, Tool tax, and Tool report immediately, and beat two's ladder shows the real
  distribution of her tools across the four stages. Her depth is one click, visibly.
- **Marcus:** beat two now shows his actual numbers, not mock ones: 6 watching, 1 on trial, 0 proving,
  3 earned, with the reversibility line sitting right next to the earn-it explanation. He trusts real
  counts more than illustrative ones. He still needs the intervals when he clicks a stage, which live
  in the Tool report view.
- **Alex (empty state):** the See/Save/Trust scroll still teaches what ctx is. The cold-start and
  watching states are now implemented in code (a warming-up headline instead of a number, honest copy,
  no `n/a`). They have not been screenshotted on a dataless machine yet, which is the one open check.
- **Jordan:** she reads 6.0M in beat one and the exact same 6.0M in beat three's restate line, both
  pulled from `total_reclaimed_chars` plus `total_reclaimed_tokens`. One number, consistent, real.

## Friction, most costly first

1. **Empty state unverified** (Alex; coherence). Built but not visually confirmed. Point a fresh or
   emptied DB at the dashboard and screenshot the cold Home before calling this fully done.
2. **Nav is the old eight items** (Sam, mild; cognitive load). Styled light and quiet, but the
   eight-tab bar is still the interim IA. The planned collapse to four groups happens in a later pass.
3. **Ladder detail is not yet clickable** (Priya, Marcus; action clarity). The rungs show counts but
   don't yet expand into per-tool evidence in place; depth still means a trip to the Tool report view.
4. **Motion unverified under real scroll** (all, minor). Confirmed via headless (count-up caught
   mid-flight, rail node lit, ladder bars drawn), but a human scroll-through is worth one look.

## Coherence check (the four tensions)

- Sam vs Priya: **resolved.** Calm hero for Sam, visible nav plus live ladder for Priya.
- Marcus vs Sam: **resolved.** Trust story in beat two, quiet, next to the mechanism, now with real n.
- Alex vs everyone: **resolved in code.** Empty and watching states are built and honest; verify the
  visual on a fresh install.
- Jordan vs history (one number): **resolved.** One live figure, identical in both beats it appears.

## Verdict and next move

Ship. This clears the bar the baseline set and matches the prototype while closing its one gate. The
single most useful next check is the fresh-install empty state screenshot; after that, pass 2 is the
See surface (Context bill plus Tool tax) restyled into the same clean-light system.
