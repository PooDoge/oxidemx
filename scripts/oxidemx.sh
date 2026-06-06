#!/bin/bash
# OxideMX MX Launcher
# Starts the daemon and overlay for the radial menu.
#
# When the systemd user service `oxidemx-daemon` is active, this script
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
elif [ -d "/usr/local/share/oxidemx" ]; then
    SCRIPT_DIR="/usr/local/share/oxidemx"
elif [ -d "/usr/share/oxidemx" ]; then
    SCRIPT_DIR="/usr/share/oxidemx"
elif [ -d "/opt/oxidemx" ]; then
    SCRIPT_DIR="/opt/oxidemx"
fi

# Locate the overlay binary
if [ -x "/usr/local/bin/oxidemx-overlay" ]; then
    OVERLAY_BIN="/usr/local/bin/oxidemx-overlay"
elif [ -x "/usr/bin/oxidemx-overlay" ]; then
    OVERLAY_BIN="/usr/bin/oxidemx-overlay"
elif [ -x "$SCRIPT_DIR/target/release/oxidemx-overlay" ]; then
    OVERLAY_BIN="$SCRIPT_DIR/target/release/oxidemx-overlay"
elif [ -x "$SCRIPT_DIR/overlay-rs/target/release/oxidemx-overlay" ]; then
    OVERLAY_BIN="$SCRIPT_DIR/overlay-rs/target/release/oxidemx-overlay"
elif [ -x "$SCRIPT_DIR/target/debug/oxidemx-overlay" ]; then
    OVERLAY_BIN="$SCRIPT_DIR/target/debug/oxidemx-overlay"
else
    echo "Error: oxidemx-overlay binary not found" >&2
    exit 1
fi

# Locate the daemon binary (only used when systemd is not managing it)
if [ -x "/usr/local/bin/oxidemxd" ]; then
    DAEMON_BIN="/usr/local/bin/oxidemxd"
elif [ -x "/usr/bin/oxidemxd" ]; then
    DAEMON_BIN="/usr/bin/oxidemxd"
elif [ -x "$SCRIPT_DIR/daemon/target/release/oxidemxd" ]; then
    DAEMON_BIN="$SCRIPT_DIR/daemon/target/release/oxidemxd"
elif [ -x "$SCRIPT_DIR/target/release/oxidemxd" ]; then
    DAEMON_BIN="$SCRIPT_DIR/target/release/oxidemxd"
elif [ -x "$SCRIPT_DIR/target/debug/oxidemxd" ]; then
    DAEMON_BIN="$SCRIPT_DIR/target/debug/oxidemxd"
else
    DAEMON_BIN=""
fi

# If systemd is already running the daemon, only manage the overlay.
# Otherwise fall back to the legacy "kill everything and respawn" behavior.
SYSTEMD_OWNED=false
if command -v systemctl &> /dev/null && \
   systemctl --user is-active --quiet oxidemx-daemon 2>/dev/null; then
    SYSTEMD_OWNED=true
fi

if [ "$SYSTEMD_OWNED" = true ]; then
    pkill -f "oxidemx-overlay" 2>/dev/null
    sleep 0.3

    "$OVERLAY_BIN" &
    OVERLAY_PID=$!

    DAEMON_PID=$(systemctl --user show -p MainPID --value oxidemx-daemon 2>/dev/null)

    echo "OxideMX MX started (daemon managed by systemd)"
    echo "  Overlay PID: $OVERLAY_PID"
    echo "  Daemon PID:  $DAEMON_PID (oxidemx-daemon.service)"

    wait $OVERLAY_PID
else
    pkill -f "oxidemxd" 2>/dev/null
    pkill -f "oxidemx-overlay" 2>/dev/null
    sleep 0.3

    "$OVERLAY_BIN" &
    OVERLAY_PID=$!

    if [ -n "$DAEMON_BIN" ]; then
        "$DAEMON_BIN" &
        DAEMON_PID=$!
    else
        echo "Warning: oxidemxd binary not found; menu will not respond to button presses" >&2
        DAEMON_PID=""
    fi

    echo "OxideMX MX started"
    echo "  Overlay PID: $OVERLAY_PID"
    echo "  Daemon PID:  ${DAEMON_PID:-<not started>}"

    if [ -n "$DAEMON_PID" ]; then
        wait $DAEMON_PID
    else
        wait $OVERLAY_PID
    fi
fi
