#!/usr/bin/env bash
# Manage the beta invite roster (an SSM SecureString, one token per line "token = label").
#
#   ./scripts/dist-token.sh list
#   ./scripts/dist-token.sh add "alice@example.com"     # mints a random token, prints it once
#   ./scripts/dist-token.sh revoke <token-or-participant-id>
#
# Give the printed token to the user: they run
#   curl -fsSL <endpoint>/install.sh | CTX_TOKEN=<token> sh
set -euo pipefail

PARAM="${SSM_TOKENS_PARAM:-/ctx/dist/alpha-tokens}"
cmd="${1:-list}"

read_param() { aws ssm get-parameter --name "$PARAM" --with-decryption --query 'Parameter.Value' --output text 2>/dev/null || printf ''; }
write_param() { aws ssm put-parameter --name "$PARAM" --type SecureString --overwrite --value "$1" >/dev/null; }
participant_id() {
  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s' "$1" | sha256sum | cut -c1-16
  else
    printf '%s' "$1" | shasum -a 256 | cut -c1-16
  fi
}

case "$cmd" in
  list)
    while IFS= read -r line; do
      [[ -z "$line" || "$line" == \#* ]] && continue
      raw_token="${line%%=*}"; token="${raw_token//[[:space:]]/}"
      label="${line#*=}"; label="${label# }"
      printf '%s  %s\n' "$(participant_id "$token")" "$label"
    done < <(read_param) ;;
  add)
    label="${2:?usage: dist-token.sh add <label>}"
    token="ctx_$(head -c 24 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 32)"
    cur="$(read_param)"
    new="$(printf '%s\n%s = %s\n' "$cur" "$token" "$label" | sed '/^$/d')"
    write_param "$new"
    printf 'token for %s (give this to them, it is shown once):\n  %s\nparticipant id:\n  %s\n' "$label" "$token" "$(participant_id "$token")" ;;
  revoke)
    target="${2:?usage: dist-token.sh revoke <token-or-participant-id>}"
    new=""; removed=0
    while IFS= read -r line; do
      [[ -z "$line" ]] && continue
      [[ "$line" == \#* ]] && { printf -v new '%s%s\n' "$new" "$line"; continue; }
      raw_token="${line%%=*}"; token="${raw_token//[[:space:]]/}"
      if [[ "$target" == "$token" || "$target" == "$(participant_id "$token")" ]]; then removed=1; continue; fi
      printf -v new '%s%s\n' "$new" "$line"
    done < <(read_param)
    [[ "$removed" == 1 ]] || { printf 'no matching participant.\n' >&2; exit 1; }
    write_param "${new%$'\n'}"; printf 'revoked %s; invite and capabilities are now invalid.\n' "$target" ;;
  *)
    echo "usage: dist-token.sh {list|add <label>|revoke <token-or-participant-id>}" >&2; exit 1 ;;
esac
