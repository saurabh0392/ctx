#!/usr/bin/env bash
# Install the coherence pre-push hook into this repo's .git/hooks.
set -euo pipefail
REPO="$(git rev-parse --show-toplevel)"
SRC="$REPO/scripts/coherence/pre-push"
DST="$REPO/.git/hooks/pre-push"
cp "$SRC" "$DST"
chmod +x "$DST"
echo "installed pre-push hook -> $DST"
