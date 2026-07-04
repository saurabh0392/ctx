# fitcheck rubric

The fixed yardstick. Keep it stable so scores stay comparable across versions. If you change it,
say so in the report.

## The seven dimensions

Score each 1 to 5, per persona, grounded in what is actually on the screen.

1. **Comprehension.** Does this persona understand what ctx is and what this screen is telling them,
   within their patience window?
2. **Time-to-value.** How fast do they reach the specific thing they came for?
3. **Trust and safety.** Is reversibility, the earn-it gate, and honesty legible where it matters (and
   quiet where it doesn't)?
4. **Cognitive load.** Numbers, words, colors, choices. Calm and scannable, or a wall?
5. **Action clarity.** Do they know the one next thing to do, if any?
6. **Journey coherence.** Does the narrative hold without contradiction? One concept, one number,
   consistent framing from landing to goal. This dimension hunts the "figures disagree" failure.
7. **Delight.** Does it feel alive, considered, and worth keeping open, or static and cheap? Motion
   that marks real events counts here; motion for its own sake does not.

## Scoring anchors

- **1** Actively fails this persona. They bounce, misunderstand, or distrust.
- **2** Weak. They push through with friction and a worse impression.
- **3** Adequate. Gets the job done, nothing memorable, no real harm.
- **4** Strong. Serves this persona clearly and calmly.
- **5** Excellent. Feels built for them; removes friction they expected to hit.

Half points are allowed (3.5) when a dimension sits between anchors.

## Per-persona weights

Weighted average over the seven dimensions gives each persona's score. Weights are 0 to 3; 0 means
that dimension barely matters to this person.

| Persona | Compr | Time | Trust | Load | Action | Coher | Delight |
|---|---|---|---|---|---|---|---|
| Sam (pragmatist)     | 1 | 3 | 0 | 3 | 2 | 1 | 1 |
| Priya (power user)   | 3 | 1 | 2 | 0 | 3 | 1 | 1 |
| Marcus (skeptic)     | 2 | 0 | 3 | 1 | 1 | 3 | 0 |
| Alex (first-run)     | 3 | 1 | 1 | 2 | 1 | 2 | 2 |
| Jordan (budget)      | 2 | 1 | 2 | 1 | 1 | 3 | 0 |

Persona score = sum(weight × dimension score) / sum(weights), rounded to one decimal.

## Overall and coherence

- **Overall score** = mean of the five persona scores. This is the headline.
- **Coherence score** (1 to 5) is separate and judged against the four known tensions from
  `docs/personas-ctx.md`. Start at 5 and subtract 1 for each tension the version resolves badly:
  - Sam vs Priya: is there a calm top with real depth underneath, or does one starve the other?
  - Marcus vs Sam: is the trust story findable without being loud?
  - Alex vs everyone: does the screen teach when empty, not just inform when full?
  - Jordan vs history: is there exactly one "reclaimed" number, identical everywhere it appears?

A high overall with a low coherence score means the design is good at pleasing personas in isolation
but pulls apart as a whole. Report both.

## Verdict bands

- **Ship**: overall >= 4.2, coherence >= 4, no persona below 3.5, and no dimension below 3 for a
  persona that weights it 3.
- **Iterate**: overall 3.4 to 4.2, no persona below 3.0. Named fixes, then re-run.
- **Rework**: overall below 3.4, or any persona below 3.0, or coherence below 3. The direction, not
  the details, needs another pass.

A regression on any single persona versus the prior version blocks a Ship even if the overall rose.
Say so plainly.
