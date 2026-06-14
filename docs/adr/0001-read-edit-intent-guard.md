# 0001. Read trimming is gated by an edit-intent guard

- Status: accepted
- Date: 2026-06-07
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-8

## Context

During the live Read trim trial (the SAU-150 / CTX-2 before-after mechanism), ctx trimmed a
Read of `FullConversation.tsx` in a real Claude Code session. The agent needed to edit that
file, the trim hid the part it needed, and the agent worked around ctx by re-reading the file
in smaller chunks. The trial was stopped immediately per the pre-agreed stop criteria.

Two facts make this structural, not a one-off:

1. In Claude Code the canonical edit flow is read-before-edit. A Read of a project source file
   is very often the precursor to an Edit of that same file. So a Read that looks "large" is not
   safe to trim just because it is large.
2. Read had the worst baseline of any tool (~29% re-reads). The size threshold alone is the
   wrong trigger for Read, and the fail-closed gate was right to distrust it.

We need a guard that lets Read be re-trialed without re-creating this harm, and we need it to be
honest: the comparison that decides whether Read "earns" trimming must not be confounded by the
guard.

## Decision

Read trimming is eligible only for **reference reads**: files the agent is not positioned to
edit. Working reads of editable files inside the active project are never trimmed, and the guard
holds even when the tool is under a deliberate trial or has passed the activation gate.

Phase 1 (this change) is a pure, static classifier, `compress::edit_intent::read_is_trim_eligible(file_path, cwd)`:

- Eligible (safe to trim) when the file is under a vendored / generated / build path
  (`node_modules/`, `target/`, `dist/`, `build/`, `.next/`, `vendor/`, `.venv/`, `site-packages/`,
  `__pycache__/`, `.git/`, `coverage/`, `out/`, `.cargo/`, `.rustup/`), or is a lockfile /
  minified / source-map / checksum artifact, or is an absolute path that resolves outside the
  project root (`cwd`).
- Protected (never trimmed) otherwise, including when the file path is unknown. Anything inside
  the project that is not a generated artifact is treated as an edit target.

The guard is controlled by `compress_read_edit_guard` (default on). It is wired into
`agent::decide`: for a `read`-kind result, `apply` is forced false when the guard is on and the
read is not trim-eligible. This sits in front of both the trial path and the normal
preset-plus-activation path, so a trial can never re-create the observed harm.

## Alternatives considered

- **Keep the size-only trigger and stop trialing Read.** Rejected: abandons a real token sink
  (large reference reads) and gives up on Read forever without a reason rooted in the data.
- **Predict edit-intent from the model's stated plan / next action.** Rejected for Phase 1: the
  PostToolUse hook is synchronous and cannot see the future turn. We can approximate intent with
  session history (Phase 2) but not with the current turn alone.
- **Reactive-only working set (protect a file once it is edited).** Necessary but insufficient on
  its own: it cannot protect the first read of a file before any edit, which is exactly the
  observed harm. Kept as Phase 2 to add precision on top of the static classifier.
- **Trim only the regions of a file far from where the agent is working.** Rejected for now: ctx
  does not reliably know the active region at read time, and a wrong guess re-creates the harm.

## Consequences

- Read trimming narrows to reference material (dependencies, generated code, lockfiles, files
  outside the repo). This is a deliberate, honest reduction in scope: we only trim Reads we can
  defend.
- The `FullConversation.tsx` class of harm is prevented by construction: project source is never
  trimmed in Phase 1.
- Measurement caveat (tracked for Phase 2): once the guard is on, the trimmed (`applied=1`)
  population is only reference reads. The Proof before-after for Read must be computed within the
  trim-eligible stratum, otherwise the baseline (all would-trim reads) and the trimmed arm are not
  comparable and the verdict is confounded. Until that stratification lands, Read proof numbers
  must be read as "reference reads only".
- `compress_read_edit_guard` defaults on. Turning it off is an experiment knob (to measure how
  much harm the guard prevents), not a normal operating mode.
- Phase 2 work: maintain a per-session working set (files touched by Edit/Write/MultiEdit and
  files read more than once) and protect those reads too; add the proof stratification above.
