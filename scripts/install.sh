#!/bin/sh
# ctx installer. No repo access, no gh, no Rust.
#
#   curl -fsSL <endpoint>/install.sh | CTX_TOKEN=<your-beta-token> sh
#
# It asks the ctx distribution endpoint for a short-lived download link (gated by your token),
# verifies the checksum, installs the binary to ~/.local/bin, and runs `ctx setup`.
set -eu

# The endpoint is templated in when this script is served. CTX_ENDPOINT can override it for testing.
CTX_ENDPOINT="${CTX_ENDPOINT:-__CTX_ENDPOINT__}"
INSTALL_DIR="${CTX_INSTALL_DIR:-$HOME/.local/bin}"

die() { printf 'error: %s\n' "$1" >&2; exit 1; }

# Sentinel assembled from two pieces so the serve-time templating (which replaces the contiguous
# placeholder) cannot rewrite this guard. If the placeholder in the assignment above was not replaced,
# CTX_ENDPOINT still equals it and we refuse to run.
_ph="__CTX_""ENDPOINT__"
case "${CTX_ENDPOINT}" in
  "$_ph"*) die "no endpoint configured. Fetch this script from the ctx distribution URL." ;;
esac
[ -n "${CTX_TOKEN:-}" ] || die "set CTX_TOKEN to your beta invite, e.g. CTX_TOKEN=xxxx sh"

# --- target detection ------------------------------------------------------
os="$(uname -s)"; arch="$(uname -m)"
case "$os" in
  Darwin) os_tag="apple-darwin" ;;
  Linux)  os_tag="unknown-linux-gnu" ;;
  *) die "unsupported OS: $os (Windows installer is install.ps1)" ;;
esac
case "$arch" in
  x86_64|amd64)  arch_tag="x86_64" ;;
  arm64|aarch64) arch_tag="aarch64" ;;
  *) die "unsupported architecture: $arch" ;;
esac
target="${arch_tag}-${os_tag}"

# --- ask the endpoint for a signed download link ---------------------------
printf 'Requesting ctx for %s...\n' "$target"
resp="$(printf '{"token":"%s","target":"%s"}' "$CTX_TOKEN" "$target" | \
  curl -fsSL -X POST "$CTX_ENDPOINT" \
  -H 'content-type: application/json' \
  --data-binary @-)" \
  || die "the endpoint rejected the request (token invalid, revoked, or no build for $target)"

field() { printf '%s' "$resp" | sed -n "s/.*\"$1\":\"\\([^\"]*\\)\".*/\\1/p"; }
url="$(field url)"; sha="$(field sha256)"; version="$(field version)"
credential="$(field credential)"; participant="$(field participantId)"; feedback="$(field feedbackEndpoint)"
[ -n "$url" ] || die "no download url returned: $resp"
[ -n "$sha" ] || die "no checksum returned; refusing an unverifiable binary"
[ -n "$version" ] || die "no release version returned"
[ -n "$credential" ] || die "no scoped beta capability returned; the distribution service needs upgrading"

# --- download + verify -----------------------------------------------------
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
printf 'Downloading ctx %s...\n' "${version:-latest}"
curl -fsSL "$url" -o "$tmp/ctx.tar.gz" || die "download failed"

if command -v sha256sum >/dev/null 2>&1; then got="$(sha256sum "$tmp/ctx.tar.gz" | cut -d' ' -f1)";
else got="$(shasum -a 256 "$tmp/ctx.tar.gz" | cut -d' ' -f1)"; fi
[ "$got" = "$sha" ] || die "checksum mismatch (expected $sha, got $got). Aborting."
printf 'Checksum verified.\n'

raw_entries="$(tar -tzf "$tmp/ctx.tar.gz")" || die "could not inspect archive"
entries="$(printf '%s' "$raw_entries" | sed 's#^\./##')"
[ "$entries" = "ctx" ] || die "archive must contain exactly one ctx binary"
tar -xzf "$tmp/ctx.tar.gz" -C "$tmp" || die "extract failed"
[ -f "$tmp/ctx" ] && [ ! -L "$tmp/ctx" ] || die "archive did not contain a regular ctx binary"

# --- install ---------------------------------------------------------------
mkdir -p "$INSTALL_DIR"
bin="$INSTALL_DIR/ctx"
staged="$INSTALL_DIR/.ctx-install-new"
backup="$INSTALL_DIR/.ctx-install-previous"
mv "$tmp/ctx" "$staged"
chmod +x "$staged"

if [ "$os" = "Darwin" ]; then
  # Trust bridge until the binary is Developer ID signed and notarized: clear the download quarantine
  # so Gatekeeper does not block it, and ad-hoc sign so launchd does not SIGKILL the dashboard service.
  xattr -dr com.apple.quarantine "$staged" 2>/dev/null || true
  codesign --force --sign - "$staged" 2>/dev/null || true
fi

reported="$("$staged" --version 2>/dev/null)" || die "downloaded ctx binary could not start"
case "$reported" in
  *"${version#v}"*) : ;;
  *) die "downloaded binary version did not match $version" ;;
esac

rm -f "$backup"
[ ! -e "$bin" ] || mv "$bin" "$backup"
if ! mv "$staged" "$bin"; then
  [ ! -e "$backup" ] || mv "$backup" "$bin"
  die "could not activate ctx; the previous binary was restored"
fi

# --- wire into your agent --------------------------------------------------
# --yes: the installer is piped through sh with no TTY, so setup must not prompt.
printf 'Setting up ctx...\n'
if ! CTX_BETA_CREDENTIAL="$credential" \
CTX_PARTICIPANT_ID="$participant" \
CTX_DIST_ENDPOINT="$CTX_ENDPOINT" \
CTX_FEEDBACK_ENDPOINT="$feedback" \
"$bin" setup --beta --yes; then
  rm -f "$bin"
  [ ! -e "$backup" ] || mv "$backup" "$bin"
  die "ctx beta setup failed; the previous binary was restored"
fi
rm -f "$backup"

# --- PATH ------------------------------------------------------------------
# The dashboard and hooks call ctx by full path, so they work regardless. This only lets the user type
# `ctx` directly. Append to the right shell profile, idempotently and with a marker they can remove.
case ":$PATH:" in
  *":$INSTALL_DIR:"*) : ;;
  *)
    case "${SHELL:-}" in
      *zsh)  profile="$HOME/.zshrc" ;;
      *bash) [ "$os" = "Darwin" ] && profile="$HOME/.bash_profile" || profile="$HOME/.bashrc" ;;
      *)     profile="$HOME/.profile" ;;
    esac
    if [ -f "$profile" ] && grep -q 'added by ctx installer' "$profile" 2>/dev/null; then
      : # already added
    else
      printf '\n# added by ctx installer\nexport PATH="%s:$PATH"\n' "$INSTALL_DIR" >> "$profile"
      printf 'Added %s to PATH in %s (open a new shell, or run: export PATH="%s:$PATH")\n' "$INSTALL_DIR" "$profile" "$INSTALL_DIR"
    fi
    ;;
esac
printf '\nctx is installed. Dashboard: http://127.0.0.1:8789\n'
