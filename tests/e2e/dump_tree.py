#!/usr/bin/env python3
"""Print the AT-SPI accessibility tree of the running wren process.

Run this after starting wren under Xvfb to inspect what's exposed:

    ./tests/e2e/run.sh -k __nonexistent__   # boots Xvfb, exits empty
    # in another terminal, with DISPLAY=:99 and PYTHONPATH set up:
    DISPLAY=:99 python3 tests/e2e/dump_tree.py

Useful when wren's UI changes and the tests need their selectors updated.
"""

from __future__ import annotations

import sys
import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def dump(acc, depth: int = 0, max_depth: int = 25) -> None:
    if depth > max_depth:
        return
    try:
        name = acc.get_name() or ""
        role = acc.get_role_name() or ""
        n = acc.get_child_count()
    except Exception as e:
        print("  " * depth + f"<error: {e}>")
        return
    ext = acc.get_extents(Atspi.CoordType.WINDOW)
    print(
        "  " * depth
        + f"{role} name={name!r} ext={ext.x},{ext.y} {ext.width}x{ext.height} children={n}"
    )
    for i in range(n):
        try:
            c = acc.get_child_at_index(i)
        except Exception:
            continue
        if c is not None:
            dump(c, depth + 1, max_depth)


def main() -> int:
    desktop = Atspi.get_desktop(0)
    for i in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(i)
        if app is not None and app.get_name() == "wren":
            print(f"=== wren application (idx {i}) ===")
            dump(app)
            return 0
    print("wren not found in AT-SPI desktop", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
