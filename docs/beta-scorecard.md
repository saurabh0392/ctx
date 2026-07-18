# CTX v0.5 beta scorecard

## Cohort

- Three small engineering teams.
- Five individual power users.
- macOS + Claude Code is the supported path.
- Cursor and Windows feedback is welcome but scored separately as experimental.

Recruit in two waves: one team plus two power users, then the remaining two teams plus three users
after the first support issues are fixed.

## Product gates

- 10 successful installs.
- 7 installs populate a Context Bill.
- 5 participants are active in week two.
- At least half report learning something material about their context.
- At least 3 take an insight-driven action.
- Every attempted rewind returns the original byte for byte.
- No unrecoverable tool output or hidden capability incident.

## Commercial gate

At least two of three team leads answer yes or maybe to a follow-up at the hypothesis price of
`$25/developer/month`. This wave records interest only; it does not collect payment.

## Weekly readout

Download the private `checkins/` objects into an operator-only directory, then run:

```bash
node scripts/beta-summary.mjs <checkin-directory>
```

The summary never prints participant IDs or free text. Because day-7 and day-21 snapshots are
cumulative, it uses only the latest valid check-in per participant for outcome totals. `checkins`
is the raw submission count; `participants` is the deduplicated cohort count. Team-lead pricing
interest and rewind integrity remain manual interview/incident checks rather than inferred product
events.

## Stop-ship conditions

- Data loss or configuration corruption.
- A hidden agent capability the user did not explicitly prune.
- A trim whose original cannot be recovered.
- An undisclosed network send.
- Update checksum failure or an updater that cannot preserve the old binary.

Immediate controls: `ctx context off`, roster revocation in SSM, and republishing the prior release
manifest.

## Interview prompts

1. What, if anything, did the Context Bill show that your agent did not?
2. Did you change a workflow, tool menu, or autopilot setting because of it?
3. When did you trust or distrust the rewind/evidence model?
4. Who on your team would own this problem?
5. Would a team lead consider continuing at $25 per developer per month? Why or why not?

## Decision memo at the end of the wave

Choose one: continue into a paid team-governance pilot, narrow the target segment, return to a free
portfolio/tool project, or stop. Include the complete scorecard and disconfirming evidence.
