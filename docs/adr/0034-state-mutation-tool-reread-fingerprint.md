# 0034. State-mutation tool re-read fingerprints

- Status: accepted
- Date: 2026-07-01
- Deciders: saurabh

## Context

TodoWrite (and similar session-state tools) had no command or path, so every decision used the bare tool name as `command_or_path`. The re-read join treated any later TodoWrite within 15 minutes as needing the prior output back. Agents call TodoWrite routinely to merge task updates, so the trimmed arm looked much worse than baseline even when trimming did not cause harm.

## Decision

1. Fingerprint TodoWrite and Task from a canonical hash of `merge` plus sorted `todos` (`ToolName:<fnv1a64>`).
2. In the re-read join, skip attribution when a legacy bare fingerprint (`command_or_path = tool_name`) would match another call of the same state-mutation tool.
3. Rejoin the corpus once on upgrade (`rejoin_outcome_labels_v5`).

## Alternatives considered

- Drop re-reads for TodoWrite entirely: too blunt; identical payload repeats are still a signal.
- Require an explicit user correction only: misses agent-driven re-fetch without a complaint.
- Train the model to ignore TodoWrite re-reads: does not fix the causal gate or dashboard proof.

## Consequences

- Routine todo updates no longer inflate harm on the trimmed arm.
- Legacy rows keep bare fingerprints but stop cross-counting re-reads after rejoin.
- New hook decisions get content fingerprints automatically.
- Other state tools can join `STATE_MUTATION_TOOL_NAMES` with the same pattern.
