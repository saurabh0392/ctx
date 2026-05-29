#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

make dashboard

cd e2e
if [[ ! -d node_modules ]]; then
  npm install
fi
npx playwright install chromium --with-deps 2>/dev/null || npx playwright install chromium
if [[ "${1:-}" == "--update-snapshots" ]]; then
  npx playwright test --update-snapshots
else
  npx playwright test "$@"
fi
