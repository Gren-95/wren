"""Right-click on a grid cell pops the file context menu."""

import time


def test_right_click_grid_item_opens_context_menu(wren_app):
    """Right-clicking a file cell shows a popover menu with action items."""
    from wren_app import _all  # type: ignore

    # No popover before the right click.
    assert (
        len(_all(wren_app.window(), lambda n: n.get_role_name() == "menu")) == 0
    ), "expected no menu before right-click"

    wren_app.right_click_grid_item("alpha.txt")
    wren_app.wait_until(
        lambda: any(
            mi for mi in _all(
                wren_app.window(), lambda n: n.get_role_name() == "menu item"
            )
        ),
        timeout=5.0,
        msg="right-click on alpha.txt did not open a context menu",
    )
    items = _all(wren_app.window(), lambda n: n.get_role_name() == "menu item")
    # The file context menu has lots of entries (Open, Cut, Copy,
    # Paste, Rename, Move to Trash, Delete, Properties, …). We don't
    # rely on names — they're empty in the AT-SPI tree — but the
    # cardinality is a reliable signal we got the file menu, not the
    # smaller hamburger menu.
    assert len(items) >= 6, f"expected ≥6 file context menu items, got {len(items)}"

    # Escape closes it again.
    wren_app.press("Escape")
    wren_app.wait_until(
        lambda: not _all(
            wren_app.window(), lambda n: n.get_role_name() == "menu item"
        ),
        timeout=3.0,
        msg="Escape did not close the context menu",
    )
