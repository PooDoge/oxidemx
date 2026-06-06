#!/bin/bash
#
# OxideMX MX Settings Launcher
# https://github.com/JuhLabs/oxidemx
#

# Try installed location first, then fall back to local development.
# /usr/local/share is checked first for atomic/immutable Fedora systems
# (Bazzite, Silverblue, Kinoite, Bluefin…) where /usr is read-only.
if [ -x "/usr/local/bin/oxidemx-settings" ]; then
    exec /usr/local/bin/oxidemx-settings "$@"
elif [ -x "/usr/bin/oxidemx-settings" ]; then
    exec /usr/bin/oxidemx-settings "$@"
elif [ -x "$(dirname "$0")/target/release/oxidemx-settings" ]; then
    exec "$(dirname "$0")/target/release/oxidemx-settings" "$@"
elif [ -x "$(dirname "$0")/settings-rs/target/release/oxidemx-settings" ]; then
    exec "$(dirname "$0")/settings-rs/target/release/oxidemx-settings" "$@"
elif [ -x "$(dirname "$0")/target/debug/oxidemx-settings" ]; then
    exec "$(dirname "$0")/target/debug/oxidemx-settings" "$@"
else
    echo "Error: oxidemx-settings binary not found"
    echo "Please run the installer or build the workspace"
    exit 1
fi
