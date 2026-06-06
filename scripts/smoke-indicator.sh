#!/usr/bin/env bash
# Smoke-test the indicator + popup + daemon stack post-install.
#
# Run after the steps in docs/live-test.md. Pure read-only:
# only probes D-Bus, systemd, gnome-extensions — no state changes.
#
# Exit codes:
#   0  all probes passed
#   1  one or more probes failed (details printed)
#   2  prerequisite missing (busctl / gnome-extensions / systemctl)

set -u

BOLD='\033[1m'; DIM='\033[2m'; RESET='\033[0m'
GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; CYAN='\033[0;36m'; GRAY='\033[0;90m'

ok()    { echo -e "  ${GREEN}✓${RESET} $*"; }
fail()  { echo -e "  ${RED}✗${RESET} ${RED}$*${RESET}"; FAILED=$((FAILED+1)); }
warn()  { echo -e "  ${YELLOW}!${RESET} ${YELLOW}$*${RESET}"; }
step()  { echo; echo -e "  ${CYAN}${BOLD}[$1]${RESET} ${BOLD}$2${RESET}"; echo -e "  ${GRAY}──────────────────────────────────────────────────────────${RESET}"; }
info()  { echo -e "  ${DIM}·${RESET} $*"; }

FAILED=0

for cmd in busctl systemctl gnome-extensions; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "ERROR: required command '$cmd' not on PATH" >&2
    exit 2
  fi
done

step 1 "Prerequisites"

if id -nG | tr ' ' '\n' | grep -qx input; then
  ok "user in 'input' group"
else
  fail "user NOT in 'input' group (daemon needs hidraw access)"
fi

if [[ "${XDG_CURRENT_DESKTOP:-}" == *"GNOME"* ]]; then
  ok "GNOME session (${XDG_CURRENT_DESKTOP})"
else
  warn "non-GNOME session (${XDG_CURRENT_DESKTOP:-unset}) — extension tests may not apply"
fi

step 2 "Daemon (systemd + D-Bus)"

if systemctl --user is-active --quiet oxidemx-daemon.service; then
  ok "oxidemx-daemon.service active"
else
  fail "oxidemx-daemon.service not active: $(systemctl --user is-active oxidemx-daemon.service)"
fi

if busctl --user list 2>/dev/null | awk '{print $1}' | grep -qx 'org.oxidemx.Daemon'; then
  ok "org.oxidemx.Daemon claims session bus"
else
  fail "org.oxidemx.Daemon NOT on session bus"
fi

if busctl --user introspect org.oxidemx.Daemon /org/oxidemx/Daemon 2>/dev/null | grep -q 'GetActiveDeviceState'; then
  ok "GetActiveDeviceState method registered"
else
  fail "GetActiveDeviceState method MISSING from interface"
fi

if busctl --user introspect org.oxidemx.Daemon /org/oxidemx/Daemon 2>/dev/null | grep -q 'ShowPopup'; then
  ok "ShowPopup method registered"
else
  fail "ShowPopup method MISSING from interface"
fi

if busctl --user introspect org.oxidemx.Daemon /org/oxidemx/Daemon 2>/dev/null | grep -q 'EnsureOverlayRunning'; then
  ok "EnsureOverlayRunning method registered"
else
  fail "EnsureOverlayRunning method MISSING from interface"
fi

if busctl --user introspect org.oxidemx.Daemon /org/oxidemx/Daemon 2>/dev/null | grep -q 'DeviceStateChanged'; then
  ok "DeviceStateChanged signal registered"
else
  fail "DeviceStateChanged signal MISSING from interface"
fi

if busctl --user introspect org.oxidemx.Daemon /org/oxidemx/Daemon 2>/dev/null | grep -q 'SetHapticsEnabled'; then
  ok "SetHapticsEnabled method registered (P2.2 follow-up wiring)"
else
  warn "SetHapticsEnabled MISSING — daemon may be a pre-followups build"
fi

step 3 "Device state snapshot"

