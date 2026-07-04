# fitcheck: live Settings surface (pass 4)

- Date: 2026-07-04
- Target: `http://127.0.0.1:8789/#settings`, Settings restyled to clean-light with Activity folded in as
  a sub-tab, Profiles card removed. Nav now four groups (Home / See / Save / Settings).
- Compare: the old Settings (dark, five cards including Profiles) and the separate Activity tab
- Evidence: rendered screenshots `settings-live.png`, `activity-live.png`, no console errors
- Rubric version: 2026-07-04

## Headline

- **Overall: 4.3 / 5** &nbsp; **Coherence: 5 / 5** &nbsp; **Verdict: Ship**
- The last utilitarian surface joins the clean-light system. Activity folds in as a sub-tab so the nav
  reaches the four-group target. Profiles is gone, since tool-menu management is now per-server on See,
  which removes a redundant, overlapping control.

## Persona scores

| Persona | Score | One-line read |
|---|---|---|
| Sam (pragmatist)   | 4.2 | Plain toggles and a clear autopilot status. Nothing to puzzle over. |
| Priya (power user) | 4.3 | All the real controls: compression master switch, export, purge, delete, spend cap. |
| Marcus (skeptic)   | 4.5 | "Nothing leaves this machine", delete-all-data, and the privacy toggles read straight. |
| Alex (first-run)   | 4.3 | Cards are self-explanatory; the Activity empty state is built. |
| Jordan (budget)    | 4.2 | The spend cap is where he expects it. |

## Notes

- The four-group nav (Home / See / Save / Settings) is the ADR 0003 endpoint. Activity lives as a
  Settings sub-tab; the old `#activity` deep link opens it directly.
- Profiles removal is a UI change only. The profile system and `/api/profiles` still function; the
  manual switcher is retired because per-server pruning on See is the management model now.
- Sub-tab state is client-side and survives the 20s auto-refresh; the deep link is honored once.
- Timestamps in "Your data" are formatted (date and time-ago) rather than raw ISO.

## Coherence check

- Sam vs Priya: **resolved.** Simple on the surface, full controls present.
- Marcus vs Sam: **resolved.** Privacy and delete controls are clear without shouting.
- Alex vs everyone: **resolved in code.** Activity empty state built; verify on a fresh DB.
- Jordan vs history: **resolved.** One spend control, no competing surfaces.

## Verdict and next move

Ship. This completes the clean-light redesign across all four customer surfaces (Home, See, Save,
Settings) and hits the four-group nav. Standing follow-up: none blocking; the redesign is feature
complete. Optional future work is the tool-catalog capture (to list removed tools with descriptions on
See) noted in the pass-2 report.
