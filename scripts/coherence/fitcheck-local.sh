#!/usr/bin/env bash
# Run the fitcheck persona review on the developer's machine. This intentionally refuses to run in
# GitHub Actions: the PR gate is evaluated locally, then scripts/pr-fitcheck.sh publishes only the
# pass/fail commit status that the main-branch ruleset requires.
#
# Exit: 0 pass, 1 verdict below the bar, 2 setup/auth/output problem.
#
# The review runs against a rendered dashboard, not the source file. Reading dashboard.html only
# shows intent; the defects that reach users (a duplicated header, a control loose in a paragraph,
# ragged left edges) exist only after the JS builds the DOM. This script boots an isolated instance,
# screenshots every view, and hands the reviewer the images.
#
# Env:
#   FITCHECK_TARGET        default: a rendered isolated dashboard. Set to a path or URL to override.
#   FITCHECK_MIN_VERDICT   Rework | Iterate | Ship (default Ship: a change must clear the bar, not
#                          merely avoid catastrophe)
#   FITCHECK_COMPARE       prior report to diff against (default: newest under docs/fitcheck/)
#   FITCHECK_MODE          brief | full (default full)
#   FITCHECK_PORT          default 8797
#   FITCHECK_MODEL         default claude-opus-4-8
#   CLAUDE_BIN             default claude
#   ANTHROPIC_API_KEY      optional; Claude Code's local login is used when absent
set -uo pipefail

if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
  echo "fitcheck-local: refusing to run in GitHub Actions; run make pr-fitcheck locally"
  exit 2
fi

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MIN="${FITCHECK_MIN_VERDICT:-Ship}"
MODE="${FITCHECK_MODE:-full}"
MODEL="${FITCHECK_MODEL:-claude-opus-4-8}"
CLAUDE_BIN="${CLAUDE_BIN:-claude}"
PORT="${FITCHECK_PORT:-8797}"
BIN="$REPO/target/release/ctx"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/ctx-fitcheck.XXXXXX")"
# Screenshots land inside the repo (under target/, already gitignored) rather than a temp dir. The
# reviewer runs sandboxed to the repo, so a /var/folders path is unreadable to it and the run dies
# having rendered images nobody could open.
SHOTS="$REPO/target/fitcheck-shots"
REAL_HOME="${CTX_FIXTURE:-${CTX_HOME:-$HOME/.ctx}}"
LIVE="$WORK/home"
STAMP="$(date +%Y-%m-%d)"

DASH_PID=""
cleanup() {
  [[ -n "$DASH_PID" ]] && kill -9 "$DASH_PID" 2>/dev/null
  rm -rf "$WORK" 2>/dev/null
  return 0
}
trap cleanup EXIT

# Newest prior report, so every run reports movement instead of an absolute score with no memory.
COMPARE="${FITCHECK_COMPARE:-$(ls -t "$REPO"/docs/fitcheck/*.md 2>/dev/null | head -1)}"

command -v "$CLAUDE_BIN" >/dev/null 2>&1 || {
  echo "fitcheck-local: Claude Code CLI not found (${CLAUDE_BIN})"
  exit 2
}

# ── Render first ─────────────────────────────────────────────────────────────────────────────────
if [[ -n "${FITCHECK_TARGET:-}" ]]; then
  TARGET="$FITCHECK_TARGET"
  echo "fitcheck-local: using caller-supplied target ${TARGET} (no render)"
