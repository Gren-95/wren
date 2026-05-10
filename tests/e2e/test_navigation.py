"""Navigation: Ctrl+L path entry, Alt+Up to parent, Alt+Left back."""


def test_navigate_via_path_entry(wren_app):
    """Ctrl+L → type path → Enter changes the active directory."""
    original = wren_app.window().get_name()
    wren_app.navigate_to_path("/tmp")
    wren_app.wait_until(
        lambda: wren_app.window().get_name() == "tmp",
        timeout=5.0,
        msg="window title never became 'tmp' after Ctrl+L navigation",
    )
    assert wren_app.window().get_name() != original


def test_back_after_navigation(wren_app):
    """Alt+Left returns to the previously-shown directory."""
    original = wren_app.window().get_name()
    wren_app.navigate_to_path("/tmp")
    wren_app.wait_until(
        lambda: wren_app.window().get_name() == "tmp",
        timeout=5.0,
        msg="navigation to /tmp did not change frame title",
    )
    wren_app.press("alt+Left")
    wren_app.wait_until(
        lambda: wren_app.window().get_name() == original,
        timeout=5.0,
        msg="Alt+Left did not restore the original directory",
    )


def test_navigate_up_to_parent(wren_app):
    """Alt+Up goes to the parent directory."""
    parent = wren_app.cwd.parent.name  # e.g. cwd is /tmp/.../wren-cwdN, parent is its tmp dir
    wren_app.press("alt+Up")
    wren_app.wait_until(
        lambda: wren_app.window().get_name() == parent,
        timeout=5.0,
        msg=f"Alt+Up did not navigate to parent {parent!r}",
    )
