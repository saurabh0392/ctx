# fitcheck: Report an issue modal

Target: the "Report an issue" modal integrated into `src/dashboard.html`, live on 8790.
Scope: the report flow (modal, not a view). States judged: populated and empty (no runs).
Rubric and personas: unchanged from prior runs, so scores are comparable.

**Overall: 3.9 / 5** &nbsp; **Coherence: 4 / 5** &nbsp; **Verdict: Iterate**

One notch below Ship. The trust story is strong and legible; what holds it back is density (everything
is shown at once) and the missing raw-payload view a skeptic and a power user both reach for.

## Per persona

**Marcus, skeptic (4.1).** This is built for him. He does not have to trust "counts only", he sees it:
the snapshot table shows the exact numbers, the "Never included: file paths, command text, tool output,
prompts, or any content" line is concrete, and "Review what gets sent" renders the literal body before
he commits. "Nothing is sent until you press Send" is the reassurance he wants. Two gaps. The banner
"ctx attaches counts only, never your code, file paths, or tool output" reads as a whole-modal
guarantee, but the Example box and screenshots can carry paths since they are user-supplied. It is
clarified below the fields, so it is honest, but the banner slightly overclaims for the whole modal.
And post-send reversibility is unstated: a sent issue cannot be unsent, and nothing says whether he can
retract it or who sees it.

**Alex, first-run (3.9).** The empty state is handled honestly: the snapshot reads "No runs recorded
yet, so there is nothing to attach" rather than looking broken. The flow is self-explanatory and the
privacy banner plus "no account needed" makes pressing Send feel safe. The snapshot table carries terms
a brand-new user will not fully parse (Stage, Recoveries, Re-read delta), but it is clearly optional
diagnostic data and toggleable, so understanding it is not required. The modal is long for a first
report.

**Priya, power user (4.1).** She recognizes the snapshot immediately: it is the real per-tool data from
the dashboard she already reads (Read earned 6.5M, Edit 4.2M), and the copy says so ("the same numbers
you see on the dashboard"). The include toggle is a real control. What is missing for her is granularity
and a raw view: it is all-or-nothing, and there is no "View raw JSON" to see the exact bundle. The
preview shows a summarized snapshot line, not the full object she would want to audit.

**Sam, pragmatist (3.6).** For a quick "this broke" he can type a title and description and Send, the
rest is optional. But the modal looks like a lot: an expanded snapshot table, a screenshots zone, and a
live preview are all visible at once. For a bug report he wants to fire and forget, that reads as a
chore. This is the main thing dragging the overall down.

**Jordan, budget (4.0).** Largely out of scope for a feedback modal, but the one thing she cares about
holds: the snapshot's Reclaimed figures come from the same source as the dashboard, so nothing
contradicts. No figures disagree.

## Friction, most costly first

1. **Snapshot expanded by default (Sam, cognitive-load).** The diagnostic table is open on load, making
   the modal feel heavy for a quick report. Fix: collapse it behind a one-line summary ("Diagnostic
   snapshot: N tools, counts only, included") with a chevron. Marcus's inspectability stays one click
   away; Sam's view gets lighter. This is the single biggest lever toward Ship.
2. **No raw-payload view (Priya and Marcus, control and trust).** The prototype's "View raw JSON" was
   dropped. Both want to see the exact bytes, not a summary. Fix: add a "View raw JSON" link that shows
   the full bundle object.
3. **Banner overclaims scope (Marcus, coherence).** "counts only, never your code, file paths" is true
   for ctx's auto-attachment but not for what the user types or attaches. Fix: scope it, for example
   "ctx's attachment is counts only. Anything you type or attach is sent as-is, so redact it first."
4. **Post-send reversibility unstated (Marcus, trust).** Add a line on what happens after Send: not
   public, and can be deleted on request.
5. **Trigger reads near the tabs (Alex, comprehension).** "Report" sits in the nav next to Home/See/Save.
   It is styled quieter (mono, muted), so minor, but it could read as a fifth page. Watch in context.

## Verdict and next move

Iterate. Apply fix 1 (collapse the snapshot) and fix 2 (raw JSON view); those two address the load and
control gaps that hold Sam and Priya below their ceilings and would likely lift the overall to about
4.2, Ship. Fixes 3 and 4 are quick copy and cheap trust wins. Re-run after.
