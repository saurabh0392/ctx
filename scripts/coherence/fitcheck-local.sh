#!/usr/bin/env bash
# Run the fitcheck persona review on the developer's machine. This intentionally refuses to run in
# GitHub Actions: the PR gate is evaluated locally, then scripts/pr-fitcheck.sh publishes only the
# pass/fail commit status that the main-branch ruleset requires.
#
# Exit: 0 pass, 1 verdict below the bar, 2 setup/auth/output problem.
#
# Env:
#   FITCHECK_TARGET        default src/dashboard.html
#   FITCHECK_MIN_VERDICT   Rework | Iterate | Ship (default Iterate: block only on Rework)
#   FITCHECK_MODEL         default claude-opus-4-8
#   CLAUDE_BIN             default claude
#   ANTHROPIC_API_KEY      optional; Claude Code's local login is used when absent
set -uo pipefail

if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
  echo "fitcheck-local: refusing to run in GitHub Actions; run make pr-fitcheck locally"
  exit 2
fi

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET="${FITCHECK_TARGET:-src/dashboard.html}"
MIN="${FITCHECK_MIN_VERDICT:-Iterate}"
MODEL="${FITCHECK_MODEL:-claude-opus-4-8}"
CLAUDE_BIN="${CLAUDE_BIN:-claude}"

command -v "$CLAUDE_BIN" >/dev/null 2>&1 || {
  echo "fitcheck-local: Claude Code CLI not found (${CLAUDE_BIN})"
  exit 2
}

PROMPT="Read and follow .claude/skills/fitcheck/SKILL.md against the target ${TARGET}, scope all, mode brief. \
Do the full persona walkthrough and score every dimension per rubric.md, including the empty state. \
This is a read-only gate: do not create, edit, delete, or save any file; return the report only in your response. \
Be honest, this gates a merge. As the very last line of your reply, print exactly one machine line: \
FITCHECK verdict=<Ship|Iterate|Rework> overall=<number> coherence=<number>"

echo "fitcheck-local: running on ${TARGET} (minimum verdict: ${MIN})…"
OUT="$(cd "$REPO" && "$CLAUDE_BIN" -p "$PROMPT" \
  --model "$MODEL" \
  --tools "Read,Grep,Glob" \
  --permission-mode dontAsk \
  --strict-mcp-config \
  --mcp-config '{"mcpServers":{}}' \
  --no-chrome \
  --no-session-persistence 2>&1)"
status=$?
echo "----- fitcheck output -----"
echo "$OUT"
echo "---------------------------"
[[ $status -eq 0 ]] || {
  echo "fitcheck-local: Claude Code failed (exit ${status}); check the local login or API key"
  exit 2
}

LINE="$(printf '%s\n' "$OUT" | grep -oE 'FITCHECK verdict=(Ship|Iterate|Rework) overall=[0-9.]+ coherence=[0-9.]+' | tail -1)"
[[ -n "$LINE" ]] || {
  echo "fitcheck-local: could not parse a verdict line"
  exit 2
}
VERDICT="$(printf '%s' "$LINE" | sed -E 's/.*verdict=([A-Za-z]+).*/\1/')"

rank() {
  case "$1" in
    Rework) echo 0 ;;
    Iterate) echo 1 ;;
    Ship) echo 2 ;;
    *) echo -1 ;;
  esac
}

echo "fitcheck-local: verdict=${VERDICT}, minimum=${MIN}"
if [[ "$(rank "$VERDICT")" -lt 0 || "$(rank "$MIN")" -lt 0 ]]; then
  echo "fitcheck-local: unrecognized verdict"
  exit 2
fi
if [[ "$(rank "$VERDICT")" -ge "$(rank "$MIN")" ]]; then
  echo "fitcheck-local: PASS"
  exit 0
fi

echo "fitcheck-local: FAIL, verdict ${VERDICT} is below ${MIN}"
exit 1
