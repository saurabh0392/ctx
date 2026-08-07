#!/bin/sh
# ctx installer. No gh, no Rust.
#
#   gh repo clone saurabh0392/ctx && bash ctx/scripts/install.sh
#   (or: curl -fsSL https://raw.githubusercontent.com/saurabh0392/ctx/main/scripts/install.sh | sh)
#
# Downloads the latest release binary from GitHub, verifies its checksum, installs it to
# ~/.local/bin, and runs `ctx setup`. Prefer `brew install saurabh0392/ctx/ctx` or
# `cargo install ctx-agent` when you have brew or cargo.
set -eu

REPO="saurabh0392/ctx"
INSTALL_DIR="${CTX_INSTALL_DIR:-$HOME/.local/bin}"

die() { printf 'error: %s\n' "$1" >&2; exit 1; }

# --- target detection ------------------------------------------------------
os="$(uname -s)"; arch="$(uname -m)"
case "$os" in
  Darwin) os_tag="apple-darwin" ;;
  Linux)  os_tag="unknown-linux-gnu" ;;
  *) die "unsupported OS: $os (Windows installer is install.ps1)" ;;
esac
case "$arch" in
  arm64|aarch64) arch_tag="aarch64" ;;
  x86_64|amd64)  arch_tag="x86_64" ;;
  *) die "unsupported architecture: $arch" ;;
esac
target="${arch_tag}-${os_tag}"
asset="ctx-${target}.tar.gz"

# --- download --------------------------------------------------------------
base="https://github.com/${REPO}/releases/latest/download"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "downloading ${asset} from ${REPO} latest release…"
curl -fsSL -o "${tmp}/${asset}" "${base}/${asset}" \
  || die "no build for ${target} in the latest release. Try 'cargo install ctx-agent' instead."
curl -fsSL -o "${tmp}/checksums.txt" "${base}/checksums.txt" \
  || die "could not fetch checksums.txt from the latest release"

# --- verify ----------------------------------------------------------------
expected="$(grep " ${asset}\$" "${tmp}/checksums.txt" | awk '{print $1}')"
[ -n "$expected" ] || die "checksums.txt has no entry for ${asset}"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${tmp}/${asset}" | awk '{print $1}')"
else
  actual="$(shasum -a 256 "${tmp}/${asset}" | awk '{print $1}')"
fi
[ "$expected" = "$actual" ] || die "checksum mismatch for ${asset}"

# --- install ---------------------------------------------------------------
mkdir -p "$INSTALL_DIR"
tar -xzf "${tmp}/${asset}" -C "$tmp"
[ -f "${tmp}/ctx" ] || die "archive did not contain a ctx binary"
install -m 0755 "${tmp}/ctx" "${INSTALL_DIR}/ctx"
echo "installed ctx to ${INSTALL_DIR}/ctx"

case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *) echo "note: add ${INSTALL_DIR} to your PATH" ;;
esac

# --- setup -----------------------------------------------------------------
"${INSTALL_DIR}/ctx" setup
