#!/usr/bin/env bash
# Manual checklist after `ctx setup` + Claude Code restart (no live API calls here).
set -euo pipefail
echo "1. From repo root: cargo test --test proxy_test"
echo "2. Confirm ~/.claude/settings.json has HTTPS_PROXY and NODE_EXTRA_CA_CERTS"
echo "3. Confirm ~/.ctx/ca-cert.pem exists"
echo "4. Restart Claude Code; send a request; confirm no 429 from third-party API mode"
echo "5. Optional: curl -v --proxy http://127.0.0.1:8788 https://api.anthropic.com/ with CA trust"
