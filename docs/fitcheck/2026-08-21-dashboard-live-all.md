# fitcheck: live dashboard http://127.0.0.1:8797 (all views)

- Date: 2026-08-21
- Target: live dashboard at http://127.0.0.1:8797 (Home, See, Save, Settings), full mode, scope all
- Compare: docs/fitcheck/2026-07-07-report-issue-modal.md (different scope and rubric; see the honesty note below)
- Evidence: rendered PNGs read from target/fitcheck-shots for the populated state (home-1..2, see-1..2, save-1..6, settings-1..2) and from target/fitcheck-shots/empty for the cold first-run state (home-1..2, see, save-1..2, settings-1..2). This run the empty set is a genuine cold render (autopilot off, 0 originals, ladder at 0, "ctx just started and has not seen any tool output yet"), so Alex's Visual execution is scored from a real image and is no longer capped. Empty-state structure cross-checked against src/dashboard.html (hero at 1577-1582, ladder empty at 1605, restate at 1612, static lede at 710).
- Rubric version: 2026-08-21
- Filename note: two earlier same-date runs were discarded. Both were blocked by broken tooling in the gate itself (a cold-start capture that was silently populated), so they scored the harness rather than the product. This is the first real baseline under the 2026-08-21 rubric.

## Headline

- **Overall: 4.2 / 5** &nbsp; **Coherence: 4.5 / 5** &nbsp; **Verdict: Ship**
- The one thing that blocked the last two runs is fixed: the cold first-run state now actually renders cold and it teaches honestly, so Alex can be scored on a real image instead of a stale populated capture. No persona regressed; Alex jumps and clears his floor. This is a tight Ship at the line, held there by a still-abstract standing headline.
- vs prior: **up 0.1** (4.1 to 4.2), biggest mover **Alex +0.4** (empty state verified and honest, Visual uncapped). No regressions.

## Persona scores

| Persona | Score | vs prior | One-line read |
|---|---|---|---|
| Sam (pragmatist)   | 4.3 | 0.0  | One number (12.9M), "nothing to do", one button. Home is still built for him. |
| Priya (power user) | 4.3 | 0.0  | Itemized bill, per-tool evidence, real prune levers; depth unchanged and intact. |
| Marcus (skeptic)   | 4.4 | +0.1 | Fail-closed and real n hold, and the empty state now shows honest "no evidence yet" instead of faking it. |
| Alex (first-run)   | 3.8 | +0.4 | Cold state finally renders: honest, teaching, calm. Only the abstract headline holds him under 4. |
| Jordan (budget)    | 4.0 | 0.0  | 12.9M identical on Home, See, Settings; empty state stays at 0 with no contradiction. Weekly verdict still buried. |

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

Alex's Visual execution is scored from target/fitcheck-shots/empty this run, not capped. The cold Home, See, Save, and Settings renders are clean: one shared left rail, ladder rungs drawn at 0 with their tracks, consistent cards, no duplicated headers, no loose controls, no overflow or collision.

## Walkthroughs

- **Sam:** Lands on Home. Eye hits "12.9M tokens, reclaimed so far", then "Autopilot is trimming 1 tool whose comparable runs passed the safety check. The tools still run in full, and nothing here needs a decision from you." One button, "See how it decides". Better off, nothing to do, in about three seconds. Never scrolls into the ladder. Unchanged from prior, still his screen.

- **Priya:** Goes to Save. Edit is TRIMMING with "108 comparable runs, re-edits +0.0 pts, fixes +1.3 pts" and "Show the evidence"; Shell is TESTING with "Stop the trial"; Read is EVALUATING with "92 comparable runs, 9 pulled back" and "Put on trial". Tools group by platform (Claude Code, Linear, Figma, Ctx, Notion) with run counts, then per-agent surfaces and per-route model detail. See itemizes the output tax biggest-first (view_image 44.3M at 90% trimmable, Shell 24.1M, Read 18.5M) with call counts, and the input tax carries explicit "Prune 41 dead / Prune 19 dead / Prune 15 dead" levers under a "0 tool-misses in 30 days" safety line. Real levers, real evidence. She still gets summarized evidence, not a raw payload/JSON audit view.

- **Marcus:** Reads for the catch. Home tells the fail-closed story plainly ("It only shortens output after comparable runs pass the safety check", "behavioral evidence, not causal proof", the REVERSIBLE callout "one command back. ctx never removes a capability"). Save backs it with n: "161 randomized trimmed results checked against what you did next, 1 correction", "43 tools recovered with ctx_expand". Settings gives him "Test restore", "No remote destinations registered", "Corrections after trim 1/704", "Gateway failures 0/3". What lifts him this run is the cold state: empty Save reads "No randomized trims have been scored yet. CTX keeps normal output unchanged while it builds a comparable safety test" and "No trims recovered yet", and empty Home says "Nothing reclaimed yet, and CTX will not pretend otherwise." That honest empty state is exactly what his persona rewards, and it is now shown rather than asserted.

