"""Ctrl+H toggles hidden files in the grid."""

import time


def test_show_hidden_toggle(wren_app):
    """Add a dotfile to the test cwd, then Ctrl+H reveals it."""
    hidden = wren_app.cwd / ".hidden_file"
    hidden.write_text("hidden\n")
    # Ask wren to reload the directory listing — F5 / Ctrl+R reload
    # is wired to win.reload? If not, navigating away and back
    # forces a fresh enumerate.
    wren_app.navigate_to_path(str(wren_app.cwd))
    time.sleep(0.3)

    # Initially hidden file should not be visible.
    items = wren_app.grid_items()
    assert ".hidden_file" not in items, (
        f"hidden file appeared without Ctrl+H: {items}"
    )
    # The visible files should include the seeded ones.
    assert "alpha.txt" in items
    assert "beta.txt" in items

    # Toggle.
    wren_app.press("ctrl+h")
    wren_app.wait_until(
        lambda: ".hidden_file" in wren_app.grid_items(),
        timeout=5.0,
        msg=f"Ctrl+H did not reveal .hidden_file (saw {wren_app.grid_items()})",
    )

    # Toggle off again.
    wren_app.press("ctrl+h")
    wren_app.wait_until(
        lambda: ".hidden_file" not in wren_app.grid_items(),
        timeout=5.0,
        msg="Second Ctrl+H did not hide the dotfile again",
    )
