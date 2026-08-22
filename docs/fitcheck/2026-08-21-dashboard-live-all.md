# fitcheck: live dashboard http://127.0.0.1:8797 (all views)

- Date: 2026-08-21
- Target: live dashboard at http://127.0.0.1:8797 (Home, See, Save, Settings), full mode, scope all
- Compare: docs/fitcheck/2026-08-21-dashboard-live-all.md (same target, same rubric; prior baseline)
- Evidence: rendered PNGs read from target/fitcheck-shots for the populated state (home-1..2, see-1..2, save-1..6, settings-1..2) and from target/fitcheck-shots/empty for the cold first-run state (home-1..2, see, save-1..2, settings-1..2). The empty set is a genuine cold render (autopilot off, 0 originals, ladder at 0, "ctx just started and has not seen any tool output yet"), so Alex's Visual execution is scored from a real image and is not capped. Empty-state copy cross-checked against src/dashboard.html.
- Rubric version: 2026-08-21
- Filename note: saved under a distinct `-rerun` slug so the prior same-date baseline (`2026-08-21-dashboard-live-all.md`) used as the compare target is not overwritten.

## Headline

- **Overall: 4.2 / 5** &nbsp; **Coherence: 4.5 / 5** &nbsp; **Verdict: Ship**
- Flat re-run. The product is materially unchanged from the prior baseline: same copy, same clean cold render, one consistent cumulative number (now 13.3M, up from 12.9M only because more data was ingested), and the same two seams still open. It holds the floor Ship, no more and no less.
- vs prior: **0.0** (4.2 to 4.2), no biggest mover, **no regressions**. The two named fixes from the last run (abstract empty-Home headline, "last ingest: just now" on an empty DB) are both still present, so nothing moved.

## Persona scores

| Persona | Score | vs prior | One-line read |
|---|---|---|---|
| Sam (pragmatist)   | 4.3 | 0.0 | One number (13.3M), "nothing to do", one button. Home is still built for him. |
| Priya (power user) | 4.3 | 0.0 | Itemized bill, per-tool evidence, real prune levers; depth intact and unchanged. |
| Marcus (skeptic)   | 4.4 | 0.0 | Fail-closed and real n hold; the empty state still shows honest "not exercised yet" instead of faking it. |
| Alex (first-run)   | 3.8 | 0.0 | Cold state renders honest, teaching, calm. The abstract headline still holds him under 4. |
| Jordan (budget)    | 4.0 | 0.0 | 13.3M identical on Home, See, Settings; empty state stays at 0. Weekly verdict still buried mid-Save. |

## Dimension grid

| Dimension | Sam | Priya | Marcus | Alex | Jordan |
|---|---|---|---|---|---|
| Comprehension    | 4   | 4.5 | 4.5 | 3.5 | 4   |
| Time-to-value    | 4.5 | 4   | 3.5 | 4   | 3.5 |
| Trust and safety | 4   | 4   | 4.5 | 4   | 4   |
| Cognitive load   | 4   | 3   | 4   | 4   | 3.5 |
| Action clarity   | 4.5 | 4.5 | 4   | 4   | 3.5 |
| Journey coherence| 4.5 | 4.5 | 4.5 | 4   | 4.5 |
| Visual execution | 4.5 | 4   | 4   | 4   | 4   |
| Delight          | 4   | 4.5 | 3.5 | 3.5 | 3.5 |

Alex's Visual execution is scored from target/fitcheck-shots/empty, not capped. The cold Home, See, Save, and Settings renders are clean: one shared left rail, ladder rungs drawn at 0 with their tracks, consistent cards, no duplicated headers, no loose controls, no overflow or collision.

## Walkthroughs

- **Sam:** Lands on Home. Eye hits "13.3M tokens, reclaimed so far", then "Autopilot is trimming 1 tool whose comparable runs passed the safety check. The tools still run in full, and nothing here needs a decision from you." One button, "See how it decides". Better off, nothing to do, in about three seconds. Never scrolls into the ladder. Unchanged, still his screen.

- **Priya:** Goes to Save. Edit is TRIMMING with "108 comparable runs, re-edits +0.0 pts, fixes +1.3 pts" and "Show the evidence"; Shell is TESTING with "Stop the trial"; Read is EVALUATING with "92 comparable runs, 9 pulled back" and "Put on trial". Tools group by platform (Claude Code, Cursor, Linear, Figma, Ctx, Notion) with run counts, then per-agent surfaces and per-route model detail. See itemizes the output tax biggest-first (view_image 44.3M at 90% trimmable, Shell 24.1M, Read 19.1M) with call counts, and the input tax carries explicit "Prune 41 dead / Prune 19 dead / Prune 15 dead" levers under a "0 tool-misses in 30 days" safety line. Real levers, real evidence. She still gets summarized evidence, not a raw payload or JSON audit view.

- **Marcus:** Reads for the catch. Home tells the fail-closed story plainly ("It only shortens output after comparable runs pass the safety check", "behavioral evidence, not causal proof", the REVERSIBLE callout "one command back. ctx never removes a capability"). Save backs it with n: "161 randomized trimmed results checked against what you did next, 1 correction", "43 tools recovered with ctx_expand". Settings gives him "Test restore", "No remote destinations registered", "Corrections after trim 1/704", "Gateway failures 0/3". The cold state holds his trust: empty Save reads "No randomized trims have been scored yet. CTX keeps normal output unchanged while it builds a comparable safety test" and "No trims recovered yet", and empty Home says "Nothing reclaimed yet, and CTX will not pretend otherwise." That honest empty state is what his persona rewards, and it is shown, not asserted.

