#!/usr/bin/env bash
# Smoke tests for ctx proxy gates in a fully isolated environment.
# No real Anthropic API calls -- a mock upstream captures each request body.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROXY_PORT="${CTX_SMOKE_PROXY_PORT:-9877}"
MOCK_PORT="${CTX_SMOKE_MOCK_PORT:-9878}"
CTX_TMP="$(mktemp -d)"
CAPTURE_FILE="$CTX_TMP/captured.json"
REQ_FILE="$CTX_TMP/request.json"
PROXY_LOG="$CTX_TMP/proxy.log"
MOCK_PID=""
PROXY_PID=""
PASS=0
FAIL=0

cleanup() {
  [[ -n "$PROXY_PID" ]] && kill "$PROXY_PID" 2>/dev/null || true
  [[ -n "$MOCK_PID"  ]] && kill "$MOCK_PID"  2>/dev/null || true
  rm -rf "$CTX_TMP"
}
trap cleanup EXIT INT TERM

red()   { printf '\033[31m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
bold()  { printf '\033[1m%s\033[0m\n' "$*"; }

pass() { green "  PASS: $1"; PASS=$(( PASS + 1 )); }
fail() { red   "  FAIL: $1"; FAIL=$(( FAIL + 1 )); }

# All assertions read directly from CAPTURE_FILE to avoid large-body variable issues.
assert_contains() {
  local label="$1" needle="$2"
  if grep -qF "$needle" "$CAPTURE_FILE" 2>/dev/null; then
    pass "$label"
  else
    fail "$label -- expected: $needle"
    python3 -c "
import json, sys
try:
    d = json.load(open('$CAPTURE_FILE'))
    s = d.get('system', '<missing>')
    print('    system:', str(s)[:400])
except Exception as e:
    print('    (parse error:', e, ')')
" 2>/dev/null || true
  fi
}

assert_not_contains() {
  local label="$1" needle="$2"
  if grep -qF "$needle" "$CAPTURE_FILE" 2>/dev/null; then
    fail "$label -- unexpected: $needle"
  else
    pass "$label"
  fi
}

assert_tool_count() {
  local label="$1" expected="$2"
  local actual
  actual=$(python3 -c "
import json
d = json.load(open('$CAPTURE_FILE'))
print(len(d.get('tools', [])))
" 2>/dev/null || echo "-1")
  if [[ "$actual" == "$expected" ]]; then
    pass "$label (tools=$actual)"
  else
    fail "$label -- expected $expected tools, got $actual"
  fi
}

# ---- Find ctx binary ----
if [[ -x "$SCRIPT_DIR/target/release/ctx" ]]; then
  CTX_BIN="$SCRIPT_DIR/target/release/ctx"
elif [[ -x "$SCRIPT_DIR/target/debug/ctx" ]]; then
  CTX_BIN="$SCRIPT_DIR/target/debug/ctx"
elif command -v ctx &>/dev/null; then
  CTX_BIN="$(command -v ctx)"
else
  echo "ctx binary not found. Run: cd $SCRIPT_DIR && cargo build"
  exit 1
fi
bold "Using ctx: $CTX_BIN"

# ---- Start mock upstream that captures each request body ----
python3 - "$CAPTURE_FILE" "$MOCK_PORT" <<'PYEOF' &
import sys, json
from http.server import HTTPServer, BaseHTTPRequestHandler

CAPTURE_FILE = sys.argv[1]
PORT = int(sys.argv[2])
RESPONSE = json.dumps({
    "id": "msg_smoke", "type": "message", "role": "assistant",
    "content": [{"type": "text", "text": "ok"}],
    "model": "claude-sonnet-4-6", "stop_reason": "end_turn",
    "stop_sequence": None, "usage": {"input_tokens": 5, "output_tokens": 2}
}).encode()

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get('Content-Length', 0))
        body = self.rfile.read(length)
        with open(CAPTURE_FILE, 'wb') as f:
            f.write(body)
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(RESPONSE)))
        self.end_headers()
        self.wfile.write(RESPONSE)
    def log_message(self, *args): pass

HTTPServer(('127.0.0.1', PORT), Handler).serve_forever()
PYEOF
MOCK_PID=$!
sleep 0.3

