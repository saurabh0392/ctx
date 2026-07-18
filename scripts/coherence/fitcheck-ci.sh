#!/usr/bin/env bash
# Run the fitcheck persona review headless and gate on its verdict. Because fitcheck is a model
# judgement, this is non-deterministic: it blocks the clear failure (Rework) by default and uploads
# the reasoning for review. Tighten with FITCHECK_MIN_VERDICT=Ship to require a Ship.
#
# Exit: 0 pass, 1 verdict below the bar (gate fail), 2 setup problem (no token, unparseable),
#       3 Copilot access denied — treated as a graceful skip (no key, fork, or plan without Copilot).
#
# Env:
#   GITHUB_TOKEN (or GH_TOKEN)   required — the standard Actions token works when Copilot is
#                                 enabled for the repository's organization.
#   FITCHECK_TARGET        default src/dashboard.html (file mode needs no running server)
#   FITCHECK_MIN_VERDICT   Rework | Iterate | Ship  (default Iterate: block only on Rework)
#   FITCHECK_MODEL         model passed to the Copilot API (default gpt-4o)
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET="${FITCHECK_TARGET:-src/dashboard.html}"
MIN="${FITCHECK_MIN_VERDICT:-Iterate}"

[[ "${SKIP_FITCHECK:-}" == "1" ]] && { echo "fitcheck-ci: SKIP_FITCHECK=1, skipping"; exit 0; }

TOKEN="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
[[ -n "${TOKEN:-}" ]] || { echo "fitcheck-ci: GITHUB_TOKEN not set"; exit 2; }

echo "fitcheck-ci: running fitcheck on ${TARGET} (min verdict: ${MIN})…"
OUT="$(cd "$REPO" && node scripts/coherence/fitcheck-engine.mjs "$TARGET" 2>&1)"
status=$?
echo "----- fitcheck output -----"; echo "$OUT"; echo "---------------------------"

if [[ $status -eq 3 ]]; then
  echo "fitcheck-ci: Copilot API access denied (no Copilot subscription or fork token), skipping gracefully"
  exit 0
fi
if [[ $status -ne 0 ]]; then
  echo "fitcheck-ci: fitcheck-engine failed (exit $status)"; exit 2
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
