"""Settings dialog: open via menu, dismiss with Escape."""

import time


def test_settings_opens_and_closes(wren_app):
    """Open the hamburger, navigate to Settings, Enter; dialog appears
    in AT-SPI tree; Escape dismisses it."""
    # Open the hamburger popover via mouse — header bar buttons DO
    # respond to synthesised clicks (unlike GtkListBox rows).
    wren_app.click_button("Main Menu")
    time.sleep(0.5)
    # The popover opens with no item focused. Down arrows move through
    # *enabled* menu items only. On a fresh window, Undo and Redo are
    # disabled (empty stacks), so the visited sequence is:
    #   1. Show Hidden Files
    #   2. Show File Extensions
    #   3. Sort By  (submenu)
    #   4. Settings…
    # That's 4 Down presses to land on Settings.
    for _ in range(4):
        wren_app.press("Down")
    wren_app.press("Return")

    # The Settings dialog is an AdwPreferencesDialog; AT-SPI exposes it
    # as a `dialog` whose name is "Settings".
    from wren_app import _all  # type: ignore

    def has_settings_dialog() -> bool:
        return any(
            d.get_name() == "Settings"
            for d in _all(wren_app.window(), lambda n: n.get_role_name() == "dialog")
        )

    wren_app.wait_until(
        has_settings_dialog,
        timeout=5.0,
        msg="Settings dialog never appeared after menu navigation",
    )

    # Escape should close the dialog.
    wren_app.press("Escape")
    wren_app.wait_until(
        lambda: not has_settings_dialog(),
        timeout=3.0,
        msg="Escape did not close the Settings dialog",
    )
