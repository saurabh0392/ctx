#!/bin/sh
# Codex plugin dispatcher. Hook processes must fail open: a missing CTX binary never blocks work.
if [ -n "${CTX_BIN:-}" ] && [ -x "${CTX_BIN}" ]; then
  exec "${CTX_BIN}" "$@"
fi

if command -v ctx >/dev/null 2>&1; then
  exec ctx "$@"
fi

if [ -x "${HOME}/.local/bin/ctx" ]; then
  exec "${HOME}/.local/bin/ctx" "$@"
fi

case " $* " in
  *" codex-session-start "*)
    printf '%s' '{"systemMessage":"CTX plugin is installed, but the ctx binary is unavailable. Run the CTX installer, then restart Codex."}'
    ;;
  *) printf '%s' '{}' ;;
esac
exit 0
