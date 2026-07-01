# 0033. Trim-relevant gate correction labels (all tools)

- Status: accepted
- Date: 2026-06-27
- Deciders: Saurabh (with CTO partner)
- Extends: ADR 0019 (richer outcome signals), ADR 0032 (needed-whole target)
- Part of: CTX-48

## Context

The causal harm gate reads `outcome_correction` on shadow compress decisions. Before this change,
any user turn flagged `%correction%` within 15 minutes of a tool call could label the nearest
preceding decision as corrected, regardless of tool, whether ctx actually trimmed output, or what
kind of user signal fired.

That produced false positives on real usage:

- **Interrupts** (`[Request interrupted by user]`) are session steering, not pushback on tool output.
  They were double-flagged as `aborted` and `correction`, so a Bash sweep stopped mid-run looked
  like trim harm.
- **Workflow commands** ("commit and push", "fold and ADR") are next-step instructions, not
  complaints about what a tool returned.
- **Terse redirects** after substantial work are observational at best; they should not vote on
  whether a trim hurt.
- **Untrimmed decisions** (`applied=0` or `lines_drop=0`) cannot have caused trim harm, yet they
  inherited correction labels when the user complained about something else later in the session.

The gate must answer one question only: did the user explicitly push back on ctx's trim of this
specific tool output?

## Decision

Split recording from voting. Keep rich signals in `outcome_signals`; tighten what sets
`outcome_correction`.

1. **`outcome_correction` (gate label)** is set only when all three hold, for every tool uniformly:
   - the user turn carries `correction_explicit` (high-confidence complaint language),
   - the decision has `applied = 1`,
   - the decision has `lines_drop > 0`.

2. **Interrupts** emit `aborted` only. Never `correction`, never gate.

3. **System/plumbing turns** (`<task-notification>`, `<system-reminder>`, empty text) are excluded
   from correction classification at ingest.

4. **Workflow commands** are excluded from correction classification at ingest.

5. **Terse corrections** (`correction_terse`) are recorded in `outcome_signals` but never set
   `outcome_correction`.

6. **One-time rejoin** (`rejoin_outcome_labels_v3`): reset all joined rows, rerun timestamp and
   transcript joins, refresh `outcome_signals`.

Shared helper: `gate_correction_label(explicit, applied, lines_drop)` in `outcome_signals.rs`.
Both `join_compress_outcomes` (Claude timestamps) and `surface/ingest::join_one` (Cursor ordinals)
call it so every surface applies the same rule.

## Alternatives considered

- **Keep broad `%correction%` matching, down-weight in training.** Rejected: the gate still reads
  `outcome_correction`; mislabels there block tool activation regardless of training weights.
- **Tool-specific rules (stricter for Bash only).** Rejected: the bug was methodological (interrupt
  vs complaint), not Bash-specific. One uniform rule is easier to audit and explain.
- **Drop correction entirely; gate on needed-whole only.** Deferred: correction remains the causal
  harm signal for trim activation; needed-whole is observational (ADR 0032).

## Consequences

- Historical correction counts drop. That is honest: many prior labels were not trim-relevant.
- Label audit and dashboard copy should prefer `outcome_signals` (`correction_explicit`,
  `correction_terse`, `aborted`, `correction_gate`) over raw `%correction%` turn flags.
- Interrupt-heavy sessions stop blocking Bash/Read activation on phantom harm.
- Users who say "commit and push" after a Read no longer poison the prior Read's gate label.
- Rejoin runs once on upgrade; live ingest applies the new rules going forward.
