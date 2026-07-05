#!/usr/bin/env bash
# Coherence-gated deploy of the ctx dashboard to the live instance on :8789.
# Builds, runs the behavioral coherence suite against an isolated copy, and only if every invariant
# holds does it install + codesign + restart launchd. A failing invariant never reaches :8789.
#
#   scripts/deploy.sh                 # gated deploy
#   SKIP_COHERENCE=1 scripts/deploy.sh  # emergency bypass (discouraged)
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_SRC="$REPO/target/release/ctx"
BIN_DST="$HOME/.cargo/bin/ctx"

echo "deploy: building release…"
(cd "$REPO" && cargo build --release)

if [[ "${SKIP_COHERENCE:-}" == "1" ]]; then
  echo "deploy: SKIP_COHERENCE=1, skipping coherence gate"
else
  echo "deploy: coherence gate…"
  if ! bash "$REPO/scripts/coherence/coherence.sh"; then
    echo "deploy: BLOCKED, coherence failed. Not deploying. Bypass with SKIP_COHERENCE=1 if you must."
    exit 1
  fi
fi

echo "deploy: install + codesign + restart…"
cp "$BIN_SRC" "$BIN_DST"
codesign --force --sign - "$BIN_DST"        # mandatory: cp invalidates the signature, launchd refuses an unsigned binary
launchctl kickstart -k "gui/$(id -u)/com.ctx.dashboard"

echo "deploy: waiting for :8789…"
for _ in $(seq 1 40); do
  [[ "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8789/ 2>/dev/null)" == "200" ]] && { echo "deploy: live on :8789"; exit 0; }
  sleep 1
done
echo "deploy: :8789 did not come up in time (launchd throttling?). Re-run kickstart if needed."
exit 1
