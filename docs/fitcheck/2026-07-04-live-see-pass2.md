# fitcheck: live See surface (pass 2)

- Date: 2026-07-04
- Target: `http://127.0.0.1:8789/#see`, the merged Context bill + Tool tax, built into `src/dashboard.html`, on live data
- Compare: the old two pages (Context bill + Tool tax), dark, separate tabs
- Evidence: rendered screenshots `see-final.png` (band + capped output + input), DOM dump confirming
  real used-tool chips and no console errors
- Rubric version: 2026-07-04

## Headline

- **Overall: 4.4 / 5** &nbsp; **Coherence: 5 / 5** &nbsp; **Verdict: Ship**
- Two dark, separate tabs are now one clean-light itemized ledger. The two-tax split is the anchor,
  the output list is capped to the top 8 with the tail summarized, and each server expands to the
  tools you actually use. The Input card unit bug is fixed (all per-request), and the dead-tool gap
  is handled honestly rather than faked.

## Persona scores

| Persona | Score | One-line read |
|---|---|---|
| Sam (pragmatist)   | 4.0 | The two-tax band is his whole read; he does not scroll the ledger, and does not need to. |
| Priya (power user) | 4.6 | Her page. Itemized both taxes, expand to sources, trim diffs, and the tools she actually uses, plus a real reversible prune. |
| Marcus (skeptic)   | 4.6 | Harm read (0 tool-misses) gates the prune, reclaimable is held apart from reclaimed, and the dead-tool limit is stated plainly, not papered over. |
| Alex (first-run)   | 4.2 | The lede teaches the two taxes before any number; empty states are built for both sections. |
| Jordan (budget)    | 4.3 | One clear figure per tax, each in a single labelled unit. The per-request vs cumulative mix that confused before is gone. |

## Dimension grid

| Dimension | Sam | Priya | Marcus | Alex | Jordan |
|---|---|---|---|---|---|
| Comprehension    | 4.0 | 4.5 | 4.5 | 4.5 | 4.5 |
| Time-to-value    | 4.5 | 4.5 | n/a | 4.0 | 4.5 |
| Trust and safety | n/a | 4.5 | 5.0 | 4.0 | 4.5 |
| Cognitive load   | 4.0 | 4.5 | 4.0 | 4.0 | 4.5 |
| Action clarity   | 4.0 | 4.5 | 4.0 | 4.0 | 4.0 |
| Journey coherence| 4.5 | 4.5 | 5.0 | 4.5 | 5.0 |
| Delight          | 4.0 | 4.5 | 4.0 | 4.5 | 4.0 |

## Walkthroughs

- **Sam:** lands on "Your context pays two taxes", reads 47M output and 77K per-request input in the
  band, gets the gist, and moves on. The capped output list means even if he glances down it is eight
  rows, not twenty.
- **Priya:** opens Read (92% trimmable) to see the top source files and an actual trim diff, then
  opens Linear to see the eleven tools she uses versus the count she does not, and prunes the 36 dead
  ones with one reversible click. This is the transparency she wanted.
- **Marcus:** reads "0 tool-misses in 30 days, so a prune has nothing to walk back", notes reclaimable
  (10M) is not dressed up as reclaimed (4.8M), and reads the dead-tool note that says ctx will not
  list names it never captured. Honest limitation is exactly what earns him.
- **Alex (empty state):** the two-tax lede teaches the concept first; both sections have a built
  warming-up state instead of a broken-looking zero.
- **Jordan:** each tax shows one number in one unit, output cumulative and input per-request, each
  labelled, so nothing contradicts. The earlier per-request-then-2M-cumulative confusion is gone.

## Friction, most costly first

1. **Dead-tool names and descriptions are not shown** (Priya, Marcus; comprehension). Honest, because
   ctx never stored the full menu, and the UI says so. To list every removed tool with its vendor
   description, ctx would need to capture the tool catalog at hook time. Proposed as a follow-up, not
   a bug.
2. **Denser than Home** (Sam; cognitive load). By nature: this is the working surface, not the
   landing. The band up top keeps his read short, which is the mitigation.
3. **Empty states built, not visually verified** (Alex; coherence). Same open check as Home: confirm
   on a fresh or emptied DB.
4. **Input-row expand not captured in a screenshot** (verification). Confirmed via DOM (real used-tool
   chips, no errors), worth one human click-through.

## Coherence check (the four tensions)

- Sam vs Priya: **resolved.** Band for Sam, expandable ledger and prune for Priya.
- Marcus vs Sam: **resolved.** The harm read and honest limits are there for Marcus, quiet for Sam.
- Alex vs everyone: **resolved in code.** Both sections teach when empty; verify the visual.
- Jordan vs history (one number, one unit): **resolved.** The unit mix is fixed; each tax is
  internally consistent and labelled.

## Verdict and next move

Ship. Pass 2 clears the bar. The most valuable follow-up is the tool-catalog capture, which would let
the input expand list every removed tool with its description, turning the current honest gap into a
full answer.
