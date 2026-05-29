#!/usr/bin/env bash
# ctx-managed Claude Code statusLine — display allowance + record snapshots for dashboard.
payload=$(cat)
PORT="__DASHBOARD_PORT__"

# Fire-and-forget snapshot (must not block status line rendering).
(
  curl -sf --max-time 0.4 -X POST "http://127.0.0.1:${PORT}/api/allowance/snapshot" \
    -H 'Content-Type: application/json' \
    -d "$payload" >/dev/null 2>&1 || true
) &

model=$(echo "$payload" | jq -r '.model.display_name // .model.id // "Claude"' 2>/dev/null || echo "Claude")
ctx_pct=$(echo "$payload" | jq -r '.context_window.used_percentage // empty' 2>/dev/null || true)
five=$(echo "$payload" | jq -r '.rate_limits.five_hour.used_percentage // empty' 2>/dev/null || true)
seven=$(echo "$payload" | jq -r '.rate_limits.seven_day.used_percentage // empty' 2>/dev/null || true)

parts=()
if [ -n "$ctx_pct" ]; then
  pct_int=${ctx_pct%%.*}
  parts+=("ctx ${pct_int}%")
fi
if [ -n "$five" ]; then
  pct_int=${five%%.*}
  parts+=("5h ${pct_int}%")
fi
if [ -n "$seven" ]; then
  pct_int=${seven%%.*}
  parts+=("7d ${pct_int}%")
fi

if [ ${#parts[@]} -gt 0 ]; then
  joined=$(IFS=' · '; echo "${parts[*]}")
  printf '\033[90m%s\033[0m  %s' "$model" "$joined"
else
  printf '\033[90m%s\033[0m' "$model"
fi
