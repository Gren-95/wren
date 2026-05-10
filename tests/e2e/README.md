# wren E2E test suite

Tier-3 integration tests. The cargo unit + integration tests cover model
logic; this layer drives the assembled GTK4 application end-to-end via
the AT-SPI accessibility bus + xdotool, the closest thing to Playwright
for a native GTK app.

## How it works

* **Xvfb** (a virtual X11 server) gives the suite a clean display so the
  tests don't fight the user's session.
* The wren binary launches under that display with a clean
  `XDG_CONFIG_HOME` / `XDG_DATA_HOME` / `XDG_STATE_HOME` so settings
  don't leak between tests.
* **AT-SPI** (queried with `gi.repository.Atspi`) walks the live
  accessibility tree of the running wren — that's how we find the
  hamburger button, the file grid, the breadcrumb, etc.
* **xdotool** synthesises mouse and keyboard input. We use AT-SPI for
  *finding* widgets and xdotool for *acting* on them.
* `dogtail` and `pyatspi` are NOT used directly — we use the lower-level
  `Atspi` GIR binding because it ships in `at-spi2-core` (already a
  hard dep of GTK), no extra package needed. (See "Dependencies" below
  for how we get xdotool / Xvfb.)

## Dependencies

System packages (Fedora / Nobara):

```bash
sudo dnf install xorg-x11-server-Xvfb xdotool at-spi2-core
```

`pytest` is the only Python package; install per-project:

```bash
python3 -m pip install --user pytest
# or in a venv: python3 -m venv .venv && .venv/bin/pip install pytest
```

### No-root fallback

If you can't install system packages (e.g. you're testing on a host
where you don't have sudo), `run.sh` will look in
`$WREN_E2E_TOOLS/usr/bin/` for `Xvfb` and `xdotool`. To populate that
directory without root:

```bash
mkdir /tmp/wren-test-tools && cd /tmp/wren-test-tools
dnf download --resolve xorg-x11-server-Xvfb xdotool python3-pyatspi
for f in *.rpm; do rpm2cpio "$f" | cpio -idmv; done
```

Then `run.sh` just works — it adds `usr/lib64` to `LD_LIBRARY_PATH`
(needed because `xdotool` links `libxdo.so.3`).

## Running

```bash
# build wren first (run.sh will also do this if the binary is missing)
cargo build --release

# run the full suite
./tests/e2e/run.sh

# one test, verbose, stop on first failure
./tests/e2e/run.sh -v -x -k test_new_folder

# run an interactive shell against a fresh wren under Xvfb (for the
# /tests/e2e/dump_tree.py helper)
DISPLAY=:99 ./target/release/wren /tmp & disown
DISPLAY=:99 python3 tests/e2e/dump_tree.py
```

## What's tested

Each test gets a fresh wren launch with its own tempdirs (see
`conftest.py::wren_app`). Order doesn't matter.

| File | Test | Coverage |
|---|---|---|
| `test_window_opens.py` | window_opens | wren registers in AT-SPI, frame name = cwd basename, hamburger + search buttons present |
| `test_window_opens.py` | static_sidebar_rows | Home / Documents / Trash exposed as sidebar list items |
| `test_window_opens.py` | grid_shows_seeded_files | seeded fixtures appear as grid `table cell`s |
| `test_navigation.py` | navigate_via_path_entry | Ctrl+L → type → Enter changes the active directory |
| `test_navigation.py` | back_after_navigation | Alt+Left undoes the navigation |
| `test_navigation.py` | navigate_up_to_parent | Alt+Up goes up |
| `test_search.py` | search_bar_opens | Ctrl+F reveals an entry widget |
| `test_search.py` | search_filters_grid | typing filters the grid |
| `test_search.py` | search_escape_clears | Escape restores the unfiltered grid |
| `test_file_ops.py` | new_folder_via_shortcut | Ctrl+Shift+N → dialog → Tab → name → Enter creates a folder |
| `test_settings.py` | settings_opens_and_closes | hamburger → Settings opens an AdwPreferencesDialog; Escape dismisses it |
| `test_tabs.py` | new_tab_increases_count | Ctrl+T adds a `page tab` |
| `test_tabs.py` | close_tab_returns_to_one | Ctrl+W removes it |
| `test_show_hidden.py` | show_hidden_toggle | Ctrl+H reveals dotfiles, again to hide |
| `test_context_menu.py` | right_click_grid_item_opens_context_menu | right-click on a cell pops a menu with ≥6 items |

Currently skipped:

* `test_file_ops.py::test_undo_new_folder` — Ctrl+Z doesn't reach the
  `win.undo` action under the harness even though the action is enabled
  and the folder was created. Looks like the accel resolution for
  class-installed widget actions only fires when a particular widget
  has keyboard focus and synthesised input under Xvfb leaves focus on
  a non-receiving scrollpane.
* `test_file_ops.py::test_rename_with_f2` — the F2 rename popover
  (`gtk4::Popover` parented to the file's cell) doesn't appear in the
  AT-SPI tree, and its child Entry never receives focus. The dialog
  fallback path works (it's used for non-visible cells).

Both are real test-harness limitations rather than bugs in wren — the
`win.rename` and `win.undo` actions do work in a normal session.

## Known harness gotchas

* **GtkListBox row activation doesn't work from synthesised mouse
  clicks under Xvfb.** Sidebar rows can't be clicked into; the tests
  use Ctrl+L path entry navigation instead. Header bar `GtkButton`s
  and grid `GridView` cells DO respond to clicks normally, so it's
  specifically a GtkListBox / GtkListBoxRow issue under no-WM Xvfb.
* **GTK4 PopoverMenu items expose empty `name` in AT-SPI.** Each
  `menu item` accessible has `name=''`. We work around this by either
  (a) navigating menu items by Down arrow + Enter (`test_settings`)
  or (b) asserting on the *count* of items, not their text
  (`test_context_menu`).
* **`do_action(0)` is unsupported.** GTK4 widgets don't register a
  default AT-SPI action you can fire from Python. We always click
  with xdotool at the widget's reported `get_extents(WINDOW)` instead.
* **Accelerator delivery is per-widget-with-focus.** `Alt+Up`,
  `Ctrl+L`, `Ctrl+F`, `Ctrl+H`, `Ctrl+Shift+N`, `Ctrl+T`, `Ctrl+W`
  all reach the window action group. `Ctrl+Z` doesn't. We don't
  understand exactly why — see the skip reason in `test_file_ops.py`.

If wren's UI is restructured, run `dump_tree.py` against the new build
and update the role-name + name patterns in `wren_app.py` accordingly.
