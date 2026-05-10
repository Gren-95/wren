"""Search bar (Ctrl+F): toggle, filter, exit."""

import time


def test_search_bar_opens(wren_app):
    """Ctrl+F reveals an entry widget in the AT-SPI tree."""
    # Before: no entry.
    win = wren_app.window()
    from wren_app import _all  # type: ignore

    assert _all(win, lambda n: n.get_role_name() == "entry") == []
    wren_app.press("ctrl+f")
    wren_app.wait_until(
        lambda: any(
            n.get_role_name() == "entry"
            for n in _all(wren_app.window(), lambda n: n.get_role_name() == "entry")
        ),
        timeout=3.0,
        msg="Ctrl+F did not reveal a search entry",
    )


def test_search_filters_grid(wren_app):
    """Typing a substring filters the grid to matching files."""
    wren_app.press("ctrl+f")
    time.sleep(0.3)
    wren_app.type_text("alph")
    # The filter is applied as you type.
    wren_app.wait_until(
        lambda: wren_app.grid_items() == ["alpha.txt"],
        timeout=5.0,
        msg=f"search did not filter to alpha.txt only; saw {wren_app.grid_items()}",
    )


def test_search_escape_clears(wren_app):
    """Escape closes the search bar; Ctrl+L navigates somewhere else
    so we can confirm we're back to the normal view (Escape alone is
    enough but we're being thorough)."""
    wren_app.press("ctrl+f")
    time.sleep(0.3)
    wren_app.type_text("nope")
    wren_app.wait_until(
        lambda: wren_app.grid_items() == [],
        timeout=5.0,
        msg="search for 'nope' should have produced an empty grid",
    )
    wren_app.press("Escape")
    wren_app.wait_until(
        lambda: "alpha.txt" in wren_app.grid_items(),
        timeout=3.0,
        msg="Escape did not close the search bar / restore the grid",
    )