if reply=$(busctl --user call org.oxidemx.Daemon /org/oxidemx/Daemon org.oxidemx.Daemon GetActiveDeviceState 2>&1); then
  info "$reply"
  ok "GetActiveDeviceState succeeded"
else
  fail "GetActiveDeviceState failed: $reply"
fi

step 4 "GNOME extensions"

cursor_state=$(gnome-extensions show oxidemx-cursor@dev.juhlabs.com 2>/dev/null | awk '/State:/ {print $2}')
if [[ "$cursor_state" == "ENABLED" ]]; then
  ok "oxidemx-cursor extension ENABLED"
else
  fail "oxidemx-cursor extension state = ${cursor_state:-MISSING}"
fi

indicator_state=$(gnome-extensions show oxidemx-indicator@dev.juhlabs.com 2>/dev/null | awk '/State:/ {print $2}')
if [[ "$indicator_state" == "ENABLED" ]]; then
  ok "oxidemx-indicator extension ENABLED"
else
  fail "oxidemx-indicator extension state = ${indicator_state:-MISSING}  (try: gnome-extensions enable oxidemx-indicator@dev.juhlabs.com; if that fails, re-login)"
fi

# Cursor extension service must also be on the bus
if busctl --user list 2>/dev/null | awk '{print $1}' | grep -qx 'org.oxidemx.CursorHelper'; then
  ok "org.oxidemx.CursorHelper on session bus"
else
  fail "org.oxidemx.CursorHelper NOT on bus (cursor extension disabled or stale)"
fi

step 5 "Popup binary on PATH"

if command -v oxidemx-popup >/dev/null 2>&1; then
  ok "oxidemx-popup at $(command -v oxidemx-popup)"
else
  fail "oxidemx-popup NOT on PATH — daemon ShowPopup will fail to spawn"
fi

if command -v oxidemx-settings >/dev/null 2>&1; then
  ok "oxidemx-settings at $(command -v oxidemx-settings)"
else
  fail "oxidemx-settings NOT on PATH — right-click → Open Settings will fail"
fi

step 6 "GSettings schema"

if gsettings list-schemas 2>/dev/null | grep -qx 'org.gnome.shell.extensions.oxidemx-indicator'; then
  ok "org.gnome.shell.extensions.oxidemx-indicator schema discoverable"
else
  fail "GSettings schema NOT compiled. Re-run dev-install-ext.sh."
fi

step 7 "Config + popup config"

if [[ -f "$HOME/.config/oxidemx/config.json" ]]; then
  ok "config.json exists"
  if command -v jq >/dev/null 2>&1; then
    if jq -e '.popup' "$HOME/.config/oxidemx/config.json" >/dev/null 2>&1; then
      ok "config.popup block present"
      info "$(jq -c '.popup' "$HOME/.config/oxidemx/config.json")"
    else
      warn "config.popup block missing — daemon will use defaults"
    fi
    if jq -e '.haptics.enabled' "$HOME/.config/oxidemx/config.json" >/dev/null 2>&1; then
      ok "config.haptics.enabled = $(jq '.haptics.enabled' "$HOME/.config/oxidemx/config.json")"
    fi
  else
    warn "jq not installed — skipping config-content checks"
  fi
else
  warn "no config.json yet — daemon will create one on first run"
fi

echo
if [[ $FAILED -eq 0 ]]; then
  echo -e "  ${GREEN}${BOLD}✓ All probes passed.${RESET}"
  echo
  echo -e "  ${DIM}Next: open the popup by clicking the panel icon, or:${RESET}"
  echo -e "  ${DIM}  busctl --user call org.oxidemx.Daemon /org/oxidemx/Daemon \\${RESET}"
  echo -e "  ${DIM}         org.oxidemx.Daemon ShowPopup iiii 1800 32 32 32${RESET}"
  echo
  exit 0
else
  echo -e "  ${RED}${BOLD}✗ $FAILED probe(s) failed.${RESET}  See docs/live-test.md for setup steps."
  echo
  exit 1
fi
