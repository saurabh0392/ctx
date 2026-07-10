#!/bin/sh
# ctx installer. No repo access, no gh, no Rust.
#
#   curl -fsSL <endpoint>/install.sh | CTX_TOKEN=<your-alpha-token> sh
#
# It asks the ctx distribution endpoint for a short-lived download link (gated by your token),
# verifies the checksum, installs the binary to ~/.local/bin, and runs `ctx setup`.
set -eu

# The endpoint is templated in when this script is served. CTX_ENDPOINT can override it for testing.
CTX_ENDPOINT="${CTX_ENDPOINT:-__CTX_ENDPOINT__}"
INSTALL_DIR="${CTX_INSTALL_DIR:-$HOME/.local/bin}"

die() { printf 'error: %s\n' "$1" >&2; exit 1; }

case "${CTX_ENDPOINT}" in
  __CTX_ENDPOINT__*) die "no endpoint configured. Fetch this script from the ctx distribution URL." ;;
esac
[ -n "${CTX_TOKEN:-}" ] || die "set CTX_TOKEN to your alpha token, e.g. CTX_TOKEN=xxxx sh"

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
resp="$(curl -fsSL -X POST "$CTX_ENDPOINT" \
  -H 'content-type: application/json' \
  -d "{\"token\":\"${CTX_TOKEN}\",\"target\":\"${target}\"}")" \
  || die "the endpoint rejected the request (token invalid, revoked, or no build for $target)"

field() { printf '%s' "$resp" | sed -n "s/.*\"$1\":\"\\([^\"]*\\)\".*/\\1/p"; }
url="$(field url)"; sha="$(field sha256)"; version="$(field version)"
[ -n "$url" ] || die "no download url returned: $resp"

# --- download + verify -----------------------------------------------------
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
printf 'Downloading ctx %s...\n' "${version:-latest}"
curl -fsSL "$url" -o "$tmp/ctx.tar.gz" || die "download failed"

if [ -n "$sha" ]; then
  if command -v sha256sum >/dev/null 2>&1; then got="$(sha256sum "$tmp/ctx.tar.gz" | cut -d' ' -f1)";
  else got="$(shasum -a 256 "$tmp/ctx.tar.gz" | cut -d' ' -f1)"; fi
  [ "$got" = "$sha" ] || die "checksum mismatch (expected $sha, got $got). Aborting."
  printf 'Checksum verified.\n'
fi

tar -xzf "$tmp/ctx.tar.gz" -C "$tmp" || die "extract failed"
[ -f "$tmp/ctx" ] || die "archive did not contain a ctx binary"

# --- install ---------------------------------------------------------------
mkdir -p "$INSTALL_DIR"
mv "$tmp/ctx" "$INSTALL_DIR/ctx"
chmod +x "$INSTALL_DIR/ctx"
bin="$INSTALL_DIR/ctx"

if [ "$os" = "Darwin" ]; then
  # Trust bridge until the binary is Developer ID signed and notarized: clear the download quarantine
  # so Gatekeeper does not block it, and ad-hoc sign so launchd does not SIGKILL the dashboard service.
  xattr -dr com.apple.quarantine "$bin" 2>/dev/null || true
  codesign --force --sign - "$bin" 2>/dev/null || true
fi

# --- wire into your agent --------------------------------------------------
printf 'Setting up ctx...\n'
"$bin" setup || die "ctx setup failed"

# --- PATH hint -------------------------------------------------------------
case ":$PATH:" in
  *":$INSTALL_DIR:"*) : ;;
  *) printf '\nAdd ctx to your PATH:\n  export PATH="%s:$PATH"\n' "$INSTALL_DIR" ;;
esac
printf '\nctx is installed. Dashboard: http://127.0.0.1:8789\n'
