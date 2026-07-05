#!/usr/bin/env bash
# Run the behavioral coherence suite against an isolated copy of the local ctx state, so it can drive
# mutation controls (trial, prune, profile) without ever touching ~/.ctx. Exits non-zero on failure.
#
#   scripts/coherence/coherence.sh [--build]
#
# Wiring: call it before deploying, or from a git pre-push hook (see scripts/coherence/README.md).
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN="$REPO/target/release/ctx"
PORT="${COHERENCE_PORT:-8799}"
REAL_HOME="${CTX_HOME:-$HOME/.ctx}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/ctx-coherence.XXXXXX")"
LIVE="$WORK/home"
PRISTINE_CONFIG="$WORK/config.pristine.toml"

cleanup() {
  [[ -n "${DASH_PID:-}" ]] && kill -9 "$DASH_PID" 2>/dev/null || true
  rm -rf "$WORK" 2>/dev/null || true
}
trap cleanup EXIT

if [[ "${1:-}" == "--build" ]]; then
  echo "coherence: building release binary…"
  (cd "$REPO" && cargo build --release >/dev/null 2>&1)
fi
[[ -x "$BIN" ]] || { echo "coherence: no binary at $BIN (run with --build)"; exit 2; }

# Isolated CTX_HOME: APFS clone if available (instant), else plain copy. The dashboard reads config and
# db from CTX_HOME, so mutations from clicked controls land here, never in $REAL_HOME.
echo "coherence: cloning $REAL_HOME -> isolated home…"
cp -Rc "$REAL_HOME" "$LIVE" 2>/dev/null || cp -R "$REAL_HOME" "$LIVE"
cp "$LIVE/config.toml" "$PRISTINE_CONFIG"

echo "coherence: launching dashboard on :$PORT (isolated)…"
CTX_HOME="$LIVE" "$BIN" dashboard --port "$PORT" --no-open >"$WORK/dash.log" 2>&1 &
DASH_PID=$!
for _ in $(seq 1 40); do
  [[ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/" 2>/dev/null)" == "200" ]] && break
  sleep 0.5
done

# Resolve playwright-core: prefer a local install, else PW_CORE from the environment.
PW_LOCAL="$REPO/scripts/coherence/node_modules/playwright-core"
if [[ -d "$PW_LOCAL" ]]; then export PW_CORE="$PW_LOCAL"; fi
[[ -n "${PW_CORE:-}" ]] || { echo "coherence: playwright-core not found. Run 'npm i' in scripts/coherence, or set PW_CORE."; exit 2; }

SMOKE_BASE="http://127.0.0.1:$PORT" CTX_HOME_LIVE="$LIVE" CTX_PRISTINE_CONFIG="$PRISTINE_CONFIG" \
  node "$REPO/scripts/coherence/run.mjs"
