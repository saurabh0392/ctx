#!/usr/bin/env bash
# Manage the alpha token allowlist (an SSM SecureString, one token per line "token = label").
#
#   ./scripts/dist-token.sh list
#   ./scripts/dist-token.sh add "alice@example.com"     # mints a random token, prints it once
#   ./scripts/dist-token.sh revoke <token>
#
# Give the printed token to the user: they run
#   curl -fsSL <endpoint>/install.sh | CTX_TOKEN=<token> sh
set -euo pipefail

PARAM="${SSM_TOKENS_PARAM:-/ctx/dist/alpha-tokens}"
cmd="${1:-list}"

read_param() { aws ssm get-parameter --name "$PARAM" --with-decryption --query 'Parameter.Value' --output text 2>/dev/null || printf ''; }
write_param() { aws ssm put-parameter --name "$PARAM" --type SecureString --overwrite --value "$1" >/dev/null; }

case "$cmd" in
  list)
    read_param | sed '/^$/d' ;;
  add)
    label="${2:?usage: dist-token.sh add <label>}"
    token="ctx_$(head -c 24 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 32)"
    cur="$(read_param)"
    new="$(printf '%s\n%s = %s\n' "$cur" "$token" "$label" | sed '/^$/d')"
    write_param "$new"
    printf 'token for %s (give this to them, it is shown once):\n  %s\n' "$label" "$token" ;;
  revoke)
    tok="${2:?usage: dist-token.sh revoke <token>}"
    new="$(read_param | grep -v -F "$tok" | sed '/^$/d')"
    write_param "$new"; printf 'revoked.\n' ;;
  *)
    echo "usage: dist-token.sh {list|add <label>|revoke <token>}" >&2; exit 1 ;;
esac
