"""Sanity check: wren actually launches and exposes its window via AT-SPI."""


def test_window_opens(wren_app):
    win = wren_app.window()
    assert win is not None
    # Frame name is the directory's basename.
    assert win.get_name() == wren_app.cwd.name
    # The hamburger menu and search button live in the header bar.
    wren_app.button("Main Menu")
    wren_app.button("Search (Ctrl+F)")


def test_static_sidebar_rows(wren_app):
    labels = wren_app.sidebar_labels()
    # Every wren install has these as static rows in the Places section.
    for required in ("Home", "Documents", "Trash"):
        assert required in labels, f"missing {required!r} in sidebar (got {labels})"


def test_grid_shows_seeded_files(wren_app):
    items = wren_app.grid_items()
    assert "alpha.txt" in items
    assert "beta.txt" in items
    assert "subdir" in items
