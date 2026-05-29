#!/usr/bin/env python3
"""Humanize dashboard copy without altering whitespace structure."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "src" / "dashboard_static"


def humanize(text: str) -> str:
    # HTML placeholders
    text = text.replace(">—<", ">n/a<")
    text = text.replace(">—</", ">n/a</")
    text = text.replace("Event stream: —", "Event stream: none yet")

    # JS empty sentinels
    text = text.replace("'—'", "UI_EMPTY")
    text = text.replace('"—"', "UI_EMPTY")
    text = text.replace("'--'", "UI_EMPTY")
    text = text.replace('"--"', "UI_EMPTY")

    # Separators in UI copy
    text = text.replace(" &middot; ", ", ")
    text = text.replace("&middot; ", ", ")
    text = text.replace(" &middot;", ",")
    text = text.replace(" · ", ", ")
    text = text.replace(" ·", ",")

    # Arrows in prose
    text = text.replace(" &rarr; ", " to ")
    text = text.replace("&rarr;", "to")
    text = text.replace(" → ", ", then ")
    text = text.replace("→ ", "then ")
    text = text.replace(" →", " then")
    text = text.replace("Experiment →", "Open Experiment")
    text = text.replace("Trace →", "Open Trace")

    # Em / en dashes (comma keeps mid-sentence flow; avoid splitting into bad new sentences)
    text = re.sub(r" — ", ", ", text)
    text = re.sub(r"—", ", ", text)
    text = text.replace("–", " to ")

    # Stiff patterns
    text = text.replace("(auto — let ctx pick)", "(auto, let ctx pick)")
    text = text.replace("(auto, let ctx pick)", "(auto, let ctx pick)")

    return text


def main() -> None:
    changed = 0
    for path in list(ROOT.glob("**/*.html")) + list(ROOT.glob("**/*.js")):
        original = path.read_text(encoding="utf-8")
        updated = humanize(original)
        if updated != original:
            path.write_text(updated, encoding="utf-8")
            changed += 1
            print(path.relative_to(ROOT.parent.parent))
    print(f"updated {changed} files")


if __name__ == "__main__":
    main()
