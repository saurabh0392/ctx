#!/usr/bin/env bash
# Quick local smoke for the native hook path (no MITM proxy).
# Full coverage: cargo test (see README).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if [[ -n "${CTX:-}" ]]; then
  CTX_BIN="$CTX"
elif [[ -x "$SCRIPT_DIR/target/release/ctx" ]]; then
  CTX_BIN="$SCRIPT_DIR/target/release/ctx"
elif [[ -x "$SCRIPT_DIR/target/debug/ctx" ]]; then
  CTX_BIN="$SCRIPT_DIR/target/debug/ctx"
else
  CTX_BIN="$(command -v ctx || true)"
fi

if [[ -z "$CTX_BIN" || ! -x "$CTX_BIN" ]]; then
  echo "ctx binary not found. Run: cargo build"
  exit 1
fi

CTX_TMP="$(mktemp -d)"
export CTX_HOME="$CTX_TMP"
trap 'rm -rf "$CTX_TMP"' EXIT

cat > "$CTX_HOME/config.toml" <<'TOML'
active_profile = "all"
inject_enabled = false
coaching_enabled = false
auto_profile_enabled = false
adaptive_prefix_enabled = false
TOML

echo "Using: $CTX_BIN"
echo "CTX_HOME: $CTX_HOME"
echo ""

echo "--- hook user-prompt-submit ---"
printf '%s\n' '{"cwd":"/tmp","prompt":"ctx test.sh smoke","session_id":"test-sh-1"}' \
  | CTX_HOME="$CTX_HOME" "$CTX_BIN" hook user-prompt-submit >/dev/null

ROWS=$("$CTX_BIN" status 2>&1 | head -1 || true)
echo "status: $ROWS"

if command -v sqlite3 &>/dev/null && [[ -f "$CTX_HOME/ctx.db" ]]; then
  N=$(sqlite3 "$CTX_HOME/ctx.db" "SELECT COUNT(*) FROM hook_traces;" 2>/dev/null || echo 0)
  if [[ "$N" -ge 1 ]]; then
    echo "hook_traces rows: $N (ok)"
  else
    echo "hook_traces empty (unexpected)" >&2
    exit 1
  fi
fi

echo ""
echo "--- cargo test (hook + power features) ---"
cargo test \
  --test hook_contract \
  --test ab_hook \
  --test journey_mode_switch \
  --test journey_subagent_costs \
  --test journey_socket \
  --test journey_self_tuning \
  --test dashboard_stitch_test \
  2>&1

echo ""
echo "Done. For full suite: cargo test"
