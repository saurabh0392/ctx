#!/usr/bin/env bash
# ctx installer — downloads the latest pre-built binary for your platform.
#
# Primary path (internal/private repo): requires the GitHub CLI (`gh`) authenticated
# to the goshippo org.
#
# Usage:
#   gh repo clone goshippo/ctx /tmp/ctx-src && bash /tmp/ctx-src/scripts/install.sh
#
# Or with a token (CI / machines without gh):
#   GITHUB_TOKEN=<pat> bash scripts/install.sh
#
# Or pipe directly if you have gh:
#   gh api repos/goshippo/ctx/contents/scripts/install.sh --jq '.content' \
#     | base64 -d | sh
set -euo pipefail

REPO="goshippo/ctx"
INSTALL_DIR="${CTX_INSTALL_DIR:-/usr/local/bin}"

# --------------------------------------------------------------------------
# Platform detection
# --------------------------------------------------------------------------
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin) OS_TAG="apple-darwin" ;;
  Linux)  OS_TAG="unknown-linux-gnu" ;;
  *)
    echo "error: unsupported OS: $OS"
    echo "       Build from source:  cargo install --git https://github.com/${REPO}"
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64|amd64)    ARCH_TAG="x86_64" ;;
  arm64|aarch64)   ARCH_TAG="aarch64" ;;
  *)
    echo "error: unsupported architecture: $ARCH"
    echo "       Build from source:  cargo install --git https://github.com/${REPO}"
    exit 1
    ;;
esac

TARGET="${ARCH_TAG}-${OS_TAG}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# --------------------------------------------------------------------------
# Download — gh CLI first (works for internal repos), curl fallback
# --------------------------------------------------------------------------
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  echo "Fetching latest ctx release via gh..."
  LATEST_TAG="$(gh release list --repo "$REPO" --limit 1 --json tagName --jq '.[0].tagName')"
  if [ -z "$LATEST_TAG" ]; then
    echo "error: no releases found at ${REPO}"
    exit 1
  fi
  echo "Latest: ${LATEST_TAG}"
  echo "Downloading ctx-${TARGET}.tar.gz..."
  gh release download "$LATEST_TAG" \
    --repo "$REPO" \
    --pattern "ctx-${TARGET}.tar.gz" \
    --dir "$TMP"
else
  # Curl path — works when GITHUB_TOKEN is set or if the repo ever becomes public.
  if [ -z "${GITHUB_TOKEN:-}" ]; then
    echo "error: 'gh' CLI not found or not authenticated, and GITHUB_TOKEN is not set."
    echo ""
    echo "Install options:"
    echo "  1. Authenticate gh:  gh auth login   then re-run this script"
    echo "  2. Set a token:      GITHUB_TOKEN=<pat> bash scripts/install.sh"
    echo "  3. Build from source (requires Rust):"
    echo "     gh repo clone ${REPO} ~/Documents/ctx"
    echo "     source \"\$HOME/.cargo/env\" && cargo install --locked --path ~/Documents/ctx"
    exit 1
  fi
  echo "Fetching latest ctx release via curl (GITHUB_TOKEN)..."
  LATEST_TAG="$(
    curl -fsSL -H "Authorization: Bearer ${GITHUB_TOKEN}" \
      "https://api.github.com/repos/${REPO}/releases/latest" \
      | grep '"tag_name"' \
      | head -1 \
      | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/'
  )"
  if [ -z "$LATEST_TAG" ]; then
    echo "error: could not determine latest release"
    exit 1
  fi
  echo "Latest: ${LATEST_TAG}"
  URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/ctx-${TARGET}.tar.gz"
  echo "Downloading ctx-${TARGET}.tar.gz..."
  curl -fsSL -H "Authorization: Bearer ${GITHUB_TOKEN}" \
    --progress-bar "$URL" -o "$TMP/ctx.tar.gz"
  tar -xzf "$TMP/ctx.tar.gz" -C "$TMP"
fi

# --------------------------------------------------------------------------
# Install — prefer ~/.local/bin over sudo when the target dir isn't writable
# --------------------------------------------------------------------------
if [ -w "$INSTALL_DIR" ]; then
  install -m 755 "$TMP/ctx" "$INSTALL_DIR/ctx"
elif [ "${CTX_INSTALL_DIR:-}" = "" ]; then
  # Default dir not writable and no override set: fall back to ~/.local/bin (no sudo)
  INSTALL_DIR="$HOME/.local/bin"
  mkdir -p "$INSTALL_DIR"
  install -m 755 "$TMP/ctx" "$INSTALL_DIR/ctx"
  echo "  (installed to $INSTALL_DIR — add it to PATH if not already there)"
  # Emit a PATH hint when ~/.local/bin isn't on PATH yet
  case ":${PATH}:" in
    *":$INSTALL_DIR:"*) ;;
    *) echo "  Add to your shell profile:  export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
  esac
else
  # Explicit CTX_INSTALL_DIR set but not writable — sudo is intentional
  echo "Installing to ${INSTALL_DIR} (requires sudo)..."
  sudo install -m 755 "$TMP/ctx" "$INSTALL_DIR/ctx"
fi

# --------------------------------------------------------------------------
# Verify + next steps
# --------------------------------------------------------------------------
if ! command -v ctx >/dev/null 2>&1; then
  echo ""
  echo "ctx installed to ${INSTALL_DIR}/ctx"
  echo "Make sure ${INSTALL_DIR} is on your PATH, then run:  ctx setup"
else
  CTX_VER="$(ctx --version 2>/dev/null || echo '?')"
  echo ""
  echo "✓ ${CTX_VER} installed to ${INSTALL_DIR}/ctx"
  echo ""
  echo "Next steps:"
  echo "  ctx setup"
  echo "  ctx profile generate   # build profiles from your MCP stack"
  echo "  ctx use <profile>"
fi
