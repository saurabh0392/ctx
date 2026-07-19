# Behavioral coherence checks

Read-only tools like `fitcheck` judge how a screen *reads*. They cannot see whether a button actually
does something, whether the same number agrees on two screens, or whether a tool is classified the
same way everywhere it appears. Every recent dashboard regression lived in exactly that blind spot:

- "Put on trial does nothing" (a held tool rendered a button the backend refuses)
- held tools shown as trim candidates on See while parked out of trimming on Save
- an earned tool's clean-test control run labelled "still only watching" in the activity feed
- two "reclaimed so far" figures disagreeing one click apart

This suite drives a **live** dashboard and asserts those cross-cutting properties. It runs against an
isolated copy of your `~/.ctx`, so it can click real mutation controls (trial, prune, preset) without
touching your data.

## Run it

```
cd scripts/coherence && npm i          # once: installs playwright-core (uses system Chrome)
scripts/coherence/coherence.sh --build # build, launch isolated, check, tear down
```

Exit code is 0 if every invariant holds, 1 if any fail, 2 on a setup problem (no binary, no
playwright). `--build` compiles the release binary first so the check runs against the code you are
about to ship. The dashboard HTML is compiled into the binary, so a rebuild is required to see edits.

## What it checks

Grouped by category in `invariants.mjs`. Each is a live assertion, not a snapshot.

- **interaction**: every re-rendering mutation control (trial, prune, preset) produces a visible
  change, and a held tool is never offered a trial.
- **classification**: the held set is identical on Save and in the API, and See marks held tools
  instead of listing them as trim candidates.
- **numeric**: the reclaimable total excludes held tools, Home's reclaimed equals See's output plus
  input components, ladder rung counts match the tools under them, and no headline label shows two
  different values across views.
- **label**: an earned tool is never labelled "only watching" (control holdouts read as control).

### Honest coverage limits

- The interaction check covers controls where "clicked and nothing re-rendered" is unambiguously a
  bug. It deliberately skips download (`exportDb`), destructive-confirm (`purgePrompts`, `deleteData`),
  and inline-field (`saveBudget`) actions, which need their own targeted checks.
- It exercises Home, See, Save, and Settings. The bill, toolbill, compaction, loop, and surfaces views
  are not covered yet. Add invariants as those surfaces grow.

## Wired as a hook

`.git/hooks/pre-push` runs this suite and blocks the push if an invariant fails. It soft-skips if
playwright is not installed, so a fresh clone can still push. `scripts/deploy.sh` runs it before the
codesign and launchd deploy, so a failing invariant never reaches port 8789.

Skip in an emergency with `git push --no-verify` or `SKIP_COHERENCE=1`.

## Fitcheck is a local PR gate

Fitcheck is intentionally not a GitHub Actions job. From a clean checkout at the exact PR head, run:

```bash
make pr-fitcheck PR=<number>
```

`scripts/pr-fitcheck.sh` runs `fitcheck-local.sh` with the developer's local Claude Code login (or
an optional `ANTHROPIC_API_KEY`) and posts a `Local Fitcheck` status on that commit. The main-branch
ruleset requires that status before merge. A changed PR head has no passing status until Fitcheck is
rerun, and setup/auth failures fail closed.
