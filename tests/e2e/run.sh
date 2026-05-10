#!/usr/bin/env bash
# Wrapper for the wren E2E test suite.
#
# Boots Xvfb on :99, exports the AT-SPI bus the GTK app needs, and runs
# pytest with the rest of the args.
#
# Looks for system Xvfb / xdotool / pyatspi first; falls back to the
# locally-extracted RPMs in /tmp/wren-test-tools (see tests/e2e/README.md
# for how to populate that without root).

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"

# ---------- locate Xvfb / xdotool / pyatspi ----------

LOCAL_TOOLS="${WREN_E2E_TOOLS:-/tmp/wren-test-tools}"

find_bin() {
    local name="$1"
    if command -v "$name" >/dev/null 2>&1; then
        command -v "$name"
    elif [[ -x "$LOCAL_TOOLS/usr/bin/$name" ]]; then
        echo "$LOCAL_TOOLS/usr/bin/$name"
    else
        echo "ERROR: $name not found in PATH or $LOCAL_TOOLS/usr/bin/" >&2
        echo "       See tests/e2e/README.md for setup instructions." >&2
        exit 1
    fi
}

XVFB="$(find_bin Xvfb)"
XDOTOOL="$(find_bin xdotool)"

# pyatspi is imported via PYTHONPATH if not system-installed.
if python3 -c "import pyatspi" 2>/dev/null; then
    EXTRA_PYTHONPATH=""
else
    if [[ -d "$LOCAL_TOOLS/usr/lib/python3.14/site-packages" ]]; then
        EXTRA_PYTHONPATH="$LOCAL_TOOLS/usr/lib/python3.14/site-packages"
    elif [[ -d "$LOCAL_TOOLS/usr/lib/python3/site-packages" ]]; then
        EXTRA_PYTHONPATH="$LOCAL_TOOLS/usr/lib/python3/site-packages"
    else
        echo "ERROR: pyatspi not importable. Install python3-pyatspi or" >&2
        echo "       extract the RPM under $LOCAL_TOOLS." >&2
        exit 1
    fi
fi

# xdotool needs libxdo.so.3 — point at it if we extracted it locally.
if [[ -d "$LOCAL_TOOLS/usr/lib64" ]]; then
    LD_LIBRARY_PATH="$LOCAL_TOOLS/usr/lib64${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export LD_LIBRARY_PATH
fi

export WREN_E2E_XDOTOOL="$XDOTOOL"
export WREN_E2E_LD_LIBRARY_PATH="${LD_LIBRARY_PATH:-}"
export PYTHONPATH="$HERE${EXTRA_PYTHONPATH:+:$EXTRA_PYTHONPATH}${PYTHONPATH:+:$PYTHONPATH}"

# ---------- ensure wren is built ----------

if [[ ! -x "$REPO/target/release/wren" ]]; then
    echo "Building wren (release)..."
    (cd "$REPO" && cargo build --release)
fi

# ---------- start Xvfb ----------

DISPLAY_NUM="${WREN_E2E_DISPLAY:-99}"
LOCKFILE="/tmp/.X${DISPLAY_NUM}-lock"
if [[ -e "$LOCKFILE" ]]; then
    # If a stale Xvfb is around, take it down first.
    XOLD=$(cat "$LOCKFILE" 2>/dev/null || true)
    if [[ -n "$XOLD" ]] && kill -0 "$XOLD" 2>/dev/null; then
        kill "$XOLD" 2>/dev/null || true
        sleep 0.3
    fi
    rm -f "$LOCKFILE"
fi

"$XVFB" ":$DISPLAY_NUM" -screen 0 1280x800x24 -nolisten tcp -noreset >/dev/null 2>&1 &
XVFB_PID=$!
trap 'kill $XVFB_PID 2>/dev/null || true' EXIT INT TERM

# Wait for the X server to be ready.
for _ in {1..50}; do
    if DISPLAY=":$DISPLAY_NUM" "$XDOTOOL" getdisplaygeometry >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done

export DISPLAY=":$DISPLAY_NUM"

# Need a session bus for at-spi. The user's existing one is fine; we just
# need to make sure DBUS_SESSION_BUS_ADDRESS is set.
if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]]; then
    if [[ -S "/run/user/$(id -u)/bus" ]]; then
        export DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$(id -u)/bus"
    fi
fi

# ---------- run pytest ----------

cd "$HERE"
exec python3 -m pytest "$@"
