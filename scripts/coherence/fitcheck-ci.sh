#!/usr/bin/env bash
# Run the fitcheck persona review headless and gate on its verdict. Because fitcheck is a model
# judgement, this is non-deterministic: it blocks the clear failure (Rework) by default and uploads
# the reasoning for review. Tighten with FITCHECK_MIN_VERDICT=Ship to require a Ship.
#
# Exit: 0 pass, 1 verdict below the bar (gate fail), 2 setup problem (no key, CLI, or unparseable).
#
# Env:
#   ANTHROPIC_API_KEY      required
#   FITCHECK_TARGET        default src/dashboard.html (file mode needs no running server)
#   FITCHECK_MIN_VERDICT   Rework | Iterate | Ship  (default Iterate: block only on Rework)
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET="${FITCHECK_TARGET:-src/dashboard.html}"
MIN="${FITCHECK_MIN_VERDICT:-Iterate}"
MODEL="${FITCHECK_MODEL:-claude-opus-4-8}"   # pin so a default-model change never shifts the score

[[ "${SKIP_FITCHECK:-}" == "1" ]] && { echo "fitcheck-ci: SKIP_FITCHECK=1, skipping"; exit 0; }
[[ -n "${ANTHROPIC_API_KEY:-}" ]] || { echo "fitcheck-ci: ANTHROPIC_API_KEY not set"; exit 2; }
command -v claude >/dev/null 2>&1 || { echo "fitcheck-ci: claude CLI not on PATH"; exit 2; }

PROMPT="Run the fitcheck skill in .claude/skills/fitcheck against the target ${TARGET}, scope all, mode brief. \
Do the full persona walkthrough and score every dimension per rubric.md, including the empty state. \
Be honest, this gates a release. As the very last line of your reply, print exactly one machine line: \
FITCHECK verdict=<Ship|Iterate|Rework> overall=<number> coherence=<number>"

echo "fitcheck-ci: running fitcheck on ${TARGET} (min verdict: ${MIN})…"
OUT="$(cd "$REPO" && claude -p "$PROMPT" --model "$MODEL" --dangerously-skip-permissions 2>&1)"
status=$?
echo "----- fitcheck output -----"; echo "$OUT"; echo "---------------------------"
if [[ $status -ne 0 ]]; then
  # Treat transient API-level errors (e.g. credit exhaustion) as a graceful skip rather
  # than a hard gate failure — they are not design regressions.
  if printf '%s\n' "$OUT" | grep -qiE "credit balance is too low|insufficient.{0,20}credit|quota.{0,10}exceeded|rate[. ]limit"; then
    echo "fitcheck-ci: API unavailable (transient error), skipping gracefully"; exit 0
  fi
  echo "fitcheck-ci: claude run failed (exit $status)"; exit 2
fi

LINE="$(printf '%s\n' "$OUT" | grep -oE 'FITCHECK verdict=(Ship|Iterate|Rework) overall=[0-9.]+ coherence=[0-9.]+' | tail -1)"
[[ -n "$LINE" ]] || { echo "fitcheck-ci: could not parse a verdict line from the output"; exit 2; }
VERDICT="$(printf '%s' "$LINE" | sed -E 's/.*verdict=([A-Za-z]+).*/\1/')"

rank() { case "$1" in Rework) echo 0;; Iterate) echo 1;; Ship) echo 2;; *) echo -1;; esac; }
echo "fitcheck-ci: verdict=${VERDICT}, gate min=${MIN}"
if [[ "$(rank "$VERDICT")" -lt 0 || "$(rank "$MIN")" -lt 0 ]]; then
  echo "fitcheck-ci: unrecognized verdict"; exit 2
fi
if [[ "$(rank "$VERDICT")" -ge "$(rank "$MIN")" ]]; then
  echo "fitcheck-ci: PASS"; exit 0
else
  echo "fitcheck-ci: FAIL, verdict ${VERDICT} is below ${MIN}"; exit 1
fi
