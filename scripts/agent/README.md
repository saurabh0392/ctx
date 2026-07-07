# Agent fix pipeline

A Claude-driven pipeline that picks up a triaged issue, implements a fix, checks it for hallucination
and against the deterministic gates, opens a **draft** PR, and notifies the reporter. It never merges.
A human is in the loop twice: applying the `agent-fix` label to start it, and reviewing the PR to ship
it.

## Flow (scripts/agent/run.mjs)

1. Fetch the issue. **Triage** with a cheap model: fixable, confidence, scope, files. Low confidence or
   a broad change stops here with a `needs-human` comment, so a bad candidate costs one small call.
2. Branch `agent/issue-N`.
3. Up to **3 attempts**: implement (strong model) -> **validate** (cheap adversarial model: invented
   APIs, does it actually fix it, regressions) -> gates (`cargo test`, then the coherence suite). Any
   failure feeds back into the next attempt with the real error output. A diff over 500 lines is
   handed to a human instead.
4. On success: commit, push, open a **draft** PR linking the issue, label `agent-authored`, comment
   `@reporter` that it is pending review. On giving up after 3: `needs-human` and a comment.

## What you provide

Two repo secrets (Settings -> Secrets and variables -> Actions):

- **`ANTHROPIC_API_KEY`** from console.anthropic.com. The pipeline bills Anthropic per run; the model
  tiering and caps below keep it bounded.
- **`AGENT_GH_TOKEN`**, a fine-grained PAT on this repo with **Contents: write, Pull requests: write,
  Issues: write**. A PAT (not the built-in `GITHUB_TOKEN`) is required so the PR it opens actually
  triggers CI. Scope it to this one repo.

And **branch protection on `main`**: require a pull request review and require the CI checks to pass.
That is what makes an agent PR safe: it cannot merge without your review and green CI.

Labels used: `agent-fix` (start), `agent-authored`, `needs-human`.

## Trigger

`on: issues: types: [labeled]` fires when `agent-fix` is added. `workflow_dispatch` with an issue
number runs it by hand (for testing). A workflow only triggers from its version on the default branch,
so this file must be on `main` before the label does anything.

## Cost and quality knobs (env, with defaults)

| Knob | Default | Purpose |
|---|---|---|
| `MAX_ATTEMPTS` | 3 | hard cap on the implement/validate/gate loop |
| `MAX_DIFF_LINES` | 500 | over this, hand to a human instead of auto-PR |
| `MIN_CONFIDENCE` | 0.6 | triage bar to attempt a fix at all |
| `MODEL_CHEAP` | claude-sonnet-5 | triage + validation |
| `MODEL_STRONG` | claude-opus-4-8 | implementation only |
| job `timeout-minutes` | 20 | wall-clock ceiling |
| `concurrency: agent-fix` | one | no fan-out |

Token spend is summed per run and posted on the PR / issue comment, so cost is visible per issue.

## Honest limits

- This runs only in CI and has not executed against a live issue yet; the first `agent-fix` label (or a
  `workflow_dispatch`) is its real test. Watch that first run.
- ctx is registered as an MCP for the agent on a best-effort basis, so triage can use ctx context; the
  pipeline still works if that registration is skipped (the agent falls back to Read/Grep).
- The gates are only as good as the coherence suite and tests. Broaden them as the product grows.