else
  [[ -x "$BIN" ]] || {
    echo "fitcheck-local: building release binary…"
    (cd "$REPO" && cargo build --release >/dev/null 2>&1)
  }
  [[ -x "$BIN" ]] || { echo "fitcheck-local: no binary at $BIN"; exit 2; }

  # Isolated CTX_HOME so the review never reads or mutates the developer's real state.
  rm -rf "$SHOTS"; mkdir -p "$SHOTS"
  cp -Rc "$REAL_HOME" "$LIVE" 2>/dev/null || cp -R "$REAL_HOME" "$LIVE" 2>/dev/null || true
  CTX_HOME="$LIVE" "$BIN" dashboard --port "$PORT" --no-open >"$WORK/dash.log" 2>&1 &
  DASH_PID=$!
  for _ in $(seq 1 40); do
    [[ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/" 2>/dev/null)" == "200" ]] && break
    sleep 0.5
  done
  [[ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/" 2>/dev/null)" == "200" ]] || {
    echo "fitcheck-local: dashboard did not come up on :$PORT"; sed -n '1,20p' "$WORK/dash.log"; exit 2
  }
  echo "fitcheck-local: rendering views…"
  SHOT_LOG="$(cd "$REPO/scripts/coherence" && node shoot.mjs "http://127.0.0.1:$PORT" "$SHOTS" home save see settings 2>&1)" || {
    echo "fitcheck-local: screenshot step failed"; echo "$SHOT_LOG"; exit 2
  }
  echo "$SHOT_LOG"

  # Alex, the first-run evaluator, only ever sees the cold start, so a shot set of populated views
  # leaves his most important dimension unscoreable. Boot a second dashboard on an empty CTX_HOME
  # and capture that too.
  EMPTY_HOME="$WORK/empty"; mkdir -p "$EMPTY_HOME"
  EMPTY_PORT=$((PORT + 1))
  CTX_HOME="$EMPTY_HOME" "$BIN" dashboard --port "$EMPTY_PORT" --no-open >"$WORK/dash-empty.log" 2>&1 &
  EMPTY_PID=$!
  for _ in $(seq 1 40); do
    [[ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$EMPTY_PORT/" 2>/dev/null)" == "200" ]] && break
    sleep 0.5
  done
  if [[ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$EMPTY_PORT/" 2>/dev/null)" == "200" ]]; then
    (cd "$REPO/scripts/coherence" && node shoot.mjs "http://127.0.0.1:$EMPTY_PORT" "$SHOTS/empty" home save see settings 2>&1) || true
    echo "fitcheck-local: captured the empty first-run state too"
  else
    echo "fitcheck-local: could not boot an empty-state dashboard; first-run render not captured"
  fi
  kill -9 "$EMPTY_PID" 2>/dev/null || true

  TARGET="http://127.0.0.1:$PORT (live) with rendered screenshots in target/fitcheck-shots, and the cold first-run state in target/fitcheck-shots/empty"
fi

COMPARE_LINE=""
[[ -n "$COMPARE" && -f "$COMPARE" ]] && COMPARE_LINE="Compare against the prior report at ${COMPARE} and report per-persona and per-dimension movement. A regression on ANY persona blocks a ship even if the overall rose. "

PROMPT="Read and follow .claude/skills/fitcheck/SKILL.md against the target ${TARGET}, scope all, mode ${MODE}. \
Read every PNG in target/fitcheck-shots, including target/fitcheck-shots/empty (the cold first-run \
state, which is what Alex sees), with the Read tool before scoring; those are the rendered views and \
they are the primary evidence. Score the Visual execution dimension from the images alone. Also read \
src/dashboard.html for structure and empty states. ${COMPARE_LINE}\
Do the full persona walkthrough and score every dimension per rubric.md, including the empty state. \
Save the report to docs/fitcheck/${STAMP}-<target-slug>.md as the skill instructs; make no other edits. \
Be honest, this gates a merge. As the very last line of your reply, print exactly one machine line: \
FITCHECK verdict=<Ship|Iterate|Rework> overall=<number> coherence=<number>"

echo "fitcheck-local: reviewing (minimum verdict: ${MIN}, mode: ${MODE})…"
OUT="$(cd "$REPO" && "$CLAUDE_BIN" -p "$PROMPT" \
  --model "$MODEL" \
  --tools "Read,Grep,Glob,Write" \
  --permission-mode acceptEdits \
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
