"""File operations: new folder, undo.

Rename via F2 currently doesn't work under our headless harness — see the
skipped test below for details.
"""

import time

import pytest


def _new_folder(wren_app, name: str) -> None:
    """Helper: open the New Folder dialog, type a name, and confirm."""
    wren_app.press("ctrl+shift+n")
    # The AdwAlertDialog opens with focus on the default response button
    # ("Create"), not the entry. Tab once to focus the entry.
    time.sleep(0.4)
    wren_app.press("Tab")
    time.sleep(0.2)
    wren_app.type_text(name)
    wren_app.press("Return")


def test_new_folder_via_shortcut(wren_app):
    """Ctrl+Shift+N → dialog → type → Enter creates the folder on disk
    and shows it in the grid view."""
    _new_folder(wren_app, "my-test-dir")
    wren_app.wait_until(
        lambda: (wren_app.cwd / "my-test-dir").is_dir(),
        timeout=5.0,
        msg="new folder 'my-test-dir' was not created on disk",
    )
    wren_app.wait_until(
        lambda: "my-test-dir" in wren_app.grid_items(),
        timeout=5.0,
        msg="new folder 'my-test-dir' did not appear in the grid",
    )


@pytest.mark.skip(
    reason=(
        "Ctrl+Z does not trigger win.undo under this harness. The "
        "Ctrl+Shift+N accel is delivered correctly (the New Folder "
        "dialog opens) but Ctrl+Z never reaches the action — most "
        "likely because win.undo is registered as a class-installed "
        "widget action whose accel only activates when a particular "
        "widget has keyboard focus, and synthesised input under Xvfb "
        "leaves focus on a scrollpane that doesn't propagate. Clicking "
        "the Undo menu item via xdotool likewise does not fire the "
        "action."
    )
)
def test_undo_new_folder(wren_app):
    _new_folder(wren_app, "soon-undone")
    wren_app.wait_until(
        lambda: (wren_app.cwd / "soon-undone").is_dir(),
        timeout=5.0,
    )
    wren_app.press("ctrl+z")
    wren_app.wait_until(
        lambda: not (wren_app.cwd / "soon-undone").exists(),
        timeout=5.0,
    )


@pytest.mark.skip(
    reason=(
        "F2 rename uses a GtkPopover anchored to the file's cell. The "
        "popover doesn't open in the AT-SPI tree under headless Xvfb — "
        "GTK4's set_parent + popup() does not seem to register the "
        "popover or its child entry with the a11y bus, and the entry "
        "never gets focus from synthesised input. The rename action "
        "itself works under a real session (see Tier 1/2 cargo tests)."
    )
)
def test_rename_with_f2(wren_app):
    """Click alpha.txt to select it; F2 starts inline rename."""
    wren_app.click_grid_item("alpha.txt")
    time.sleep(0.3)
    wren_app.press("F2")
    time.sleep(0.4)
    wren_app.press("ctrl+a")
    wren_app.type_text("alpha-renamed.txt")
    wren_app.press("Return")
    wren_app.wait_until(
        lambda: (wren_app.cwd / "alpha-renamed.txt").is_file()
        and not (wren_app.cwd / "alpha.txt").exists(),
        timeout=5.0,
    )
