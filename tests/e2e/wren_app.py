"""High-level helpers for driving wren via AT-SPI + xdotool.

# AT-SPI tree shape we found by inspection

Run `dump_tree.py` (in this directory) against a live wren under Xvfb to
reproduce. Summary as of the time these tests were written:

- Application root: role=`application`, name=`wren`
- Main window: role=`frame`, name=<the basename of the current directory>
  (e.g. when wren is opened on `/tmp/foo`, the frame name is `'foo'`).
- Sidebar: a single `list` somewhere inside the window. Each row is a
  `list item` whose own `name` is empty; the visible label is in a
  `label` child two levels deep:
      list item
        panel
          image
          label name='Documents'
  Some rows are header rows ("Recent", "Bookmarks", "Devices") — those
  put the label directly under the list item, with no panel wrapper.
- Grid view: role=`table`, with each file/folder a `table cell` whose
  `name` is the filename (e.g. `'alpha.txt'`).
- List view: role=`tree table` with `table cell` rows (same naming).
- Breadcrumb crumbs: each path segment is a `button` whose `name` is the
  segment text (e.g. `'/'`, `'tmp'`, `'wren-e2e-test'`). The current
  directory itself is a `label`, not a button.
- Header bar: `button` / `toggle button` named after their tooltip:
  `'Toggle Sidebar'`, `'Back (Alt+Left)'`, `'Forward (Alt+Right)'`,
  `'View Options'`, `'Search (Ctrl+F)'`, `'Main Menu'`.
- Hamburger popover menu: opens as `menu` named `'Main Menu'`. The
  individual `menu item` entries unfortunately have empty names — this
  is a known GTK4 PopoverMenu a11y limitation. We fall back to indexing
  by position within the popover panels (see `click_menu_item_at`).
- Tab bar: AdwTabBar — tabs are `page tab`s, but their accessible names
  are also empty in libadwaita 0.9 (named only as the active page).

# Coordinates under Xvfb

There is no window manager under Xvfb, so `get_extents(SCREEN)` and
`get_extents(WINDOW)` return identical coordinates and they match what
xdotool sends. We use `WINDOW` to be explicit.

# Why xdotool and not pyatspi `do_action(0)`

`do_action(0)` returns `No action with index 0` for almost everything in
this app — neither GTK4's `GtkButton` nor `GtkListBoxRow` register a
default AT-SPI action in the way `do_action` expects. We use xdotool
mouse clicks at the element's reported extents instead. For keyboard
shortcuts (Ctrl+F, F2, etc.) we likewise drive xdotool directly.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import time
from pathlib import Path
from typing import Callable, Iterable, Optional

import gi  # type: ignore

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # type: ignore  # noqa: E402


# Path to the bundled xdotool (resolved by conftest.py and stuffed into env).
XDOTOOL = os.environ.get("WREN_E2E_XDOTOOL", "xdotool")
LD_LIBRARY_PATH = os.environ.get("WREN_E2E_LD_LIBRARY_PATH", "")


def _xdotool_env() -> dict[str, str]:
    env = os.environ.copy()
    if LD_LIBRARY_PATH:
        env["LD_LIBRARY_PATH"] = LD_LIBRARY_PATH
    return env


def _xdotool(*args: str) -> None:
    subprocess.run([XDOTOOL, *args], check=True, env=_xdotool_env())


# ---------------------------------------------------------------------------
# AT-SPI tree walking helpers
# ---------------------------------------------------------------------------


def _children(acc: Atspi.Accessible) -> Iterable[Atspi.Accessible]:
    n = acc.get_child_count()
    for i in range(n):
        c = acc.get_child_at_index(i)
        if c is not None:
            yield c


def _walk(acc: Atspi.Accessible, max_depth: int = 30, depth: int = 0):
    if depth > max_depth:
        return
    yield acc
    for c in _children(acc):
        yield from _walk(c, max_depth, depth + 1)


def _find_first(
    acc: Atspi.Accessible,
    pred: Callable[[Atspi.Accessible], bool],
    max_depth: int = 30,
) -> Optional[Atspi.Accessible]:
    for n in _walk(acc, max_depth):
        try:
            if pred(n):
                return n
        except Exception:
            continue
    return None


def _all(
    acc: Atspi.Accessible,
    pred: Callable[[Atspi.Accessible], bool],
    max_depth: int = 30,
) -> list[Atspi.Accessible]:
    out: list[Atspi.Accessible] = []
    for n in _walk(acc, max_depth):
        try:
            if pred(n):
                out.append(n)
        except Exception:
            continue
    return out


def _list_item_label(li: Atspi.Accessible) -> Optional[str]:
    """Return the visible text of a sidebar list item.

    The wren sidebar wraps row content in either:
        list item -> panel -> [image, label]    (nav rows)
        list item -> label                      (section headers)
    """
    if li.get_child_count() == 0:
        return None
    first = li.get_child_at_index(0)
    if first is None:
        return None
    if first.get_role_name() == "label":
        return first.get_name()
    if first.get_role_name() == "panel":
        for kid in _children(first):
            if kid.get_role_name() == "label":
                return kid.get_name()
    return None


# ---------------------------------------------------------------------------
# WrenApp
# ---------------------------------------------------------------------------


class WrenApp:
    """Wrapper around a running wren subprocess + its AT-SPI tree."""

    def __init__(self, proc: subprocess.Popen, cwd: Path):
        self.proc = proc
        self.cwd = cwd

    # -- root finders ------------------------------------------------------

    def app(self, timeout: float = 10.0) -> Atspi.Accessible:
        """Return the application accessible (waits for it to appear)."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            desktop = Atspi.get_desktop(0)
            for i in range(desktop.get_child_count()):
                a = desktop.get_child_at_index(i)
                if a is not None and a.get_name() == "wren":
                    return a
            time.sleep(0.1)
        raise TimeoutError("wren app did not appear in AT-SPI desktop")

    def window(self, timeout: float = 10.0) -> Atspi.Accessible:
        """Return the main wren `frame` accessible."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                app = self.app(timeout=0.5)
                frame = _find_first(app, lambda n: n.get_role_name() == "frame")
                if frame is not None:
                    return frame
            except TimeoutError:
                pass
            time.sleep(0.1)
        raise TimeoutError("wren window frame did not appear")

    # -- finding things by name -------------------------------------------

    def button(self, label: str) -> Atspi.Accessible:
        win = self.window()
        for b in _all(win, lambda n: n.get_role_name() in ("button", "toggle button")):
            if b.get_name() == label:
                return b
        raise LookupError(f"no button named {label!r}")

    def sidebar_rows(self) -> list[tuple[str, Atspi.Accessible]]:
        """Returns [(label, accessible), ...] for every sidebar entry."""
        win = self.window()
        out = []
        for li in _all(win, lambda n: n.get_role_name() == "list item"):
            label = _list_item_label(li)
            if label is not None:
                out.append((label, li))
        return out

    def sidebar_labels(self) -> list[str]:
        return [label for label, _ in self.sidebar_rows()]

    def grid_items(self) -> list[str]:
        """Names of all visible cells in the file view (grid or list)."""
        win = self.window()
        # Both GridView (role=table) and ListView/ColumnView (role=tree table)
        cells = _all(
            win,
            lambda n: n.get_role_name() in ("table", "tree table")
            and n.get_child_count() > 0,
        )
        out: list[str] = []
        for tbl in cells:
            for cell in _children(tbl):
                if cell.get_role_name() in ("table cell",):
                    name = cell.get_name() or ""
                    if name:
                        out.append(name)
            if out:
                break  # only the first non-empty file-view counts
        return out

    def breadcrumb_segments(self) -> list[str]:
        """Visible path segments in the breadcrumb (everything except the
        current dir, which is a label)."""
        win = self.window()
        # Header bar buttons we DON'T want to count as breadcrumb crumbs.
        non_crumb = {
            "Toggle Sidebar",
            "Back (Alt+Left)",
            "Forward (Alt+Right)",
            "View Options",
            "Search (Ctrl+F)",
            "Main Menu",
            "Minimize",
            "Maximize",
            "Close",
            "Edit Path",
            "Edit",
        }
        out = []
        for b in _all(win, lambda n: n.get_role_name() == "button"):
            name = b.get_name()
            if name and name not in non_crumb:
                out.append(name)
        return out

    def breadcrumb_text(self) -> str:
        """The current directory label inside the breadcrumb."""
        # The current dir is shown as a bold label. Look for any label that
        # equals the basename of the current frame.
        frame_name = self.window().get_name()
        return frame_name

    # -- input -------------------------------------------------------------

    def click_accessible(self, acc: Atspi.Accessible, button: int = 1) -> None:
        """Click in the centre of an accessible's reported extents."""
        ext = acc.get_extents(Atspi.CoordType.WINDOW)
        if ext.width == 0 or ext.height == 0:
            raise RuntimeError(
                f"accessible {acc.get_role_name()}/{acc.get_name()!r} has no extents"
            )
        cx = ext.x + ext.width // 2
        cy = ext.y + ext.height // 2
        _xdotool("mousemove", str(cx), str(cy), "click", str(button))

    def click_sidebar(self, label: str) -> None:
        for row_label, row in self.sidebar_rows():
            if row_label == label:
                self.click_accessible(row)
                return
        raise LookupError(f"no sidebar row {label!r}")

    def click_button(self, label: str) -> None:
        self.click_accessible(self.button(label))

    def right_click_grid_item(self, name: str) -> None:
        win = self.window()
        for tbl in _all(win, lambda n: n.get_role_name() in ("table", "tree table")):
            for cell in _children(tbl):
                if (
                    cell.get_role_name() == "table cell"
                    and cell.get_name() == name
                ):
                    self.click_accessible(cell, button=3)
                    return
        raise LookupError(f"no grid item {name!r}")

    def click_grid_item(self, name: str) -> None:
        win = self.window()
        for tbl in _all(win, lambda n: n.get_role_name() in ("table", "tree table")):
            for cell in _children(tbl):
                if (
                    cell.get_role_name() == "table cell"
                    and cell.get_name() == name
                ):
                    self.click_accessible(cell)
                    return
        raise LookupError(f"no grid item {name!r}")

    def right_click_grid_empty(self) -> None:
        """Right-click in an empty area of the grid view."""
        win = self.window()
        tbl = _find_first(
            win, lambda n: n.get_role_name() in ("table", "tree table")
        )
        if tbl is None:
            raise RuntimeError("no grid/list view found")
        ext = tbl.get_extents(Atspi.CoordType.WINDOW)
        # Click near the bottom of the view, away from cells.
        cx = ext.x + ext.width // 2
        cy = ext.y + ext.height - 30
        _xdotool("mousemove", str(cx), str(cy), "click", "3")

    def type_text(self, text: str) -> None:
        _xdotool("type", "--delay", "30", text)

    def press(self, key: str) -> None:
        _xdotool("key", "--delay", "30", key)

    # -- sync helpers -----------------------------------------------------

    def wait_until(
        self,
        predicate: Callable[[], bool],
        timeout: float = 5.0,
        interval: float = 0.1,
        msg: str = "predicate never became true",
    ) -> None:
        deadline = time.monotonic() + timeout
        last_exc: Optional[BaseException] = None
        while time.monotonic() < deadline:
            try:
                if predicate():
                    return
            except Exception as e:  # AT-SPI is racy; retry
                last_exc = e
            time.sleep(interval)
        raise AssertionError(
            f"{msg} after {timeout}s" + (f" (last exc: {last_exc})" if last_exc else "")
        )

    # -- popover menu helpers --------------------------------------------

    def open_main_menu(self) -> Atspi.Accessible:
        """Click the hamburger button and return the opened `menu` accessible."""
        self.click_button("Main Menu")
        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline:
            win = self.window()
            menus = _all(win, lambda n: n.get_role_name() == "menu" and n.get_name() == "Main Menu" and n.get_child_count() > 0)
            if menus:
                return menus[0]
            time.sleep(0.1)
        raise TimeoutError("Main Menu popover did not open")

    def menu_items_in(self, menu: Atspi.Accessible) -> list[Atspi.Accessible]:
        """All `menu item` descendants of a menu accessible (in DOM order)."""
        return _all(menu, lambda n: n.get_role_name() == "menu item")

    # -- navigation helpers ----------------------------------------------

    def navigate_to_path(self, path: str, settle: float = 1.0) -> None:
        """Navigate the active tab to *path* via the path entry (Ctrl+L).

        We use this instead of clicking sidebar rows because under Xvfb
        (no window manager) GTK4's GtkListBox does not reliably activate
        rows from synthesised mouse events. Keyboard input goes through
        the same code path as the user's, so this is robust.
        """
        self.press("ctrl+l")
        time.sleep(0.2)
        # Select-all first so we replace whatever's in the entry.
        self.press("ctrl+a")
        self.type_text(path)
        self.press("Return")
        time.sleep(settle)

