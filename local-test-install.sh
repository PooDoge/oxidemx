#!/bin/bash
# Local test install — run with: sudo bash local-test-install.sh
# Mirrors install.sh's install_files() but uses the current checkout
# (with the bazzite-atomic-and-supervisor-fix patches) instead of
# cloning upstream into /opt/juhradial-mx.
#
# Atomic image is detected automatically; SHARE_DIR/APP_DIR/ICON_DIR
# resolve to /usr/local/share paths on Bazzite.

set -euo pipefail

if [ "$EUID" -ne 0 ]; then
    echo "Run with: sudo bash $0" >&2
    exit 1
fi

cd "$(dirname "$(readlink -f "$0")")"

BIN_DIR="/usr/local/bin"
SHARE_DIR="/usr/local/share/juhradial"
APP_DIR="/usr/local/share/applications"
ICON_DIR="/usr/local/share/icons/hicolor/scalable/apps"
# install.sh's convention: per-user systemd unit, no sudo write to /usr/lib
INVOKING_USER="${SUDO_USER:-$USER}"
USER_HOME="$(getent passwd "$INVOKING_USER" | cut -d: -f6)"
USER_SYSTEMD_DIR="$USER_HOME/.config/systemd/user"

echo "==> Daemon binary"
install -Dm755 daemon/target/release/juhradiald "$BIN_DIR/juhradiald"

echo "==> Launcher scripts"
install -Dm755 scripts/juhradial-mx.sh        "$BIN_DIR/juhradial-mx"
install -Dm755 scripts/juhradial-settings.sh  "$BIN_DIR/juhradial-settings"

echo "==> Overlay python"
mkdir -p "$SHARE_DIR"
cp -r overlay/*.py "$SHARE_DIR/"

echo "==> Flow module"
rm -rf "$SHARE_DIR/flow"
cp -r overlay/flow "$SHARE_DIR/flow"

echo "==> Locales"
mkdir -p "$SHARE_DIR/locales"
cp -r overlay/locales/* "$SHARE_DIR/locales/"

echo "==> Assets"
mkdir -p "$SHARE_DIR/assets/radial-wheels"
cp -r assets/radial-wheels/*.png "$SHARE_DIR/assets/radial-wheels/"
mkdir -p "$SHARE_DIR/assets/devices"
cp assets/devices/*.png assets/devices/*.svg "$SHARE_DIR/assets/devices/" 2>/dev/null || true
cp assets/ai-*.svg          "$SHARE_DIR/assets/" 2>/dev/null || true
cp assets/os-*.svg          "$SHARE_DIR/assets/" 2>/dev/null || true
cp assets/flow-indicator.png "$SHARE_DIR/assets/" 2>/dev/null || true
cp assets/genericmouse.png  "$SHARE_DIR/assets/" 2>/dev/null || true
cp assets/nav-*.png         "$SHARE_DIR/assets/" 2>/dev/null || true
if [ -d assets/settings-generated ]; then
    mkdir -p "$SHARE_DIR/assets/settings-generated"
    cp assets/settings-generated/control-ring.png "$SHARE_DIR/assets/settings-generated/" 2>/dev/null || true
    cp assets/settings-generated/easyswitch.png   "$SHARE_DIR/assets/settings-generated/" 2>/dev/null || true
    cp assets/settings-generated/haptics.png      "$SHARE_DIR/assets/settings-generated/" 2>/dev/null || true
fi

echo "==> Icon + desktop files"
mkdir -p "$ICON_DIR" "$APP_DIR"
install -Dm644 assets/juhradial-mx.svg "$ICON_DIR/juhradial-mx.svg"
install -Dm644 packaging/juhradial-mx.desktop "$APP_DIR/juhradial-mx.desktop"
install -Dm644 packaging/org.kde.juhradialmx.settings.desktop \
    "$APP_DIR/org.kde.juhradialmx.settings.desktop"

echo "==> systemd user unit (per-user, matches install.sh)"
install -d -o "$INVOKING_USER" -g "$INVOKING_USER" "$USER_SYSTEMD_DIR"
install -m644 -o "$INVOKING_USER" -g "$INVOKING_USER" \
    packaging/systemd/juhradialmx-daemon.service \
    "$USER_SYSTEMD_DIR/juhradialmx-daemon.service"

echo "==> udev rules"
install -Dm644 packaging/udev/99-juhradialmx.rules    /etc/udev/rules.d/99-juhradialmx.rules
install -Dm644 packaging/udev/60-ydotool-uinput.rules /etc/udev/rules.d/60-ydotool-uinput.rules
udevadm control --reload-rules
udevadm trigger

echo ""
echo "Install complete. Now run (as your user, NOT sudo):"
echo "  systemctl --user daemon-reload"
echo "  systemctl --user enable --now juhradialmx-daemon.service"
echo "  systemctl --user status juhradialmx-daemon --no-pager"
echo ""
echo "Then check the journal for any 'Unknown key' warnings:"
echo "  journalctl --user -u juhradialmx-daemon --since '2 min ago' | grep -i 'unknown key' || echo 'no warnings'"
