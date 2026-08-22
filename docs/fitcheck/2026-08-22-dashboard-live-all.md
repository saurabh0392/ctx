# fitcheck: live dashboard http://127.0.0.1:8797 (all views)

- Date: 2026-08-22
- Target: live dashboard at http://127.0.0.1:8797 (Home, See, Save, Settings), full mode, scope all
- Compare: docs/fitcheck/2026-08-21-dashboard-live-all.md (prior baseline, same target, same rubric)
- Evidence: rendered PNGs read from target/fitcheck-shots for the populated state (home-1..2, see-1..2, save-1..6, settings-1..2) and from target/fitcheck-shots/empty for the cold first-run state (home-1..2, see, save-1..2, settings-1..2). The empty set is a genuine cold render (autopilot off, 0 originals at 0 B, ladder rungs drawn at 0, "ctx just started and has not seen any tool output yet"), so Alex's Visual execution is scored from a real image and is not capped. Copy and the empty-state seams cross-checked against src/dashboard.html.
- Rubric version: 2026-08-21

## Headline

- **Overall: 4.2 / 5** &nbsp; **Coherence: 4.5 / 5** &nbsp; **Verdict: Ship**
- Another floor Ship. The product is materially the same as the prior baseline: one consistent cumulative number (now 13.7M, up from 13.3M only because more work was ingested), the same clean cold render, and the same two seams still open. The one structural change is cosmetic to the score: the weekly net-ahead ledger now sits in a "Weekly ledger" fold above the MCP-drop callout instead of below it.
- vs prior: **0.0** (4.2 to 4.2), no biggest mover, **no regressions**. Both named fixes from the last run (abstract empty-Home headline, "last ingest: just now" on an empty DB) are still present, so no persona moved.

## Persona scores

| Persona | Score | vs prior | One-line read |
|---|---|---|---|
| Sam (pragmatist)   | 4.3 | 0.0 | One number (13.7M), "nothing to do", one button. Home is still his screen. |
| Priya (power user) | 4.3 | 0.0 | Itemized bill, per-tool evidence, real prune levers; depth intact and unchanged. |
| Marcus (skeptic)   | 4.4 | 0.0 | Fail-closed story and real n hold; the cold state still says "not exercised yet" instead of faking it. |
| Alex (first-run)   | 3.8 | 0.0 | Cold render is honest, teaching, calm. The abstract standing headline still holds him under 4. |
| Jordan (budget)    | 4.0 | 0.0 | 13.7M identical on Home, See, Settings; empty stays at 0. Weekly verdict now higher but folded, still off Home. |

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

- **Sam:** Lands on Home. Eye hits "13.7M tokens, reclaimed so far", then "Autopilot is trimming 1 tool whose comparable runs passed the safety check. The tools still run in full, and nothing here needs a decision from you." One button, "See how it decides". Better off, nothing to do, in about three seconds. Never scrolls into the ladder. Unchanged, still built for him.

- **Priya:** Goes to Save. Edit is TRIMMING with "108 comparable runs, re-edits +0.0 pts, fixes +1.3 pts" and "Show the evidence"; Shell is TESTING with "Stop the trial"; Read is EVALUATING with "93 comparable runs, 9 pulled back, 8 more comparable runs needed before CTX decides" and "Put on trial". Tools group by platform (Claude Code, Linear, Figma, Ctx, Notion, Cursor) with per-tool run counts, then per-agent surfaces and per-route model detail. See itemizes the output tax biggest-first (view_image 44.3M at 90% trimmable, Shell 24.1M, Read 19.7M) with call counts, and the input tax carries explicit "Prune 41 dead / Prune 19 dead / Prune 15 dead" levers under a "0 tool-misses in 30 days" safety line. Real levers, real evidence. She still gets summarized evidence, not a raw payload or JSON audit view.

- **Marcus:** Reads for the catch. Home tells the fail-closed story plainly ("It only shortens output after comparable runs pass the safety check", "behavioral evidence, not causal proof", the REVERSIBLE callout "a trimmed detail already ran in full and is one command back. ctx never removes a capability"). Save backs it with n: "161 randomized trimmed results checked against what you did next, 1 correction", "43 trims recovered with ctx_expand". Settings gives him "Test restore", "No remote destinations registered", "Corrections after trim 1 / 704", "Gateway failures 0 / 3". The cold state holds: empty Save reads "No randomized trims have been scored yet. CTX keeps normal output unchanged while it builds a comparable safety test" and "No trims recovered yet", empty Home says "Nothing reclaimed yet, and CTX will not pretend otherwise." That honest empty state is shown, not asserted, which is what his persona rewards.

