#!/usr/bin/env bash
# Isolated ctx + Claude CLI session. Does not touch ~/.claude/settings.json.
# Ctrl+C exits Claude; the proxy is stopped automatically.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PORT="${CTX_TEST_PORT:-9788}"

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
  echo "ctx binary not found. Run: cd \"$SCRIPT_DIR\" && cargo build"
  exit 1
fi

echo "Using: $CTX_BIN (proxy on :$PORT)"
"$CTX_BIN" proxy start --port "$PORT" &
PROXY_PID=$!
cleanup() {
  kill "$PROXY_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

sleep 1

echo ""
echo "--- ctx test session (ANTHROPIC_BASE_URL=http://127.0.0.1:$PORT) ---"
"$CTX_BIN" status 2>&1 | head -5 || true
echo ""

CLAUDE_BIN="$(command -v claude 2>/dev/null || true)"
if [[ -z "$CLAUDE_BIN" ]]; then
  # Look inside Cursor's bundled extension
  CLAUDE_BIN="$(ls -t "$HOME"/.cursor/extensions/anthropic.claude-code-*/resources/native-binary/claude 2>/dev/null | head -1 || true)"
fi
if [[ -z "$CLAUDE_BIN" || ! -x "$CLAUDE_BIN" ]]; then
  echo "claude CLI not found in PATH or Cursor extensions. Run manually:"
  echo "  ANTHROPIC_BASE_URL=http://127.0.0.1:$PORT <path-to-claude>"
  exit 0
fi

# Use an isolated config dir so the test CLI never modifies ~/.claude/settings.json
TEST_CONFIG_DIR="${TMPDIR:-/tmp}/ctx-test-claude-config"
mkdir -p "$TEST_CONFIG_DIR"
if [[ ! -f "$TEST_CONFIG_DIR/settings.json" ]]; then
  echo '{"permissions":{"allow":[]},"model":"sonnet"}' > "$TEST_CONFIG_DIR/settings.json"
fi

echo "Claude: $CLAUDE_BIN"
echo "Config: $TEST_CONFIG_DIR (isolated from ~/.claude)"
ANTHROPIC_BASE_URL="http://127.0.0.1:$PORT" CLAUDE_CONFIG_DIR="$TEST_CONFIG_DIR" exec "$CLAUDE_BIN" "$@"
