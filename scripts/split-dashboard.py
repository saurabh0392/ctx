#!/usr/bin/env python3
"""One-time / repeatable split of src/dashboard.html into dashboard_static fragments."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src" / "dashboard.html"
STATIC = ROOT / "src" / "dashboard_static"


def lines(path: Path) -> list[str]:
    return path.read_text(encoding="utf-8").splitlines(keepends=True)


def slice_lines(all_lines: list[str], start: int, end: int) -> str:
    """1-based inclusive start/end."""
    return "".join(all_lines[start - 1 : end])


def write(rel: str, content: str) -> None:
    p = STATIC / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")


def main() -> None:
    L = lines(SRC)

    # CSS in original order: 9-502, onboarding 503-504, 505-636
    write("styles/dashboard_part1.css", slice_lines(L, 9, 502))
    write("styles/onboarding.css", slice_lines(L, 503, 504))
    write("styles/dashboard_part2.css", slice_lines(L, 505, 636))

    write("fragments/shell_head.html", slice_lines(L, 1, 8))
    write("fragments/shell_body.html", slice_lines(L, 637, 711))

    # Savings tab: before onboarding wrap, wrap open, fragment, wrap close, rest
    write("tabs/savings_head.html", slice_lines(L, 712, 721))
    write(
        "tabs/savings_onboarding_wrap_open.html",
        slice_lines(L, 722, 722),
    )
    write("fragments/onboarding.fragment.html", slice_lines(L, 723, 783))
    write("tabs/savings_tail.html", slice_lines(L, 784, 849))

    write("tabs/promptstats.html", slice_lines(L, 850, 951))
    write("tabs/profiles.html", slice_lines(L, 952, 962))
    write("tabs/trace.html", slice_lines(L, 963, 976))
    write("tabs/pipeline.html", slice_lines(L, 977, 1008))
    write("tabs/experiment.html", slice_lines(L, 1009, 1042))
    write("tabs/settings.html", slice_lines(L, 1043, 1136))
    write("fragments/modals.html", slice_lines(L, 1137, 1166))

    # JS (inside <script>, lines 1168-3209; trailing blank lines preserved at section ends)
    write("js/core.js", slice_lines(L, 1168, 1284))
    write("js/onboarding.js", slice_lines(L, 1285, 1357))
    write("js/settings.js", slice_lines(L, 1358, 1694))
    write("js/navigation.js", slice_lines(L, 1695, 1750))
    write("js/savings.js", slice_lines(L, 1751, 2227))
    write("js/promptstats.js", slice_lines(L, 2228, 2568))
    write("js/profiles.js", slice_lines(L, 2569, 2628))
    write("js/trace.js", slice_lines(L, 2629, 2984))
    write("js/pipeline.js", slice_lines(L, 2985, 3127))
    write("js/profile_analytics.js", slice_lines(L, 3128, 3185))
    write("js/theme.js", slice_lines(L, 3186, 3203))
    write("js/boot.js", slice_lines(L, 3204, 3209))

    write("fragments/tail.html", slice_lines(L, 3211, 3212))

    manifest = """# Dashboard stitch manifest (order matters)
fragments/shell_head.html
styles:styles/dashboard_part1.css
styles:styles/onboarding.css
styles:styles/dashboard_part2.css
fragments/shell_body.html
tabs/savings_head.html
tabs/savings_onboarding_wrap_open.html
include:fragments/onboarding.fragment.html
tabs/savings_tail.html
tabs/promptstats.html
tabs/profiles.html
tabs/trace.html
tabs/pipeline.html
tabs/experiment.html
tabs/settings.html
fragments/modals.html
script:js/core.js
script:js/onboarding.js
script:js/settings.js
script:js/navigation.js
script:js/savings.js
script:js/promptstats.js
script:js/profiles.js
script:js/trace.js
script:js/pipeline.js
script:js/profile_analytics.js
script:js/theme.js
script:js/boot.js
fragments/tail.html
"""
    write("MANIFEST", manifest)
    print(f"Wrote fragments under {STATIC}")


if __name__ == "__main__":
    main()
