# 0028. Honest "reference only" display state for guard-held read tools

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh (with CTO partner)

## Context

The loop-health view places each tool on the same ladder: watching, then testing real trims, then
earned. A tool in "watching" with enough baseline but no trimmed runs was shown as "ctx will start
trimming it on a few runs and finish the proof on its own."

That promise is false for the Read tool under a common usage pattern. The edit-intent guard (ADR
0001) refuses to trim a read of a file the agent might edit; Read only ever trims *reference* reads
(dependencies, vendored code, files outside the working tree). When a user's reads are mostly their
own project files, the guard holds every meaningful trim, so Read can never build a trimmed arm and
can never graduate, no matter how long it watches. On the live dev corpus (ctx self-dev rows
excluded) all 117 Read decisions had `applied = 0`; of the 49 with something to cut, 88 meaningful
reads were guard-held and the rest dropped a single line. So Read was correctly parked, but the card
read as if trimming were imminent. A false promise about your own data is a trust defect, not a
cosmetic one.

## Decision

Surface the guard's effect as a first-class, honest display state instead of pretending the tool is
mid-proof.

- The backend reports, per read-kind tool, how many reads the guard held whole
  (`read_guard_held`: clean corpus, `read_protected = true`, `lines_drop > 0`). Only read-kind tools
  set `read_protected`, so the field is absent for tools the guard does not touch (Bash, Grep, MCP).
- When a tool has a non-trivial guard-held count and zero trimmed runs, the dashboard treats it as
  "reference only / parked on purpose": a distinct badge, a short line, a long story, and a progress
  label that all explain it only trims reference reads, that the user's reads are files they are
  editing which ctx never trims, and that it is correctly parked rather than stuck.
- The "Put on trial" action is hidden for these tools, because a trial still runs through the guard
  and would collect nothing.
- The logic keys off the backend signal, not the literal tool name "Read", so any read-kind tool the
  guard protects is handled the same way.

## Alternatives considered

- **Copy-only softening for the tool named "Read".** Rejected: hardcoding one tool name is brittle
  and still guesses; the honest signal is whether the guard actually held this user's reads, which is
  data we already record.
- **Change the gate or burn-in so Read can graduate.** Rejected and out of scope: the guard is a
  deliberate safety decision (ADR 0001). The defect is the display overpromising, not the guard.
- **Drop Read from the loop-health view entirely when guard-held.** Rejected: hiding it would leave
  users wondering why a tool they see ctx watching never appears. Showing it with an honest "parked"
  state is more trustworthy than silence.

## Consequences

- The Read card now tells the truth on day one: it explains why it is not trimming and that this is
  correct, instead of promising a proof the guard will never let it finish.
- The honest state is general: if a future read-kind tool is guard-protected on a user's work, it
  reads the same way without code changes.
- A read tool that does have reference reads (heavy dependency or vendored-code browsing) still moves
  through watching to testing to earned normally, because the guard-held count alone does not park
  it; it parks only when there are also zero trimmed runs.
- One more backend query runs per Context view load. It is a single grouped count over
  `compress_decisions` and is negligible next to the existing causal aggregation.
