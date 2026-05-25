#!/usr/bin/env bash
# Dev-mode end-to-end test setup for juhradial-mx.
#
# What this does, in order:
#   1. Checks prerequisites (input group for the daemon, GNOME Shell
#      available, source tree intact).
#   2. Builds all three crates inside the distrobox container.
#   3. Refreshes the GNOME extension in ~/.local/share via
#      ./dev-install-ext.sh (hot-reloads via gnome-extensions
#      disable/enable; verifies the new D-Bus method is registered).
#   4. Restarts daemon, overlay, and settings via ./dev.sh.
#   5. Prints next-step hints (logs, manual restart commands).
#
# What this does NOT do:
#   - No `sudo`. No system installs. No rpm-ostree calls. Pure
#     user-space, safe on Bazzite / Silverblue / atomic Fedora.
#   - No udev rule installation. If you don't have
#     /etc/udev/rules.d/99-juhradialmx.rules, run install.sh once for
#     the system bits, then come back here for ongoing dev cycles.
#
# Usage:
#   ./dev-test.sh             # full setup: build → ext → start
#   ./dev-test.sh --no-build  # skip rebuild (useful for iterating on ext only)
#   ./dev-test.sh --no-ext    # skip extension install (binary-only iteration)
#   ./dev-test.sh stop        # stop daemon + overlay + settings
#   ./dev-test.sh status      # show component states (alias for ./dev.sh status)
#
# Env overrides:
#   JUHRADIAL_DISTROBOX  distrobox container name (default: claude_development)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEV="$ROOT/dev.sh"
EXT="$ROOT/dev-install-ext.sh"

BOLD='\033[1m'; DIM='\033[2m'; RESET='\033[0m'
GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; CYAN='\033[0;36m'; GRAY='\033[0;90m'
say()  { echo -e "  ${CYAN}→${RESET} $*"; }
ok()   { echo -e "  ${GREEN}✓${RESET} $*"; }
warn() { echo -e "  ${YELLOW}!${RESET} ${YELLOW}$*${RESET}"; }
err()  { echo -e "  ${RED}✗${RESET} ${RED}$*${RESET}" >&2; }
hr()   { echo -e "  ${GRAY}$(printf '%.0s─' {1..56})${RESET}"; }

step() {
  echo
  echo -e "  ${CYAN}${BOLD}[$1]${RESET} ${BOLD}$2${RESET}"
  hr
}

# ── Pre-flight ─────────────────────────────────────────────────────
check_scripts() {
  for s in "$DEV" "$EXT"; do
    if [[ ! -x "$s" ]]; then
      err "Missing or not executable: $s"
      exit 1
    fi
  done
}

check_input_group() {
  if id -nG | tr ' ' '\n' | grep -qx input; then
    ok "User is in 'input' group — daemon can read /dev/input/event*"
    DAEMON_OK=true
  else
    warn "User '$USER' is NOT in 'input' group."
    warn "The daemon needs hidraw + evdev access. Without it, button"
    warn "events and HID++ controls (DPI / SmartShift) won't work."
    warn "Fix: sudo usermod -aG input $USER  →  log out + back in"
    warn ""
    warn "dev-test.sh will skip starting the daemon. Overlay + settings"
    warn "will still launch but without daemon-driven features."
    DAEMON_OK=false
  fi
}

check_gnome() {
  if [[ "${XDG_CURRENT_DESKTOP:-}" != *"GNOME"* ]]; then
    warn "Not a GNOME session (XDG_CURRENT_DESKTOP=${XDG_CURRENT_DESKTOP:-unset})."
    warn "Skipping GNOME-extension steps."
    EXT_OK=false
    return 0
  fi
  if ! command -v gnome-extensions >/dev/null 2>&1; then
    warn "gnome-extensions CLI not found — skipping extension install."
    EXT_OK=false
    return 0
  fi
  ok "GNOME session detected."
  EXT_OK=true
}

check_udev_rules() {
  if [[ -f /etc/udev/rules.d/99-juhradialmx.rules ]]; then
    ok "udev rules present (/etc/udev/rules.d/99-juhradialmx.rules)"
  else
    warn "udev rules NOT installed. The daemon may not be able to access"
    warn "the Logitech receiver via hidraw. Run install.sh once to set"
    warn "those up; this dev script is pure user-space and won't touch"
    warn "/etc."
  fi
}

# ── Steps ──────────────────────────────────────────────────────────
do_build() {
  step "1/5" "Build (distrobox)"
  "$DEV" build all
}

do_install_ext() {
  step "2/5" "GNOME extension"
  if [[ "$EXT_OK" != true ]]; then
    say "Skipped (no GNOME session)."
    return 0
  fi
  "$EXT"
}

do_stop() {
  step "3/5" "Stop existing instances"
  "$DEV" stop all || true
}

