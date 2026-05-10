"""Tab management: Ctrl+T new, Ctrl+W close."""

import time


def _count_page_tabs(wren_app) -> int:
    """Count the AdwTabView's `page tab` accessibles."""
    from wren_app import _all  # type: ignore

    return len(_all(wren_app.window(), lambda n: n.get_role_name() == "page tab"))


def test_new_tab_increases_count(wren_app):
    """Ctrl+T opens a second tab; AdwTabBar exposes both as page tabs."""
    # AdwTabBar is set to autohide=true in window.ui, so it only appears
    # when there are >=2 tabs. Before Ctrl+T, the bar may be empty.
    wren_app.press("ctrl+t")
    wren_app.wait_until(
        lambda: _count_page_tabs(wren_app) >= 2,
        timeout=5.0,
        msg=f"Ctrl+T did not produce a second tab (saw {_count_page_tabs(wren_app)})",
    )


def test_close_tab_returns_to_one(wren_app):
    """Ctrl+T then Ctrl+W returns to a single tab (or none in the bar)."""
    wren_app.press("ctrl+t")
    wren_app.wait_until(
        lambda: _count_page_tabs(wren_app) >= 2,
        timeout=5.0,
        msg="Ctrl+T did not open a second tab",
    )
    wren_app.press("ctrl+w")
    wren_app.wait_until(
        lambda: _count_page_tabs(wren_app) <= 1,
        timeout=5.0,
        msg="Ctrl+W did not close the extra tab",
    )
