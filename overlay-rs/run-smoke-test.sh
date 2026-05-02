#!/bin/bash
# Smoke-test script for the Rust overlay rewrite.
#
# Prerequisites (Bazzite / atomic Fedora):
#
#   sudo rpm-ostree install \
#       cairo-devel gdk-pixbuf2-devel gtk4-devel \
#       gtk4-layer-shell-devel pango-devel glib2-devel
#   sudo systemctl reboot
#
# What this does:
#   1. Builds juhradial-overlay-rs in release mode.
#   2. Confirms the daemon is running (or warns that you'll see no menu).
#   3. Launches the overlay in the foreground with verbose tracing so
#      D-Bus events are visible.
#
# Quit the overlay with Ctrl-C in the terminal it's running in.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> cargo build (release)"
cargo build --release -p juhradial-overlay-rs

echo ""
echo "==> daemon health"
if systemctl --user is-active --quiet juhradialmx-daemon.service; then
    PID=$(systemctl --user show -p MainPID --value juhradialmx-daemon.service)
    echo "    juhradialmx-daemon active (PID $PID)"
else
    echo "    juhradialmx-daemon NOT running — the overlay will load but"
    echo "    won't receive any MenuRequested signals. Start it with:"
    echo "      systemctl --user start juhradialmx-daemon.service"
fi

echo ""
echo "==> launching overlay (RUST_LOG=info,juhradial_overlay_rs=debug)"
echo "    Press the gesture button on your MX Master to test."
echo "    Ctrl-C here to quit."
echo ""

exec env RUST_LOG="info,juhradial_overlay_rs=debug" \
    ./target/release/juhradial-overlay-rs
