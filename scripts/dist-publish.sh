#!/usr/bin/env bash
# Publish one ctx build to the distribution bucket: tar, checksum, upload, and merge the manifest.
# CI (release.yml) runs this per target; locally you can publish the host build to smoke test.
#
#   BUCKET=<artifacts-bucket> ./scripts/dist-publish.sh <target> <path-to-ctx-binary> <version>
#
# Also uploads scripts/install.sh so the endpoint serves the current installer.
set -euo pipefail

BUCKET="${BUCKET:?set BUCKET to the ctx-dist artifacts bucket}"
TARGET="${1:?usage: dist-publish.sh <target> <ctx-binary> <version>}"
BIN="${2:?path to the built ctx binary}"
VERSION="${3:?version string, e.g. 0.4.0}"
HERE="$(cd "$(dirname "$0")" && pwd)"

work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
cp "$BIN" "$work/ctx"; chmod +x "$work/ctx"
archive="ctx-${VERSION}-${TARGET}.tar.gz"
tar -C "$work" -czf "$work/$archive" ctx
sha="$(shasum -a 256 "$work/$archive" | cut -d' ' -f1)"

echo "uploading bin/$archive ($sha)"
aws s3 cp "$work/$archive" "s3://$BUCKET/bin/$archive" --only-show-errors

# Merge this target into manifest/latest.json (create if missing).
aws s3 cp "s3://$BUCKET/manifest/latest.json" "$work/latest.json" --only-show-errors 2>/dev/null || echo '{}' > "$work/latest.json"
python3 - "$work/latest.json" "$VERSION" "$TARGET" "bin/$archive" "$sha" <<'PY'
import json, sys
path, version, target, file, sha = sys.argv[1:6]
try: m = json.load(open(path))
except Exception: m = {}
previous_version = m.get("version")
m.setdefault("targets", {})
if previous_version:
    for existing in m["targets"].values():
        existing.setdefault("version", previous_version)
m["version"] = version
m["targets"][target] = {"version": version, "file": file, "sha256": sha}
json.dump(m, open(path, "w"), indent=2)
print("manifest:", json.dumps(m))
PY
aws s3 cp "$work/latest.json" "s3://$BUCKET/manifest/latest.json" --content-type application/json --only-show-errors

# Keep the served installer current.
aws s3 cp "$HERE/install.sh" "s3://$BUCKET/install.sh" --content-type text/x-shellscript --only-show-errors
[ -f "$HERE/install.ps1" ] && aws s3 cp "$HERE/install.ps1" "s3://$BUCKET/install.ps1" --content-type text/plain --only-show-errors || true

echo "published ctx $VERSION for $TARGET"
