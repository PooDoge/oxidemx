#!/usr/bin/env bash
# Dev-mode GNOME extension installer for oxidemx-cursor + oxidemx-indicator.
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
#      For the indicator extension also copies: prefs.js, stylesheet.css,
#      icons/, and schemas/ (then compiles the GSettings schema in-place).
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

# ── Extension: indicator ───────────────────────────────────────────
IND_UUID="oxidemx-indicator@dev.juhlabs.com"
IND_SRC="$ROOT/gnome-extension/$IND_UUID"
IND_DEST="$HOME/.local/share/gnome-shell/extensions/$IND_UUID"

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
  if [[ ! -d "$IND_SRC" ]]; then
    err "Indicator source not found: $IND_SRC"
    err "Run this from a oxidemx workspace checkout."
    exit 1
  fi
  if [[ ! -f "$IND_SRC/metadata.json" ]]; then
    err "Missing metadata.json in $IND_SRC"
    exit 1
  fi
  if [[ ! -f "$IND_SRC/extension.ts" && ! -f "$IND_SRC/extension.js" ]]; then
    err "Missing extension.ts (and no compiled extension.js) in $IND_SRC"
    err "The extension source is TypeScript. Run: cd gnome-extension && npm install && npm run build"
    exit 1
  fi
}

# ── Operations ─────────────────────────────────────────────────────
copy_indicator_files() {
  mkdir -p "$IND_DEST"

  # Core JS + metadata
  cp "$IND_SRC/metadata.json"  "$IND_DEST/metadata.json"
  cp "$IND_SRC/extension.js"   "$IND_DEST/extension.js"
  [[ -f "$IND_SRC/prefs.js" ]] && cp "$IND_SRC/prefs.js" "$IND_DEST/prefs.js"
  [[ -d "$IND_SRC/lib" ]] && cp -af "$IND_SRC/lib" "$IND_DEST/"

  # Stylesheet (panel button CSS classes)
  [[ -f "$IND_SRC/stylesheet.css" ]] && cp "$IND_SRC/stylesheet.css" "$IND_DEST/stylesheet.css"

  # Symbolic icons (new in Phase 1D)
  if [[ -d "$IND_SRC/icons" ]]; then
    mkdir -p "$IND_DEST/icons"
    cp -r "$IND_SRC/icons/." "$IND_DEST/icons/"
    ok "Copied icons/ → $IND_DEST/icons/"
  fi

  # GSettings schema — copy then compile in the INSTALLED dir.
  # Shell loads gschemas.compiled from the installed extension dir, not source.
  if [[ -d "$IND_SRC/schemas" ]]; then
    mkdir -p "$IND_DEST/schemas"
    cp "$IND_SRC/schemas/"*.xml "$IND_DEST/schemas/"
    if command -v glib-compile-schemas >/dev/null 2>&1; then
      glib-compile-schemas "$IND_DEST/schemas/"
      ok "Compiled GSettings schema in $IND_DEST/schemas/"
    else
      warn "glib-compile-schemas not found — schema won't be readable by Shell."
      warn "Install glib2-devel (or equivalent) and re-run to compile schemas."
    fi
  fi

  ok "Copied indicator extension files → $IND_DEST"
  say "extension.js: $(wc -c < "$IND_DEST/extension.js") bytes"
}

hot_reload_one() {
  local uuid="$1"
  local state
  state=$(gnome-extensions info "$uuid" 2>/dev/null | grep -oP '(?<=State: )\S+' || echo "")
  case "$state" in
    "")
      say "Extension $uuid is not yet known to GNOME Shell. Enabling for the first time…"
      gnome-extensions enable "$uuid" 2>/dev/null || {
        warn "Initial enable of $uuid failed. Log out and back in, then re-run."
        return 0
      }
      ok "Enabled $uuid (first activation)."
      ;;
    ACTIVE|ENABLED)
      say "Hot-reloading $uuid: disable → enable cycle…"
      gnome-extensions disable "$uuid" 2>/dev/null || true
      sleep 0.3
      gnome-extensions enable "$uuid" 2>/dev/null || {
        warn "Re-enable of $uuid failed; check journalctl logs."
        return 0
      }
      ok "Disable/enable cycle for $uuid complete."
      ;;
    *)
      say "Extension $uuid state: $state — enabling…"
      gnome-extensions enable "$uuid" 2>/dev/null || true
      ;;
  esac
}

# Disable + enable cycle. Quiet about failures because the user might
# be running this before the extension was ever installed (first run).
hot_reload() {
  if ! command -v gnome-extensions >/dev/null 2>&1; then
    warn "gnome-extensions CLI not found — skipping hot-reload."
    warn "Log out and back in for changes to take effect."
    return 0
  fi

  hot_reload_one "$IND_UUID"
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
    org.oxidemx.CursorHelper /org/oxidemx/CursorHelper 2>/dev/null || echo "")
  if [[ -z "$introspect" ]]; then
    warn "Couldn't reach org.oxidemx.CursorHelper — extension not active yet."
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

# ── TypeScript compile ─────────────────────────────────────────────
# Compile .ts → .js so GNOME Shell can load the extensions.
# Source of truth is .ts in gnome-extension/; .js is gitignored,
# generated alongside the source.
compile_ts() {
  if [ -d "$ROOT/gnome-extension/node_modules" ] || [ -f "$ROOT/gnome-extension/package.json" ]; then
    if ! command -v npx >/dev/null 2>&1; then
      err "npx not found on PATH."
      err "  The GNOME extensions are TypeScript; .js is compiled from .ts."
      err "  Install Node.js (recommended: inside a distrobox)"
      err "  then run \"cd gnome-extension && npm install\" once."
      exit 1
    fi
    if [ ! -d "$ROOT/gnome-extension/node_modules" ]; then
      say "Installing TypeScript build dependencies..."
      (cd "$ROOT/gnome-extension" && npm install) || exit 1
    fi
    say "Compiling TypeScript..."
    (cd "$ROOT/gnome-extension" && npm run build) || exit 1
    ok "TypeScript compile complete."
  fi
}

# ── Subcommands ────────────────────────────────────────────────────
do_install() {
  compile_ts
  require_gnome
  require_source
  copy_indicator_files
  if [[ "${1:-}" != "--no-reload" ]]; then
    hot_reload
    verify_dbus
  fi
  echo
  echo -e "  ${BOLD}Done.${RESET} If a method is missing on Wayland, log out and back in."
}

do_uninstall() {
  if command -v gnome-extensions >/dev/null 2>&1; then
    say "Disabling indicator extension…"
    gnome-extensions disable "$IND_UUID" 2>/dev/null || true
  fi
  if [[ -d "$IND_DEST" ]]; then
    rm -rf "$IND_DEST"
    ok "Removed $IND_DEST"
  else
    say "Nothing to remove ($IND_DEST does not exist)."
  fi
}

usage() {
  cat <<USAGE
Dev-mode GNOME extension installer for $IND_UUID

  $0                  install extension + hot-reload + verify
  $0 --no-reload      copy files only, don't touch extension state
  $0 --uninstall      disable + remove user-local copy

Indicator source:  $IND_SRC
Indicator dest:    $IND_DEST
USAGE
}

case "${1:-}" in
  ""|--reload)     do_install ;;
  --no-reload)     do_install --no-reload ;;
  --uninstall|-u)  do_uninstall ;;
  -h|--help|help)  usage ;;
  *) err "Unknown arg: $1"; usage; exit 1 ;;
esac