- **Alex (cold first-run):** The make-or-break screen renders real and cold. Empty Home leads with the standing lede "See where context goes. Reclaim noise with the original one step away", then "ctx just started and has not seen any tool output yet" and "Run a few agent turns. CTX watches what your tools return, then reclaims room only after comparable runs pass its safety check. Your number appears right here." The button adapts tense to "See how it will decide". Beat two: "No tools on the ladder yet. As you work, each tool starts at Watching and only moves toward Trimming once comparable runs pass the safety check." Beat three: "Nothing reclaimed yet, and CTX will not pretend otherwise." See empty: "Nothing to itemize yet. Run a few agent turns and your output bill fills in here" and "No MCP servers seen yet." Settings empty: autopilot off with "Turn on autopilot", 0 originals at 0 B, "Nothing to read yet" on context pressure. This reads as warming up, not broken, and it teaches what is coming. What still holds him under 4 is the static headline: "Reclaim noise with the original one step away" is abstract for someone who does not yet know what ctx is, and it sits above the teaching copy, not below it. The warm copy also leans on "comparable runs" and "safety check" before she has the concept. Unchanged from the prior run.

- **Jordan:** Wants one defensible figure and gets it: 13.7M reclaimed on Home, 13.7M output reclaimed on See, 54,824,509 chars (~13.7M tok) in Settings Product proof, and Home restates "reclaimed 13.7M tokens ... output trimmed and tool menu pruned, counted once ... they add up to exactly this". The empty state stays consistent: 0 everywhere, no phantom number. Her weekly verdict ("Yes, net-ahead", "3.8M of 50K tokens", "6% vs 29%") now sits higher on Save, in a "Weekly ledger" fold placed above the MCP-drop callout and the tools-by-platform breakdown rather than below them. That is a modest step toward her, but the verdict is still off Home and now gated behind a fold, so her time-to-value does not clear the prior mark.

## Friction, most costly first

1. **Empty-state headline is abstract and jargon-first (Alex, comprehension).** Cold Home leads with the static lede "See where context goes. Reclaim noise with the original one step away" before any plain statement of what ctx does, and the warm copy uses "comparable runs" and "safety check" before Alex has the concept. This is the single thing keeping his score under 4. Fix: lead the empty Home with one plain sentence of what ctx does for her, before the "reclaim noise" framing and the vocabulary. Still open from the prior run.

2. **"Last ingest: just now" on a never-ingested empty DB (Alex and Jordan, journey coherence).** Empty Settings shows "Last ingest: just now" alongside "Sessions 0", "Tool invocations 0", and "Tracking since n/a". src/dashboard.html:3004 renders this line through timeAgo(last_ingest_at), which returns "just now" under 60 seconds regardless of whether any session exists. With nothing ingested, "just now" is a one-state contradiction of the kind this dimension hunts. It costs the half point on coherence. Fix: show "n/a" or "not yet" for last ingest when session and invocation counts are zero. Still open from the prior run.

3. **The weekly net-ahead verdict is still off Home and now folded (Jordan, time-to-value).** "Yes, net-ahead" with the 3.8M weekly figure and the 6%-vs-29% trend is what Jordan opens the product for. It moved up on Save (now above the MCP-drop callout, in a "Weekly ledger / net-ahead verdict, week by week" fold), but it is still not on Home and is behind a details fold that is collapsed by default. Fix: echo the weekly net-ahead line on Home, or promote it out of the fold near the top of Save.

4. **See stacks three large numbers before the payoff is isolated (Jordan and Sam, cognitive load).** The two-taxes header shows 113.7M read back, 86.5M reclaimable, and 13.7M output reclaimed close together; the payoff figure does not stand apart from the two larger context numbers. Fix: visually subordinate read-back and reclaimable so the one reclaimed figure is unmistakably the payoff.

5. **Per-tool undo command not shown next to each trimming row (Marcus, trust).** "One command back" is stated and ctx_expand appears on Settings and the model-routes recover lines, but not inside a TRIMMING tool's "Show the evidence" panel. Fix: put the recover command where the action is, in each trimming tool's evidence panel.

## Coherence check (the four tensions)

- Sam vs Priya: **resolved.** Home's first viewport is one number, one line, one button; Save and See carry the depth one click down. Neither starves the other.
- Marcus vs Sam: **resolved.** The trust story (fail-closed, reversible, real n) lives on Save and Settings and in Home's lower beats, and the empty state shows honest "not exercised yet" copy. Marcus finds it; Sam never has to read it on the first screen.
- Alex vs everyone: **resolved.** The design teaches when empty and informs when full, verified in render: the cold Home, See, Save, and Settings all read as warming up with a clear "here is what is coming", not as broken. Held back from a clean full point by the abstract standing headline and the "last ingest: just now" seam, both still open.
- Jordan vs history (one number): **resolved.** 13.7M is identical on Home, See, and Settings, output plus input (input 0) sums to it, and the empty state stays at 0 with no phantom figure. The weekly 3.8M figure is consistent with itself across the ledger cells. The old three-numbers bug stays dead.

## Verdict and next move

Ship, at the floor. Overall 4.2 meets the Ship line, coherence is 4.5, no persona sits below 3.5 (Alex is lowest at 3.8), and no persona's weight-3 dimension is below 3. No persona regressed versus the prior baseline, so the Ship is not blocked. This is effectively a flat re-run: the cumulative figure grew consistently and the weekly ledger changed position without moving a rounded persona score. The single most important thing before the next run is unchanged: lead the empty Home with one plain sentence of what ctx does, before the "reclaim noise" lede and the vocabulary, which is the one change that would move Alex clear of the floor. Fix the "last ingest: just now" empty-state seam at the same time so it does not harden into a coherence regression.
