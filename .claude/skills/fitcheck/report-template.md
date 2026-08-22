# fitcheck report template

Fill this in. Save to `docs/fitcheck/<yyyy-mm-dd>-<target-slug>.md`. Give the user the headline block
in chat; the file is the record.

---

# fitcheck: <target> (<scope>)

- Date: <yyyy-mm-dd>
- Target: <file path / URL / screenshot>
- Compare: <prior target or report, or "none">
- Evidence: <the rendered screenshots you actually read, or "source only, render not verified">
- Rubric version: 2026-08-21

## Headline

- **Overall: <x.x> / 5** &nbsp; **Coherence: <x> / 5** &nbsp; **Verdict: <Ship | Iterate | Rework>**
- One sentence on the state of it.
- If compared: **<up/down> <delta>** vs <prior>, biggest mover <persona/dimension>.

## Persona scores

| Persona | Score | vs prior | One-line read |
|---|---|---|---|
| Sam (pragmatist)   | x.x | +/- | ... |
| Priya (power user) | x.x | +/- | ... |
| Marcus (skeptic)   | x.x | +/- | ... |
| Alex (first-run)   | x.x | +/- | ... |
| Jordan (budget)    | x.x | +/- | ... |

## Dimension grid

| Dimension | Sam | Priya | Marcus | Alex | Jordan |
|---|---|---|---|---|---|
| Comprehension    | | | | | |
| Time-to-value    | | | | | |
| Trust and safety | | | | | |
| Cognitive load   | | | | | |
| Action clarity   | | | | | |
| Journey coherence| | | | | |
| Visual execution | | | | | |
| Delight          | | | | | |

## Walkthroughs

Short, in-voice, patience-window-bounded. What they see, do, and where they stall. Ground every
claim in the actual screen.

- **Sam:** ...
- **Priya:** ...
- **Marcus:** ...
- **Alex (empty state):** ...
- **Jordan:** ...

## Friction, most costly first

1. **<title>** (persona, dimension). What breaks, and the specific fix.
2. ...

## Coherence check (the four tensions)

- Sam vs Priya: <resolved / broken, why>
- Marcus vs Sam: <resolved / broken, why>
- Alex vs everyone: <resolved / broken, why>
- Jordan vs history (one number): <resolved / broken, why>

## Verdict and next move

The band, and the single most important thing to change before the next run.