- **Alex (cold first-run):** This is the make-or-break screen and this run it is real. Empty Home leads with the standing lede "See where context goes. Reclaim noise with the original one step away", then "ctx just started and has not seen any tool output yet" and "Run a few agent turns. CTX watches what your tools return, then reclaims room only after comparable runs pass its safety check. Your number appears right here." Beat two: "No tools on the ladder yet. As you work, each tool starts at Watching and only moves toward Trimming once comparable runs pass the safety check." Beat three restates "Nothing reclaimed yet, and CTX will not pretend otherwise." See empty: "Nothing to itemize yet. Run a few agent turns and your output bill fills in here" and "No MCP servers seen yet." Settings empty: autopilot off with "Turn on autopilot", 0 originals, "Nothing to read yet" on context pressure. This reads as warming up, not broken, and it teaches what will happen. What still holds him under 4 is the static headline (src/dashboard.html:710): "Reclaim noise with the original one step away" is abstract for someone who does not yet know what ctx is, and it sits above the teaching copy, not below it. The warm copy also leans on "comparable runs" and "safety check" before she has the concept.

- **Jordan:** Wants one defensible figure and gets it: 12.9M reclaimed on Home, 12.9M output reclaimed on See, ~12.9M tok (51,434,998 chars) in Settings Product proof, and Home restates "reclaimed 12.9M tokens ... output trimmed and tool menu pruned, counted once ... they add up to exactly this" (output 12.9M + input 0). The empty state stays consistent too: 0 everywhere, no phantom number. Her weekly verdict exists on Save ("Yes, net-ahead", "3.0M of 50K tokens", "6% vs 29%") but still sits mid-Save behind the ladder, not near a headline.

## Friction, most costly first

1. **Empty-state headline is abstract and jargon-first (Alex, comprehension).** The cold Home leads with the static lede "See where context goes. Reclaim noise with the original one step away" (src/dashboard.html:710) before any plain statement of what ctx does, and the warm copy uses "comparable runs" and "safety check" before Alex has the concept. This is the single thing keeping his score under 4 and the overall off a comfortable Ship. Fix: lead the empty Home with one plain sentence of what ctx does for her, before the "reclaim noise" framing and the vocabulary.

2. **"Last ingest: just now" on a never-ingested empty DB (Alex and Jordan, journey coherence).** Empty Settings shows "Last ingest: just now" alongside "Sessions 0", "Tool invocations 0", and "Tracking since n/a". With nothing ingested, "just now" is a small one-state contradiction of the kind this dimension hunts. It costs the half point on coherence and could grow into a real seam. Fix: show "n/a" or "not yet" for last ingest when session and invocation counts are zero.

3. **The weekly net-ahead verdict is buried (Jordan, time-to-value).** "Yes, net-ahead" with the 3.0M weekly figure and the 6%-vs-29% trend is what Jordan opens the product for, but it sits mid-Save behind the ladder and the MCP-drop callout. Fix: surface the weekly net-ahead line higher on Save or echo it on Home.

4. **See stacks three large numbers before the payoff is isolated (Jordan and Sam, cognitive load).** The two-taxes header shows 112.4M read back, 85.2M reclaimable, and 12.9M reclaimed close together; the payoff figure does not stand apart from the two larger context numbers. Fix: visually subordinate read-back and reclaimable so the one reclaimed figure is unmistakably the payoff.

5. **Per-tool undo command not shown next to each trimming row (Marcus, trust).** "One command back" is stated and `ctx expand <rewind-id>` appears on Settings and the model-routes recover lines, but not inside a TRIMMING tool's "Show the evidence" panel. Fix: put the recover command where the action is, in each trimming tool's evidence panel.

## Coherence check (the four tensions)

- Sam vs Priya: **resolved.** Home's first viewport is one number, one line, one button; Save and See carry the depth one click down. Neither starves the other.
- Marcus vs Sam: **resolved.** The trust story (fail-closed, reversible, real n) lives on Save and Settings and in Home's lower beats, and the empty state now shows honest "not enough evidence yet" copy. Marcus finds it; Sam never has to read it on the first screen.
- Alex vs everyone: **resolved (this run).** The design now teaches when empty and informs when full, verified in render: the cold Home, See, Save, and Settings all read as warming up with a clear "here is what is coming", not as broken. This regains the point lost in the prior two runs. Held back from a clean full point by the abstract standing headline and the "last ingest: just now" seam.
- Jordan vs history (one number): **resolved.** 12.9M is identical on Home, See, and Settings, output plus input (input 0) sums to it, and the empty state stays at 0 with no phantom figure. The old three-numbers bug stays dead.

## Verdict and next move

Ship, at the floor. Overall 4.2 meets the Ship line, coherence is 4.5, no persona sits below 3.5 (Alex is the lowest at 3.8), and no persona's weight-3 dimension is below 3. Most important: the sole blocker from the prior two runs, an "empty" shot set that was actually the populated dashboard, is fixed. This run's empty set is a genuine cold render and it teaches honestly, so Alex is graded on real evidence and rises +0.4 with no persona regressing. It is a tight Ship, not a comfortable one. The single most important thing before the next run: lead the empty Home with one plain sentence of what ctx does, before the "reclaim noise" lede and the vocabulary, which is the one change that would move Alex clear of the floor and make the overall a confident Ship rather than a boundary one. Fix the "last ingest: just now" empty-state seam at the same time so it does not become a coherence regression.
