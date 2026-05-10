"""pytest fixtures: launch wren under Xvfb with isolated XDG dirs."""

from __future__ import annotations

import os
import shutil
import signal
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Iterator

import pytest

from wren_app import WrenApp


# Find the wren binary, relative to the repo root.
REPO_ROOT = Path(__file__).resolve().parents[2]
WREN_BIN = REPO_ROOT / "target" / "release" / "wren"


def _require_display() -> str:
    display = os.environ.get("DISPLAY")
    if not display:
        pytest.fail(
            "DISPLAY is not set. Run tests under Xvfb via tests/e2e/run.sh."
        )
    # Xvfb in our run.sh is :99 by default. We don't *strictly* require :99
    # but we do want to fail loudly if DISPLAY points at the user's real
    # session — refuse to drive the user's mouse and keyboard from tests.
    if display in (":0", ":1") and not os.environ.get("WREN_E2E_ALLOW_REAL_DISPLAY"):
        pytest.fail(
            f"DISPLAY={display!r} looks like a real session. Refusing to "
            "run E2E tests against the user's display. Set "
            "WREN_E2E_ALLOW_REAL_DISPLAY=1 to override (you don't want to)."
        )
    return display


def _populate_test_dir(d: Path) -> None:
    """Seed the test working directory with a known set of files."""
    (d / "alpha.txt").write_text("alpha\n")
    (d / "beta.txt").write_text("beta\n")
    (d / "subdir").mkdir()
    (d / "subdir" / "nested.txt").write_text("nested\n")


@pytest.fixture(scope="function")
def wren_app(tmp_path_factory: pytest.TempPathFactory) -> Iterator[WrenApp]:
    """Per-test isolated wren launch.

    Creates fresh tempdirs for the test working directory and for every XDG
    base dir variable, launches wren pointing at the working directory,
    waits for the AT-SPI tree to expose the wren application + frame,
    yields a WrenApp wrapper, then sigterms wren and removes the tempdirs.
    """
    _require_display()

    if not WREN_BIN.exists():
        pytest.fail(
            f"wren binary not found at {WREN_BIN}. Run `cargo build --release` first."
        )

    workdir = tmp_path_factory.mktemp("wren-cwd")
    _populate_test_dir(workdir)

    xdg_config = tmp_path_factory.mktemp("xdg-config")
    xdg_data = tmp_path_factory.mktemp("xdg-data")
    xdg_state = tmp_path_factory.mktemp("xdg-state")
    xdg_cache = tmp_path_factory.mktemp("xdg-cache")

    env = os.environ.copy()
    env["XDG_CONFIG_HOME"] = str(xdg_config)
    env["XDG_DATA_HOME"] = str(xdg_data)
    env["XDG_STATE_HOME"] = str(xdg_state)
    env["XDG_CACHE_HOME"] = str(xdg_cache)
    # Force GTK to use AT-SPI a11y bridge + X11.
    env["GTK_A11Y"] = "atspi"
    env["GDK_BACKEND"] = "x11"
    # Disable animations to make tests less flaky.
    env["GTK_DEBUG"] = ""
    # Quiet libadwaita color scheme negotiation noise.
    env.setdefault("GTK_THEME", "Adwaita")

    proc = subprocess.Popen(
        [str(WREN_BIN), str(workdir)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    app = WrenApp(proc=proc, cwd=workdir)

    # Wait for wren to register with AT-SPI.
    try:
        app.window(timeout=10.0)
    except TimeoutError:
        proc.terminate()
        proc.wait(timeout=5)
        pytest.fail("wren did not appear in the AT-SPI tree within 10s")

    # Give the UI a beat to populate the model + lay out the views.
    time.sleep(0.4)

    try:
        yield app
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=2)
