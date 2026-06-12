# 0006. Show savings, but only the earned kind, as the payoff of proof

- Status: accepted
- Date: 2026-06-11
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-14

## Context

ADR 0003 (CTX-4) removed the Savings and Prompt Stats pages because ctx led with a cost story
that was mostly `n/a` and anchored the wrong mental model (ctx as a compressor). That was right.

But the pipeline trace surfaces a real, large number ("80.9K chars compressed") and the question
came up: why hide the value? Not being a compression product does not mean compression has no
value. The honest objection to simply banking that number:

- The big char count comes from tools on trial (Bash, Read) that have not earned the causal gate.
  One of them (Read) showed harm in an earlier trial. Counting that as "money saved" would
  celebrate trims we cannot vouch for: the exact premature-success trap.
- Raw chars are not money. The one-shot dollar value of 80.9K chars is about $0.006 at a
  conservative input rate, so a dollar headline on the gross number would both mislead and
  underwhelm. The real value is context room reclaimed, which compounds because a trimmed result
  stays out of the window on every later turn.

## Decision

Reintroduce savings, but tie it to proof rather than to raw activity, and lead with tokens, not
dollars:

- Safe savings: characters removed by tools whose causal verdict is "safe" (earned). This is the
  only figure shown as real savings. Headline unit is tokens of context kept out of the window;
  a conservative dollar figure is shown underneath, explicitly labeled an estimate, with a note
  that it compounds across turns. Until a tool earns it, this reads "none yet" and says why.
- Trimmed while testing: characters removed by tools that have not earned yet (trials and
  unproven trims). Shown as descriptive activity, in chars and tokens, never as money, with an
  explicit "we do not count this as savings until the proof clears".

Savings are computed from `compress_decisions` applied rows only (chars_in - would_chars_out,
floored at 0), bucketed by the causal verdict, and exposed on `/api/context/proof`
(`safe_chars_saved`, `trial_chars_saved`, and per-tool `applied_chars_saved`). The Tools page shows
the safe-savings card plus the testing line; Home shows the safe-savings headline only once it is
non-zero. No new page is added, and the retired Savings page is not resurrected.

## Alternatives considered

- **Bank the gross number now (trials included).** Rejected: it credits unproven and
  demonstrably harmful trims as success, and contradicts the safety positioning we just shipped
  ("no tool has earned trimming yet").
- **Lead with the dollar figure.** Rejected: the conservative one-shot dollar value is tiny and a
  larger figure would require speculative compounding assumptions. Tokens of context reclaimed is
  the honest, defensible headline; the dollar estimate is secondary and labeled.
- **Bring back the Savings page.** Rejected: ADR 0003 retired it for good reasons. Savings belongs
  inline as the reward for proof, on the Tools page and Home, not as a standalone cost dashboard.

## Consequences

- This partially reverses ADR 0003's "no cost framing" stance, deliberately and narrowly: cost is
  shown only as the earned consequence of safety, never as the headline pitch.
- Day one the safe-savings number is honestly zero (nothing earned). It only grows as tools clear
  their proof, which keeps the story honest on day one, day seven, and day thirty.
- `/api/context/proof` now carries savings fields; contributors changing the compression decision
  schema should keep `chars_in`/`would_chars_out`/`applied` populated for the figure to stay true.
