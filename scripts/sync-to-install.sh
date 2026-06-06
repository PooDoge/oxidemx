#!/bin/bash
# Sync dev files to install locations and restart
# Run with: sudo bash scripts/sync-to-install.sh

set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEV_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
INSTALL_DIR="/opt/oxidemx"
SHARE_DIR="/usr/share/oxidemx"

echo "=== Stopping running processes ==="
pkill -x oxidemxd 2>/dev/null || true
pkill -f '[j]uhradial-overlay' 2>/dev/null || true
pkill -f '[j]uhradial-settings' 2>/dev/null || true
sleep 1

echo "=== Building Rust Workspace ==="
cargo build --release

echo "=== Syncing dev -> $INSTALL_DIR ==="

# 3D radial wheel images
mkdir -p "$INSTALL_DIR/assets/radial-wheels"
cp "$DEV_DIR"/assets/radial-wheels/*.png "$INSTALL_DIR/assets/radial-wheels/"

# Device/settings imagery used by the settings dashboard
mkdir -p "$INSTALL_DIR/assets/devices"
cp "$DEV_DIR"/assets/devices/*.png "$DEV_DIR"/assets/devices/*.svg "$INSTALL_DIR/assets/devices/" 2>/dev/null || true
mkdir -p "$INSTALL_DIR/assets/settings-generated"
cp "$DEV_DIR"/assets/settings-generated/control-ring.png "$INSTALL_DIR/assets/settings-generated/" 2>/dev/null || true
cp "$DEV_DIR"/assets/settings-generated/easyswitch.png "$INSTALL_DIR/assets/settings-generated/" 2>/dev/null || true
cp "$DEV_DIR"/assets/settings-generated/haptics.png "$INSTALL_DIR/assets/settings-generated/" 2>/dev/null || true

# Daemon binary
install -Dm755 "$DEV_DIR/target/release/oxidemxd" /usr/local/bin/oxidemxd
install -Dm755 "$DEV_DIR/target/release/oxidemx-overlay" /usr/local/bin/oxidemx-overlay
install -Dm755 "$DEV_DIR/target/release/oxidemx-settings" /usr/local/bin/oxidemx-settings
install -Dm755 "$DEV_DIR/target/release/oxidemx-popup" /usr/local/bin/oxidemx-popup

echo "=== Syncing dev -> $SHARE_DIR ==="
mkdir -p "$SHARE_DIR"

# Assets
mkdir -p "$SHARE_DIR/assets"
cp "$DEV_DIR"/assets/ai-*.svg "$SHARE_DIR/assets/" 2>/dev/null || true
cp "$DEV_DIR"/assets/os-*.svg "$SHARE_DIR/assets/" 2>/dev/null || true
cp "$DEV_DIR"/assets/flow-indicator.png "$SHARE_DIR/assets/" 2>/dev/null || true
cp "$DEV_DIR"/assets/genericmouse.png "$SHARE_DIR/assets/" 2>/dev/null || true
cp "$DEV_DIR"/assets/nav-*.png "$SHARE_DIR/assets/" 2>/dev/null || true
mkdir -p "$SHARE_DIR/assets/devices"
cp "$DEV_DIR"/assets/devices/*.png "$DEV_DIR"/assets/devices/*.svg "$SHARE_DIR/assets/devices/" 2>/dev/null || true
mkdir -p "$SHARE_DIR/assets/settings-generated"
cp "$DEV_DIR"/assets/settings-generated/control-ring.png "$SHARE_DIR/assets/settings-generated/" 2>/dev/null || true
cp "$DEV_DIR"/assets/settings-generated/easyswitch.png "$SHARE_DIR/assets/settings-generated/" 2>/dev/null || true
cp "$DEV_DIR"/assets/settings-generated/haptics.png "$SHARE_DIR/assets/settings-generated/" 2>/dev/null || true

echo ""
echo "Done! Use your keyboard shortcut to start OxideMX MX."
