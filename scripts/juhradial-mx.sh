#!/bin/bash
# JuhRadial MX Launcher
# Starts the daemon and overlay for the radial menu.
#
# When the systemd user service `juhradialmx-daemon` is active, this script
# leaves the daemon alone (it's already running and owns the D-Bus name) and
# only (re)starts the overlay. This avoids creating a second daemon process
# that would fight the first for HID++ protocol responses on the same hidraw
# fd — a race that silently breaks button-divert state on the mouse firmware.

# Find script directory (works for both installed and dev mode).
# /usr/local/share is checked first for atomic/immutable Fedora systems
# (Bazzite, Silverblue, Kinoite, Bluefin…) where /usr is read-only.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -d "$SCRIPT_DIR/.git" ]; then
    :
elif [ -d "/usr/local/share/juhradial" ]; then
    SCRIPT_DIR="/usr/local/share/juhradial"
elif [ -d "/usr/share/juhradial" ]; then
    SCRIPT_DIR="/usr/share/juhradial"
elif [ -d "/opt/juhradial-mx" ]; then
    SCRIPT_DIR="/opt/juhradial-mx"
fi

# Locate the overlay script
if [ -f "$SCRIPT_DIR/overlay/juhradial-overlay.py" ]; then
    OVERLAY_SCRIPT="$SCRIPT_DIR/overlay/juhradial-overlay.py"
elif [ -f "$SCRIPT_DIR/juhradial-overlay.py" ]; then
    OVERLAY_SCRIPT="$SCRIPT_DIR/juhradial-overlay.py"
else
    echo "Error: juhradial-overlay.py not found under $SCRIPT_DIR" >&2
    exit 1
fi

# Locate the daemon binary (only used when systemd is not managing it)
if [ -x "/usr/local/bin/juhradiald" ]; then
    DAEMON_BIN="/usr/local/bin/juhradiald"
elif [ -x "/usr/bin/juhradiald" ]; then
    DAEMON_BIN="/usr/bin/juhradiald"
elif [ -x "$SCRIPT_DIR/daemon/target/release/juhradiald" ]; then
    DAEMON_BIN="$SCRIPT_DIR/daemon/target/release/juhradiald"
else
    DAEMON_BIN=""
fi

# If systemd is already running the daemon, only manage the overlay.
# Otherwise fall back to the legacy "kill everything and respawn" behavior.
SYSTEMD_OWNED=false
if command -v systemctl &> /dev/null && \
   systemctl --user is-active --quiet juhradialmx-daemon 2>/dev/null; then
    SYSTEMD_OWNED=true
fi

if [ "$SYSTEMD_OWNED" = true ]; then
    pkill -f "juhradial-overlay" 2>/dev/null
    sleep 0.3

    python3 "$OVERLAY_SCRIPT" &
    OVERLAY_PID=$!

    DAEMON_PID=$(systemctl --user show -p MainPID --value juhradialmx-daemon 2>/dev/null)

    echo "JuhRadial MX started (daemon managed by systemd)"
    echo "  Overlay PID: $OVERLAY_PID"
    echo "  Daemon PID:  $DAEMON_PID (juhradialmx-daemon.service)"

    wait $OVERLAY_PID
else
    pkill -f "juhradiald" 2>/dev/null
    pkill -f "juhradial-overlay" 2>/dev/null
    sleep 0.3

    python3 "$OVERLAY_SCRIPT" &
    OVERLAY_PID=$!

    if [ -n "$DAEMON_BIN" ]; then
        "$DAEMON_BIN" &
        DAEMON_PID=$!
    else
        echo "Warning: juhradiald binary not found; menu will not respond to button presses" >&2
        DAEMON_PID=""
    fi

    echo "JuhRadial MX started"
    echo "  Overlay PID: $OVERLAY_PID"
    echo "  Daemon PID:  ${DAEMON_PID:-<not started>}"

    if [ -n "$DAEMON_PID" ]; then
        wait $DAEMON_PID
    else
        wait $OVERLAY_PID
    fi
fi
