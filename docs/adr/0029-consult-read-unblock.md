# 0029. Consult-read unblock: trim working reads with no edit intent

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh (with CTO partner)
- Extends: ADR 0001 (read edit-intent guard), ADR 0004 (narration intent signal)

## Context

The read edit-intent guard (ADR 0001) refuses to trim any read of a file under the working tree,
because read-before-edit is the canonical flow and trimming a read of a file the agent is about to
change hides the region and forces re-reads (measured harm in the original Read trial). The cost is
that on a normal single-repo workflow Read trims almost nothing: on the live corpus all 125 Read
decisions were never applied, and the only non-guarded reads wanted to drop a single line. Read is
correctly parked (ADR documented in CTX-44) but never helps.

Not every working read is a read-before-edit. Many are *consult* reads: the agent reads a file to
understand it, with no intent to edit it. The narration intent signal (ADR 0004) already reads the
agent's recent narration, but it was wired protective-only: it could add protection to a reference
read, never remove protection from a working read.

## Decision

Make the read guard a three-way classifier in `agent::decide`, and let the intent signal unblock a
working read when the narration shows the read is consultative.

1. Reference read (deps, vendored, outside the repo): trim-eligible. Unchanged.
2. Working read with edit intent, or with no readable narration: protected. The safe default.
3. Working read the narration shows is consultative: trim-eligible, behind a new opt-in flag
   `compress_read_consult_trim` (default off). The predicate is
   `IntentSignal::consult_read_unblocks()` = `has_text && !has_edit_verb`.

The unblock fails closed and is deliberately strict:

- No readable narration means no unblock. A missing transcript, signature-only thinking, or a Cursor
  session without a transcript leaves the read protected.
- Any edit verb in the recent narration, even about a different file, blocks the unblock. We do not
  require the narration to name this file, because the absence of edit intent for the whole turn is
  the safer, higher-precision signal.

Nothing about the proof changes. An unblocked read still only trims through the existing burn-in and
causal gate: burn-in keeps its 25% baseline-correction fuse, and the gate fails closed and stops
trimming if re-reads or corrections rise. The narration gate exists to keep burn-in trims low-risk;
the gate remains the thing that proves it on the user's own work. Unblocked reads are tagged
(`read_unblock = "consult"` in `features_json`) so their outcomes are measurable in isolation and the
expansion can be rolled back on data alone.

This ADR also adds two logged-only features for the file-aware retention model (CTX-46): the file
extension was already logged; we add a coarse `path_role` (src / test / config / generated /
vendored / docs). Neither changes a trim decision.

## The risk asymmetry

Used protectively (ADR 0004), a false positive (claiming edit intent when there is none) only costs
a little context. Using the *absence* of edit intent to unblock flips this: a false negative
(missing real edit intent) trims a read the agent needed, which is a correctness cost. That is why
the unblock is strict, narration-required, and proven before its default is flipped on, rather than
mirroring the lenient protective path.

## Alternatives considered

- **Remove the static guard, let the causal gate sort it out.** Rejected: ADR 0001 measured real
  harm; burn-in would make the user eat those re-reads for 30+ trimmed runs before the gate reacts.
- **Unblock on `has_text && !edit_intent_for_path()`** (no edit verb *for this file*). Rejected as
  the first cut: an edit verb about a sibling file often precedes editing this one too; requiring no
  edit verb anywhere in the turn is stricter and higher precision for "not about to edit."
- **Default on immediately.** Rejected: unproven on real data. Ship off, prove with the offline
  validation (label reads by whether the agent edited the same file within N turns) and a live
  burn-in arm, then flip.

## Consequences

- Read can finally earn its keep on consult-heavy work, while editable working reads stay protected.
  The path runs entirely through the existing proof, so a wrong heuristic call is caught and stopped,
  not silently shipped.
- Default off means existing behavior is unchanged until a user opts in; the feature must be proven
  before the default flips, and the CTX-44 "Reference only / parked" card must be updated once Read
  starts trimming via this path, or it becomes the next honesty defect.
- The `read_unblock` tag and `path_role` feature start accruing now, so the file-aware model (CTX-46)
  has a clean, labeled corpus to train on when the data is there.
- Claude Code first. The unblock needs narration, and Cursor does not always persist a transcript we
  can read, so most Cursor working reads stay protected. That is acceptable: it only means less Read
  savings there, never a wrong trim.