- **Alex (cold first-run):** The make-or-break screen renders real and cold. Empty Home leads with the standing lede "See where context goes. Reclaim noise with the original one step away", then "ctx just started and has not seen any tool output yet" and "Run a few agent turns. CTX watches what your tools return, then reclaims room only after comparable runs pass its safety check. Your number appears right here." The button even adapts tense: "See how it will decide". Beat two: "No tools on the ladder yet. As you work, each tool starts at Watching and only moves toward Trimming once comparable runs pass the safety check." Beat three: "Nothing reclaimed yet, and CTX will not pretend otherwise. The number moves here only after comparable runs pass the safety check." See empty: "Nothing to itemize yet. Run a few agent turns and your output bill fills in here" and "No MCP servers seen yet." Settings empty: autopilot off with "Turn on autopilot", 0 originals, "Nothing to read yet" on context pressure. This reads as warming up, not broken, and it teaches what will happen. What still holds him under 4 is the static headline: "Reclaim noise with the original one step away" is abstract for someone who does not yet know what ctx is, and it sits above the teaching copy, not below it. The warm copy also leans on "comparable runs" and "safety check" before she has the concept. Unchanged from the prior run.

- **Jordan:** Wants one defensible figure and gets it: 13.3M reclaimed on Home, 13.3M output reclaimed on See, ~13.3M tok (53,039,244 chars) in Settings Product proof, and Home restates "reclaimed 13.3M tokens ... output trimmed and tool menu pruned, counted once ... they add up to exactly this". The empty state stays consistent: 0 everywhere, no phantom number. Her weekly verdict exists on Save ("Yes, net-ahead", "3.4M of 50K tokens", "6% vs 29%") but still sits mid-Save behind the ladder, not near a headline.

## Friction, most costly first

1. **Empty-state headline is abstract and jargon-first (Alex, comprehension).** Cold Home leads with the static lede "See where context goes. Reclaim noise with the original one step away" before any plain statement of what ctx does, and the warm copy uses "comparable runs" and "safety check" before Alex has the concept. This is the single thing keeping his score under 4. Fix: lead the empty Home with one plain sentence of what ctx does for her, before the "reclaim noise" framing and the vocabulary. Still open from the prior run.

2. **"Last ingest: just now" on a never-ingested empty DB (Alex and Jordan, journey coherence).** Empty Settings shows "Last ingest: just now" alongside "Sessions 0", "Tool invocations 0", and "Tracking since n/a". With nothing ingested, "just now" is a small one-state contradiction of the kind this dimension hunts. It costs the half point on coherence. Fix: show "n/a" or "not yet" for last ingest when session and invocation counts are zero. Still open from the prior run.

3. **The weekly net-ahead verdict is buried (Jordan, time-to-value).** "Yes, net-ahead" with the 3.4M weekly figure and the 6%-vs-29% trend is what Jordan opens the product for, but it sits mid-Save behind the ladder and the MCP-drop callout. Fix: surface the weekly net-ahead line higher on Save or echo it on Home.

4. **See stacks three large numbers before the payoff is isolated (Jordan and Sam, cognitive load).** The two-taxes header shows 113.1M read back, 85.9M reclaimable, and 13.3M reclaimed close together; the payoff figure does not stand apart from the two larger context numbers. Fix: visually subordinate read-back and reclaimable so the one reclaimed figure is unmistakably the payoff.

5. **Per-tool undo command not shown next to each trimming row (Marcus, trust).** "One command back" is stated and `ctx expand <rewind-id>` appears on Settings and the model-routes recover lines, but not inside a TRIMMING tool's "Show the evidence" panel. Fix: put the recover command where the action is, in each trimming tool's evidence panel.

## Coherence check (the four tensions)

- Sam vs Priya: **resolved.** Home's first viewport is one number, one line, one button; Save and See carry the depth one click down. Neither starves the other.
- Marcus vs Sam: **resolved.** The trust story (fail-closed, reversible, real n) lives on Save and Settings and in Home's lower beats, and the empty state shows honest "not exercised yet" copy. Marcus finds it; Sam never has to read it on the first screen.
- Alex vs everyone: **resolved.** The design teaches when empty and informs when full, verified in render: the cold Home, See, Save, and Settings all read as warming up with a clear "here is what is coming", not as broken. Held back from a clean full point by the abstract standing headline and the "last ingest: just now" seam, both still open.
- Jordan vs history (one number): **resolved.** 13.3M is identical on Home, See, and Settings, output plus input (input 0) sums to it, and the empty state stays at 0 with no phantom figure. The old three-numbers bug stays dead.

## Verdict and next move

Ship, at the floor. Overall 4.2 meets the Ship line, coherence is 4.5, no persona sits below 3.5 (Alex is lowest at 3.8), and no persona's weight-3 dimension is below 3. This is a flat re-run: the UI is unchanged from the prior baseline, so no persona moved in either direction and the Ship is not blocked by a regression. The single most important thing before the next run is unchanged: lead the empty Home with one plain sentence of what ctx does, before the "reclaim noise" lede and the vocabulary, which is the one change that would move Alex clear of the floor. Fix the "last ingest: just now" empty-state seam at the same time so it does not become a coherence regression.
