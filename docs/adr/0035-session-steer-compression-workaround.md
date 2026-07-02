# 0035. Session steers and compression workaround signals

- Status: accepted
- Date: 2026-06-27
- Deciders: Saurabh, CTX
- For: CTX-50

## Context

CTX-48 tightened gate corrections to explicit complaints on trimmed decisions. That fixed bulk false positives, but two cases still mislabeled harm:

1. **Session steers.** Users pivot scope with phrases like "lets do the fun stuff" or "drop it." A bare "nope" in that message matched `NEGATIVE_CUES` and counted as `correction_explicit`, feeding `outcome_correction` on the wrong tool (for example a trimmed Bash metadata call the user was not complaining about).

2. **Compression workarounds.** When ctx trims a Read, the agent sometimes narrates that output was compressed, then uses Bash to write JSON or a dump back to disk. The workaround is harm from the Read trim, not from Bash doing its job.

Both need to be recorded honestly without widening what the activation gate counts as a correction (ADR 0019 observe-first).

## Decision

1. **`CorrectionClass::Steer` and `session_steer` flag.** Topic pivots match `STEER_PHRASES` and do not carry output-specific complaints (`has_output_specific_complaint`). Steers are checked before negative cues so "nope... lets do the fun stuff" is a steer, while "nope. bad bg image" stays explicit. Steers persist as `session_steer` in turn flags and `outcome_signals` only. They never set `correction_explicit` or `outcome_correction`.

2. **`compression_workaround` signal (observation only).** After a trimmed decision (`applied=1`, `lines_drop>0`), if the agent narrates compression or truncation and then calls Bash/Shell with a bypass fingerprint (json, heredoc, redirect, inline python/node) within the structural window, record `compression_workaround` on the trimmed decision. The transcript join requires narration plus bypass; the timestamp join uses structural bypass only when narration is unavailable. Neither path feeds the gate.

3. **Corpus repair.** `steer_turn_flags_v1` reclassifies legacy `correction_explicit` rows; `rejoin_outcome_labels_v6` rejoins outcomes after the new labels land.

## Alternatives considered

- **Strip "nope" from negative cues.** Rejected: real one-word pushbacks would disappear; steers need a positive pivot phrase, not cue deletion.
- **Count compression workaround on Bash.** Rejected: Bash is the workaround, not the cause; attribution belongs on the trimmed Read (or whichever tool was trimmed).
- **Promote compression workaround into the gate immediately.** Rejected: ADR 0019 requires a precision spot-check first.

## Consequences

- Gate correction counts drop further on steer-heavy sessions; dashboard tool report stays honest.
- Tool report and signal audit can surface `session_steer` and `compression_workaround` before any gate promotion.
- Steer lexicon will need occasional tuning as new pivot phrases appear in real sessions.