do_start_daemon() {
  step "4/5" "Start daemon"
  if [[ "$DAEMON_OK" != true ]]; then
    say "Skipped (user not in input group)."
    return 0
  fi
  "$DEV" start daemon
  # Give it a moment to claim the D-Bus name + scan hidraw, then sanity-
  # check that org.juhradial.Daemon actually appeared. A failed
  # daemon would leave us starting overlay+settings against a dead
  # bus name and confuse the user later.
  sleep 0.6
  if command -v busctl >/dev/null 2>&1; then
    if busctl --user list 2>/dev/null | grep -q "org.juhradial.Daemon"; then
      ok "Daemon claimed org.juhradial.Daemon on the session bus."
    else
      warn "Daemon started but D-Bus name not yet visible. Tail logs:"
      warn "  ./dev.sh logs daemon -f"
    fi
  fi
}

do_start_uis() {
  step "5/5" "Start overlay + settings"
  "$DEV" start overlay
  "$DEV" start settings
}

print_done() {
  echo
  echo -e "  ${GREEN}${BOLD}╭─────────────────────────────────────────────────╮${RESET}"
  echo -e "  ${GREEN}${BOLD}│   ✓  Dev test environment ready                 │${RESET}"
  echo -e "  ${GREEN}${BOLD}╰─────────────────────────────────────────────────╯${RESET}"
  echo
  echo -e "  ${BOLD}Status${RESET}"
  hr
  "$DEV" status
  echo
  echo -e "  ${BOLD}Useful commands${RESET}"
  hr
  echo -e "  ${DIM}Tail daemon logs${RESET}    ./dev.sh logs daemon -f"
  echo -e "  ${DIM}Tail overlay logs${RESET}   ./dev.sh logs overlay -f"
  echo -e "  ${DIM}Tail settings logs${RESET}  ./dev.sh logs settings -f"
  echo -e "  ${DIM}Restart overlay${RESET}     ./dev.sh restart overlay"
  echo -e "  ${DIM}Stop everything${RESET}     ./dev-test.sh stop"
  echo -e "  ${DIM}Probe focus method${RESET}  busctl --user introspect \\"
  echo -e "                          org.juhradial.CursorHelper /org/juhradial/CursorHelper"
  if [[ "$EXT_OK" = true ]]; then
    echo
    echo -e "  ${YELLOW}${BOLD}Extension hot-reload caveat:${RESET} GJS sometimes caches the"
    echo -e "  old extension.js across disable/enable. If \`busctl introspect\`"
    echo -e "  doesn't list ${BOLD}GetFocusedWindowClass${RESET}, log out and back in."
  fi
  echo
}

# ── Subcommands ────────────────────────────────────────────────────
run_full() {
  echo
  echo -e "  ${BOLD}juhradial-mx dev test setup${RESET}"
  hr
  check_scripts
  check_input_group
  check_gnome
  check_udev_rules

  if [[ "$DO_BUILD" = true ]]; then do_build; else step "1/5" "Build"; say "Skipped (--no-build)"; fi
  if [[ "$DO_EXT"   = true ]]; then do_install_ext; else step "2/5" "GNOME extension"; say "Skipped (--no-ext)"; fi
  do_stop
  do_start_daemon
  do_start_uis
  print_done
}

run_stop() {
  "$DEV" stop all
  echo
  ok "All dev components stopped."
  echo
  echo -e "  ${DIM}Note:${RESET} the GNOME extension stays installed + enabled."
  echo -e "  ${DIM}      Use './dev-install-ext.sh --uninstall' to remove it.${RESET}"
}

run_status() {
  "$DEV" status
}

usage() {
  cat <<USAGE
Dev-mode end-to-end test setup for juhradial-mx.

  $0                  full setup: build → ext → start daemon/overlay/settings
  $0 --no-build       skip rebuild
  $0 --no-ext         skip GNOME extension install
  $0 stop             stop daemon + overlay + settings
  $0 status           show component states
  $0 -h | --help      this message

What this script changes on your system:
  - Writes built binaries under ./target/release/ (cargo)
  - Writes ~/.local/share/gnome-shell/extensions/juhradial-cursor@dev.juhlabs.com/
  - Toggles that extension via 'gnome-extensions disable/enable'
  - Starts processes whose PIDs/logs live under \$XDG_RUNTIME_DIR/juhradial-dev/

What it does NOT change:
  - Nothing under /usr, /opt, or /etc
  - No sudo, no rpm-ostree, no systemd unit installs
USAGE
}

# ── Argument parsing ───────────────────────────────────────────────
DO_BUILD=true
DO_EXT=true
MODE="run"

for arg in "$@"; do
  case "$arg" in
    --no-build)       DO_BUILD=false ;;
    --no-ext)         DO_EXT=false ;;
    stop)             MODE="stop" ;;
    status)           MODE="status" ;;
    -h|--help|help)   usage; exit 0 ;;
    *) err "Unknown arg: $arg"; usage; exit 1 ;;
  esac
done

case "$MODE" in
  run)    run_full ;;
  stop)   run_stop ;;
  status) run_status ;;
esac
