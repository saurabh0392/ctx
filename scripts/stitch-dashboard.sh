#!/usr/bin/env bash
# Concatenate dashboard_static fragments into src/dashboard.html (stdout).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STATIC="$ROOT/src/dashboard_static"
MANIFEST="$STATIC/MANIFEST"

if [[ ! -f "$MANIFEST" ]]; then
  echo "stitch-dashboard: missing $MANIFEST (run scripts/split-dashboard.py first)" >&2
  exit 1
fi

emit_file() {
  cat "$STATIC/$1"
}

in_style=0
in_script=0

while IFS= read -r line || [[ -n "$line" ]]; do
  [[ -z "$line" || "$line" =~ ^# ]] && continue

  case "$line" in
    styles:*)
      rel="${line#styles:}"
      in_style=1
      emit_file "$rel"
      ;;
    script:*)
      rel="${line#script:}"
      if [[ $in_script -eq 0 ]]; then
        printf '<script>\n'
        in_script=1
      fi
      emit_file "$rel"
      ;;
    include:*)
      rel="${line#include:}"
      emit_file "$rel"
      ;;
    *)
      if [[ $in_script -eq 1 ]]; then
        printf '</script>\n'
        in_script=0
      fi
      in_style=0
      emit_file "$line"
      ;;
  esac
done < "$MANIFEST"

if [[ $in_script -eq 1 ]]; then
  printf '</script>\n'
fi