# ---- Helpers ----
write_config() {
  local profile="${1:-carrier}" auto="${2:-false}" inject="${3:-false}"
  cat > "$CTX_TMP/config.toml" <<TOML
active_profile = "$profile"
auto_profile_enabled = $auto
inject_enabled = $inject
TOML
}

restart_proxy() {
  [[ -n "$PROXY_PID" ]] && { kill "$PROXY_PID" 2>/dev/null || true; sleep 0.2; }
  CTX_HOME="$CTX_TMP" "$CTX_BIN" proxy start \
    --port "$PROXY_PORT" \
    --upstream "http://127.0.0.1:$MOCK_PORT" \
    >"$PROXY_LOG" 2>&1 &
  PROXY_PID=$!
  sleep 0.5
}

# Write request JSON to REQ_FILE and POST it; CAPTURE_FILE gets what upstream received.
send_request() {
  echo "$1" > "$REQ_FILE"
  rm -f "$CAPTURE_FILE"
  curl -s -o /dev/null \
    -X POST "http://127.0.0.1:$PROXY_PORT/v1/messages" \
    -H "Content-Type: application/json" \
    -H "anthropic-version: 2023-06-01" \
    -H "x-api-key: test" \
    --data-binary "@$REQ_FILE"
  sleep 0.15
}

send_request_file() {
  rm -f "$CAPTURE_FILE"
  curl -s -o /dev/null \
    -X POST "http://127.0.0.1:$PROXY_PORT/v1/messages" \
    -H "Content-Type: application/json" \
    -H "anthropic-version: 2023-06-01" \
    -H "x-api-key: test" \
    --data-binary "@$1"
  sleep 0.15
}

TOOL_SLACK='{"name":"mcp__claude_ai_Slack__send","description":"slack","input_schema":{"type":"object","properties":{}}}'
TOOL_FIGMA='{"name":"mcp__claude_ai_Figma__get_design","description":"figma","input_schema":{"type":"object","properties":{}}}'
TOOL_ATLA='{"name":"mcp__claude_ai_Atlassian__search","description":"jira","input_schema":{"type":"object","properties":{}}}'

# ============================================================
bold ""
bold "=== Gate 2: MCP tool schema filtering ==="
write_config "carrier" "false" "false"
restart_proxy

send_request "{
  \"model\":\"claude-sonnet-4-6\",\"max_tokens\":10,
  \"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],
  \"tools\":[$TOOL_SLACK,$TOOL_FIGMA,$TOOL_ATLA]
}"
assert_tool_count "Gate 2: 2 tools remain after Figma stripped" "2"
assert_not_contains "Gate 2: Figma absent at upstream" "mcp__claude_ai_Figma__get_design"
assert_contains     "Gate 2: Slack present at upstream" "mcp__claude_ai_Slack__send"

# ============================================================
bold ""
bold "=== Gate 1: auto-profile from system prompt ==="
write_config "all" "true" "false"
restart_proxy

send_request "{
  \"model\":\"claude-sonnet-4-6\",\"max_tokens\":10,
  \"system\":\"Primary working directory: /Users/alice/Documents/carrier-integrations-platform\nYou are Claude Code.\",
  \"messages\":[{\"role\":\"user\",\"content\":\"hi from carrier cwd\"}],
  \"tools\":[$TOOL_SLACK,$TOOL_FIGMA]
}"
assert_tool_count "Gate 1: auto-selected carrier strips Figma, 1 tool remains" "1"
assert_not_contains "Gate 1: Figma absent (auto-profile fired)" "mcp__claude_ai_Figma__get_design"

# Non-carrier CWD: auto-profile should stay on "all" -> no filtering
send_request "{
  \"model\":\"claude-sonnet-4-6\",\"max_tokens\":10,
  \"system\":\"Primary working directory: /Users/alice/Documents/some-unrelated-repo\",
  \"messages\":[{\"role\":\"user\",\"content\":\"hi from random cwd\"}],
  \"tools\":[$TOOL_SLACK,$TOOL_FIGMA]
}"
assert_tool_count "Gate 1: unknown CWD stays on all profile, 2 tools pass through" "2"

