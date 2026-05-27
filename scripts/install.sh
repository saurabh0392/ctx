#!/usr/bin/env bash
# ctx installer — downloads the latest pre-built binary for your platform.
# Usage:  curl -fsSL https://raw.githubusercontent.com/goshippo/ctx/main/scripts/install.sh | sh
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

# --------------------------------------------------------------------------
# Resolve latest release tag
# --------------------------------------------------------------------------
echo "Fetching latest ctx release..."
LATEST_TAG="$(
  curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' \
    | head -1 \
    | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/'
)"

if [ -z "$LATEST_TAG" ]; then
  echo "error: could not determine latest release (API rate limit or no releases yet)"
  echo "       Check: https://github.com/${REPO}/releases"
  exit 1
fi

echo "Latest: ${LATEST_TAG}"

# --------------------------------------------------------------------------
# Download and install
# --------------------------------------------------------------------------
URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/ctx-${TARGET}.tar.gz"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Downloading ctx-${TARGET}.tar.gz..."
curl -fsSL --progress-bar "$URL" -o "$TMP/ctx.tar.gz"

tar -xzf "$TMP/ctx.tar.gz" -C "$TMP"

# Write to install dir, prompting for sudo if needed
if [ -w "$INSTALL_DIR" ]; then
  install -m 755 "$TMP/ctx" "$INSTALL_DIR/ctx"
else
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
  echo "Next step:"
  echo "  ctx setup"
fi
