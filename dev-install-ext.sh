#!/usr/bin/env bash
# Dev-mode GNOME extension installer for juhradial-cursor.
#
# Why a separate script: the canonical install.sh does a full system
# install (deps + binaries + udev + desktop entries). For local dev
# work we just want the extension files refreshed in
# ~/.local/share/gnome-shell/extensions/<uuid>/ so the overlay can
# call the latest D-Bus methods. Pure user-space — no sudo, no system
# changes, safe on rpm-ostree atomic distros.
#
# What it does:
#   1. Copies extension.js + metadata.json from gnome-extension/<uuid>/
#      into ~/.local/share/gnome-shell/extensions/<uuid>/
#   2. Tries a hot-reload via `gnome-extensions disable; enable` so
#      the new D-Bus methods become callable without a Shell restart.
#   3. Probes the new method via `busctl --user introspect` to confirm
#      it's actually registered. If not, prints the logout/login hint.
#
# Why this works on Wayland: gnome-extensions disable/enable triggers
# a fresh `enable()` call on the extension class, which re-runs the
# D-Bus name registration. New methods added to the DBUS_IFACE XML
# pick up on that re-registration without needing a Shell restart.
# (Module-level imports are cached by GJS, but module-level *constants*
# like our DBUS_IFACE string are read again at enable-time.)
#
# Usage:
#   ./dev-install-ext.sh                # install + hot-reload + verify
#   ./dev-install-ext.sh --no-reload    # copy only, leave extension state alone
#   ./dev-install-ext.sh --uninstall    # remove ~/.local copy + disable

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXT_UUID="juhradial-cursor@dev.juhlabs.com"
EXT_SRC="$ROOT/gnome-extension/$EXT_UUID"
EXT_DEST="$HOME/.local/share/gnome-shell/extensions/$EXT_UUID"

# ── Output helpers ─────────────────────────────────────────────────
BOLD='\033[1m'; DIM='\033[2m'; RESET='\033[0m'
GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; CYAN='\033[0;36m'
say()  { echo -e "  ${CYAN}→${RESET} $*"; }
ok()   { echo -e "  ${GREEN}✓${RESET} $*"; }
warn() { echo -e "  ${YELLOW}!${RESET} ${YELLOW}$*${RESET}"; }
err()  { echo -e "  ${RED}✗${RESET} ${RED}$*${RESET}" >&2; }

# ── Pre-flight ─────────────────────────────────────────────────────
require_gnome() {
  if [[ "${XDG_CURRENT_DESKTOP:-}" != *"GNOME"* ]]; then
    warn "XDG_CURRENT_DESKTOP=${XDG_CURRENT_DESKTOP:-unset} doesn't look like GNOME."
    warn "Continuing anyway — the extension only does anything inside Mutter."
  fi
}

require_source() {
  if [[ ! -d "$EXT_SRC" ]]; then
    err "Source not found: $EXT_SRC"
    err "Run this from a juhradial-mx workspace checkout."
    exit 1
  fi
  for f in extension.js metadata.json; do
    if [[ ! -f "$EXT_SRC/$f" ]]; then
      err "Missing $f in $EXT_SRC"
      exit 1
    fi
  done
}

# ── Operations ─────────────────────────────────────────────────────
copy_files() {
  mkdir -p "$EXT_DEST"
  # Copy each file individually so any read failure surfaces clearly.
  cp "$EXT_SRC/metadata.json" "$EXT_DEST/metadata.json"
  cp "$EXT_SRC/extension.js"  "$EXT_DEST/extension.js"
  ok "Copied extension files → $EXT_DEST"
  say "extension.js: $(wc -c < "$EXT_DEST/extension.js") bytes"
}

# Disable + enable cycle. Quiet about failures because the user might
# be running this before the extension was ever installed (first run).
hot_reload() {
  if ! command -v gnome-extensions >/dev/null 2>&1; then
    warn "gnome-extensions CLI not found — skipping hot-reload."
    warn "Log out and back in for changes to take effect."
    return 0
  fi

  local state
  state=$(gnome-extensions info "$EXT_UUID" 2>/dev/null | grep -oP '(?<=State: )\S+' || echo "")
  case "$state" in
    "")
      say "Extension is not yet known to GNOME Shell. Enabling for the first time…"
      gnome-extensions enable "$EXT_UUID" 2>/dev/null || {
        warn "Initial enable failed. Log out and back in, then re-run."
        return 0
      }
      ok "Enabled (first activation)."
      ;;
    ACTIVE|ENABLED)
      say "Hot-reloading: disable → enable cycle…"
      gnome-extensions disable "$EXT_UUID" 2>/dev/null || true
      sleep 0.3
      gnome-extensions enable "$EXT_UUID" 2>/dev/null || {
        warn "Re-enable failed; check 'journalctl --user -t gnome-shell'."
        return 0
      }
      ok "Disable/enable cycle complete."
      ;;
    *)
      say "Extension state: $state — enabling…"
      gnome-extensions enable "$EXT_UUID" 2>/dev/null || true
      ;;
  esac
}

# Probe the GetFocusedWindowClass method via D-Bus. If the method is
# present in the introspection XML, the extension is loaded and the
# new code is live. If absent, GJS held a cached copy and the user
# needs a Shell restart (logout/login on Wayland).
verify_dbus() {
  if ! command -v busctl >/dev/null 2>&1; then
    say "busctl not available — skipping D-Bus verification."
    return 0
  fi
  local introspect
  introspect=$(busctl --user introspect \
    org.juhradial.CursorHelper /org/juhradial/CursorHelper 2>/dev/null || echo "")
  if [[ -z "$introspect" ]]; then
    warn "Couldn't reach org.juhradial.CursorHelper — extension not active yet."
    warn "If you just installed for the first time, log out and back in."
    return 0
  fi
  if echo "$introspect" | grep -q "GetFocusedWindowClass"; then
    ok "GetFocusedWindowClass method is registered on the session bus."
  else
    warn "Extension is active but missing GetFocusedWindowClass method."
    warn "GJS may have cached the old extension.js. Logout + login should fix it."
    warn "Workaround: 'busctl --user list | grep CursorHelper' to confirm running."
  fi
}

# ── Subcommands ────────────────────────────────────────────────────
do_install() {
  require_gnome
  require_source
  copy_files
  if [[ "${1:-}" != "--no-reload" ]]; then
    hot_reload
    verify_dbus
  fi
  echo
  echo -e "  ${BOLD}Done.${RESET} If a method is missing on Wayland, log out and back in."
}

do_uninstall() {
  if command -v gnome-extensions >/dev/null 2>&1; then
    say "Disabling extension…"
    gnome-extensions disable "$EXT_UUID" 2>/dev/null || true
  fi
  if [[ -d "$EXT_DEST" ]]; then
    rm -rf "$EXT_DEST"
    ok "Removed $EXT_DEST"
  else
    say "Nothing to remove ($EXT_DEST does not exist)."
  fi
}

usage() {
  cat <<USAGE
Dev-mode GNOME extension installer for $EXT_UUID

  $0                  install + hot-reload + verify
  $0 --no-reload      copy files only, don't touch extension state
  $0 --uninstall      disable + remove the user-local copy

Source:    $EXT_SRC
Dest:      $EXT_DEST
USAGE
}

case "${1:-}" in
  ""|--reload)     do_install ;;
  --no-reload)     do_install --no-reload ;;
  --uninstall|-u)  do_uninstall ;;
  -h|--help|help)  usage ;;
  *) err "Unknown arg: $1"; usage; exit 1 ;;
esac