# ============================================================
bold ""
bold "=== Gate 3: system prompt injection ==="
echo "[ctx-smoke-gate3-prefix]" > "$CTX_TMP/system_prefix.md"
write_config "all" "false" "true"
restart_proxy

send_request "{
  \"model\":\"claude-sonnet-4-6\",\"max_tokens\":10,
  \"system\":\"original system prompt\",
  \"messages\":[{\"role\":\"user\",\"content\":\"hi gate3 inject\"}]
}"
assert_contains "Gate 3: prefix present in forwarded request" "[ctx-smoke-gate3-prefix]"
assert_contains "Gate 3: original system preserved" "original system prompt"

# Disable inject by removing the prefix file
rm -f "$CTX_TMP/system_prefix.md"
send_request "{
  \"model\":\"claude-sonnet-4-6\",\"max_tokens\":10,
  \"system\":\"original system prompt\",
  \"messages\":[{\"role\":\"user\",\"content\":\"hi gate3 no-inject\"}]
}"
assert_not_contains "Gate 3: prefix absent when file removed" "[ctx-smoke-gate3-prefix]"

# ============================================================
bold ""
bold "=== Gate 4: per-request coaching signal ==="
write_config "all" "false" "false"
restart_proxy

# detect_reask needs >=3 user turns and compares current vs 2+ turns back.
# This is the same content used in the coach unit test (reask_fires_on_high_overlap).
send_request "{
  \"model\":\"claude-sonnet-4-6\",\"max_tokens\":10,
  \"messages\":[
    {\"role\":\"user\",    \"content\":\"How does the carrier integration factory handle label generation errors?\"},
    {\"role\":\"assistant\",\"content\":\"It retries automatically.\"},
    {\"role\":\"user\",    \"content\":\"Got it, what about retry logic?\"},
    {\"role\":\"assistant\",\"content\":\"Exponential backoff is used.\"},
    {\"role\":\"user\",    \"content\":\"Can you explain how the carrier integration factory handles errors for label generation?\"}
  ]
}"
assert_contains "Gate 4: coach hint injected for high-overlap reask (3 user turns)" "keyword overlap"

# Unrelated messages: coach should not fire
send_request "{
  \"model\":\"claude-sonnet-4-6\",\"max_tokens\":10,
  \"messages\":[
    {\"role\":\"user\",    \"content\":\"How do I configure the Deutsche Post adapter?\"},
    {\"role\":\"assistant\",\"content\":\"Set the API key in your env file.\"},
    {\"role\":\"user\",    \"content\":\"What environment variables does it need?\"},
    {\"role\":\"assistant\",\"content\":\"DEUTSCHEPOST_API_KEY and DEUTSCHEPOST_SECRET.\"},
    {\"role\":\"user\",    \"content\":\"Where do I set those in staging?\"}
  ]
}"
assert_not_contains "Gate 4: coach silent for progressive follow-up messages" "[ctx coach"

# ============================================================
bold ""
bold "=== Gate 5: budget guard ==="
write_config "all" "false" "false"
restart_proxy

# 50M chars triggers the budget threshold (~25 USD estimate)
BUDGET_REQ_FILE="$CTX_TMP/budget_request.json"
python3 -c "
import json
big = 'x' * 50_000_000
payload = {
    'model': 'claude-sonnet-4-6',
    'max_tokens': 10,
    'messages': [{'role': 'user', 'content': big}]
}
with open('$BUDGET_REQ_FILE', 'w') as f:
    json.dump(payload, f)
"
send_request_file "$BUDGET_REQ_FILE"
assert_contains "Gate 5: budget warning injected for large request" "AskUserQuestion"

# Small request: guard must stay silent
send_request "{
  \"model\":\"claude-sonnet-4-6\",\"max_tokens\":10,
  \"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]
}"
assert_not_contains "Gate 5: no warning for small request" "AskUserQuestion"

# ============================================================
bold ""
bold "=== Results ==="
TOTAL=$(( PASS + FAIL ))
if [[ $FAIL -eq 0 ]]; then
  green "All $TOTAL gate smoke tests passed."
else
  red "$FAIL / $TOTAL tests FAILED."
  exit 1
fi
