#!/usr/bin/env bash
# Point the Homebrew tap at a published GitHub release.
#
#   scripts/release-brew.sh [version] [--push]
#
# Reads the version from Cargo.toml when not given. Downloads the two macOS tarballs from the
# GitHub release, computes their checksums, rewrites the tap formula, and commits. Without --push
# it stops after the commit so the diff can be read first.
#
# Env:
#   CTX_TAP   path to the homebrew-ctx checkout
#             (default: /opt/homebrew/Library/Taps/saurabh0392/homebrew-ctx)
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-}"
[[ "$VERSION" == "--push" ]] && VERSION=""
[[ -n "$VERSION" ]] || VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$REPO/Cargo.toml" | head -1)"
PUSH=false
for a in "$@"; do [[ "$a" == "--push" ]] && PUSH=true; done

TAP="${CTX_TAP:-/opt/homebrew/Library/Taps/saurabh0392/homebrew-ctx}"
FORMULA="$TAP/Formula/ctx.rb"
BASE="https://github.com/saurabh0392/ctx/releases/download/v${VERSION}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

[[ -f "$FORMULA" ]] || { echo "release-brew: no formula at $FORMULA (set CTX_TAP)"; exit 2; }

# Refuse to point the formula at a release that does not exist yet: a formula with a good-looking
# checksum and a 404 url fails at install time, on the user's machine, not here.
# Two plain variables, not an associative array: macOS ships bash 3.2, where `declare -A` is a
# syntax error, and this script is meant to run on the maintainer's Mac.
fetch_sha() {
  # Separate statements: bash 3.2 does not reliably see an earlier name assigned in the same
  # `local`, and `set -u` turns that into an unbound-variable abort.
  local arch="$1"
  local url="$BASE/ctx-${arch}.tar.gz"
  echo "release-brew: fetching ${arch}…" >&2
  curl -fsSL --retry 3 -o "$WORK/$arch.tar.gz" "$url" || {
    echo "release-brew: cannot download $url" >&2
    echo "  Is the v${VERSION} GitHub release published and are its assets uploaded?" >&2
    return 2
  }
  shasum -a 256 "$WORK/$arch.tar.gz" | awk '{print $1}'
}
SHA_ARM="$(fetch_sha aarch64-apple-darwin)" || exit 2
echo "release-brew:   arm   $SHA_ARM"
SHA_INTEL="$(fetch_sha x86_64-apple-darwin)" || exit 2
echo "release-brew:   intel $SHA_INTEL"

python3 - "$FORMULA" "$VERSION" "$SHA_ARM" "$SHA_INTEL" <<'PY'
import re, sys
path, version, arm_sha, intel_sha = sys.argv[1:5]
s = open(path).read()
s = re.sub(r'^(\s*version\s+")[^"]+(")', rf'\g<1>{version}\g<2>', s, count=1, flags=re.M)
s = re.sub(r'/download/v[^/]+/', f'/download/v{version}/', s)

# Rewrite each sha256 inside the block that owns it, so the arm checksum can never land under
# on_intel. Anchor on the url line above it rather than on ordering.
def swap(block, sha):
    global s
    pat = re.compile(rf'(on_{block} do.*?sha256 ")[0-9a-f]{{64}}(")', re.S)
    s, n = pat.subn(rf'\g<1>{sha}\g<2>', s, count=1)
    assert n == 1, f'no sha256 found in on_{block} block'

swap('arm', arm_sha)
swap('intel', intel_sha)
open(path, 'w').write(s)
print(f'release-brew: formula set to {version}')
PY

git -C "$TAP" --no-pager diff -- Formula/ctx.rb
git -C "$TAP" add Formula/ctx.rb
git -C "$TAP" commit -q -m "feat: ctx ${VERSION}"
echo "release-brew: committed in $TAP"

if $PUSH; then
  git -C "$TAP" push
  echo "release-brew: pushed. Users get it with: brew update && brew upgrade ctx"
else
  echo "release-brew: not pushed. Review the diff above, then: git -C $TAP push"
fi
