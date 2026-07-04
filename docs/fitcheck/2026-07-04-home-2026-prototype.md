# fitcheck: Home 2026 prototype

- Date: 2026-07-04
- Target: `docs/prototypes/home-2026.html` (guided story, clean light)
- Compare: `docs/fitcheck/2026-07-04-current-dashboard-home.md` (baseline, 2.9 / coherence 2)
- Rubric version: 2026-07-04

## Headline

- **Overall: 4.3 / 5** &nbsp; **Coherence: 4 / 5** &nbsp; **Verdict: Ship (with one gate)**
- **Up +1.4** vs baseline, coherence **+2**. Biggest movers are exactly the personas the redesign
  targeted: Sam +2.4 and Alex +1.9. No persona regressed. The one gate before a real ship: the empty
  state is specified but not built in the prototype, and Alex lives there, so it must be built and
  re-checked before this replaces the live Home.

## Persona scores

| Persona | Score | vs prior | One-line read |
|---|---|---|---|
| Sam (pragmatist)   | 4.8 | +2.4 | Beat one is one sentence, one number, one line, one button. He gets his answer and leaves happy. |
| Priya (power user) | 4.0 | +0.5 | Calm top, but her depth now sits behind the ladder and footer links. One click away, not on the surface. |
| Marcus (skeptic)   | 4.3 | +0.9 | The earn-it ladder and reversibility sit right next to the mechanism, quiet. Wants the sample sizes when he clicks in. |
| Alex (first-run)   | 4.4 | +1.9 | The See/Save/Trust narrative teaches what ctx is as he scrolls. Held back only by the unbuilt empty state. |
| Jordan (budget)    | 4.2 | +1.3 | One number, 1.24M, stated once and restated identically. Net-ahead now in concrete tokens, not "room". |

## Dimension grid

| Dimension | Sam | Priya | Marcus | Alex | Jordan |
|---|---|---|---|---|---|
| Comprehension    | 4.5 | 4.0 | 4.0 | 4.5 | 4.0 |
| Time-to-value    | 5.0 | 3.5 | n/a | 4.0 | 4.0 |
| Trust and safety | n/a | 4.0 | 4.5 | 4.0 | 4.0 |
| Cognitive load   | 5.0 | 4.5 | 4.0 | 4.5 | 4.5 |
| Action clarity   | 4.5 | 4.0 | 4.0 | 4.0 | 3.5 |
| Journey coherence| 4.5 | 4.5 | 4.5 | 4.5 | 4.5 |
| Delight          | 4.5 | 4.5 | 4.0 | 4.5 | 4.0 |

## Walkthroughs

- **Sam:** lands on "ctx makes your agent leaner without losing the thread", one big green number
  counting up, one line saying autopilot is handling 3 tools and there is nothing to do. That is his
  entire question answered in under ten seconds. He does not scroll, and that is a success.
- **Priya:** appreciates the calm but goes looking for depth. She finds the ladder (Watching, Trial,
  Proving, Earned) and the footer links into the bill and report. Her score holds rather than jumps
  because the per-tool detail is now behind a click instead of on the surface; correct for a Home, but
  she notices.
- **Marcus:** reads beat two, sees trimming is gated on his own sessions not re-reading or re-editing,
  and the reversibility line sits right beside it, quiet. He trusts it more than the loud version. He
  will click in for the actual n and intervals, which must be there when he does.
- **Alex (empty state):** the narrative teaches him what ctx is as he scrolls, which is the win. The
  gate: the prototype shows populated data. The specified warming-up empty state is not built, so his
  real first run is unverified. This is the one thing standing between the prototype and a ship.
- **Jordan:** sees 1.24M reclaimed in beat one, the same 1.24M restated in beat three, and a net-ahead
  figure now in tokens after the re-read cost is paid back. One number, consistent, defensible.

## Friction, most costly first

1. **Empty state unbuilt** (Alex; comprehension, coherence). The spec has it; the prototype doesn't.
   Build the warming-up beat-one and verify a cold Home before this replaces the live one.
2. **Priya's depth is all behind clicks** (Priya; action clarity). Make the ladder nodes visibly
   interactive so the promise of depth reads on the surface, not just when she guesses to click.
3. **Marcus needs the n on click-in** (Marcus; trust). When he opens a ladder stage, the real sample
   sizes and intervals must be one layer down, or the calm surface reads as hiding them.
4. **Numbers are illustrative** (all; coherence). Wire the hero, ladder, and net-ahead to the real
   endpoints so the consistency proven here holds on live data, not just in the mockup.

## Coherence check (the four tensions)

- Sam vs Priya: **resolved.** Calm beat one for Sam, ladder and footer depth for Priya.
- Marcus vs Sam: **resolved.** The trust story is in beat two, quiet, next to the mechanism.
- Alex vs everyone: **partial.** The narrative teaches, but the empty state that is his real first run
  is not yet built. This is why coherence is 4, not 5.
- Jordan vs history (one number): **resolved.** One cumulative figure, identical in both beats it
  appears, net-ahead in concrete tokens.

## Verdict and next move

Ship the direction. The single gate before it replaces the live Home: build and re-fitcheck the empty
state, since Alex is the persona that decides whether an alpha user keeps ctx at all.
