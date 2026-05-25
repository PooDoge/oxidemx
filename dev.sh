#!/usr/bin/env bash
# Dev convenience runner for the juhradial-mx Rust workspace.
#
# Build steps re-exec inside the distrobox container so we have the
# Rust toolchain + system libs without polluting the host (Bazzite
# is rpm-ostree atomic — `cargo install` to /usr/bin would be a
# pain). Run steps execute the binary directly on the host so it
# can attach to the user's Wayland session and talk to the daemon's
# session-bus signals.
#
# Usage:
#   ./dev.sh start    [overlay|settings|daemon|all]   # default: overlay
#   ./dev.sh stop     [overlay|settings|daemon|all]   # default: all
#   ./dev.sh restart  [overlay|settings|daemon|all]   # default: overlay
#   ./dev.sh status
#   ./dev.sh logs     <overlay|settings|daemon> [-f]
#   ./dev.sh build    [overlay|settings|daemon|all]   # default: all
#
# Note on daemon: needs hidraw + evdev access. If you're not in the
# `input` group ('id -nG | grep input'), the daemon will crash at
# startup trying to open /dev/input/event*. Fix: 'sudo usermod -aG
# input $USER' + log out / back in. dev-test.sh checks this for you.
#
# Env overrides:
#   JUHRADIAL_DISTROBOX  distrobox container name (default: claude_development)
#   JUHRADIAL_LOG        RUST_LOG value (default: info + debug for our crates)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET="$ROOT/target/release"
RUNDIR="${XDG_RUNTIME_DIR:-/tmp}/juhradial-dev"
DISTROBOX="${JUHRADIAL_DISTROBOX:-claude_development}"
LOG_LEVEL="${JUHRADIAL_LOG:-info,juhradial_overlay_rs=debug,juhradial_settings=debug,usvg=error}"

mkdir -p "$RUNDIR"

# Component → binary path
overlay_bin="$TARGET/juhradial-overlay-rs"
settings_bin="$TARGET/juhradial-settings"
daemon_bin="$TARGET/juhradiald"

# Component → cargo crate name (-p flag)
overlay_crate="juhradial-overlay-rs"
settings_crate="juhradial-settings-rs"
daemon_crate="juhradiald"

bin_for() {
  case "$1" in
    overlay)  echo "$overlay_bin" ;;
    settings) echo "$settings_bin" ;;
    daemon)   echo "$daemon_bin" ;;
  esac
}
crate_for() {
  case "$1" in
    overlay)  echo "$overlay_crate" ;;
    settings) echo "$settings_crate" ;;
    daemon)   echo "$daemon_crate" ;;
  esac
}

components_for_arg() {
  case "${1:-overlay}" in
    all)                     echo "daemon overlay settings" ;;
    overlay|settings|daemon) echo "$1" ;;
    *) echo "Unknown component: $1 (want: overlay | settings | daemon | all)" >&2; exit 1 ;;
  esac
}

is_running() {
  local pidfile="$1"
  [[ -f "$pidfile" ]] || return 1
  local pid; pid="$(cat "$pidfile" 2>/dev/null || true)"
  [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null
}

cmd_build() {
  local comps; comps="$(components_for_arg "${1:-all}")"
  local crate_args=""
  for c in $comps; do crate_args="$crate_args -p $(crate_for "$c")"; done
  echo "==> cargo build --release $crate_args  (inside distrobox: $DISTROBOX)"
  distrobox enter "$DISTROBOX" -- bash -c "cd '$ROOT' && cargo build --release $crate_args"
}

ensure_daemon() {
  # Auto-start the daemon when overlay or settings is requested —
  # both are useless without it. The overlay's gesture-button
  # handling, the settings UI's battery / DPI / Easy-Switch
  # readouts, and the radial menu open path all live behind the
  # daemon's D-Bus surface. No-op when the daemon is already up.
  local daemon_pid="$RUNDIR/daemon.pid"
  if is_running "$daemon_pid"; then
    return 0
  fi
  echo "==> daemon not running — auto-starting (required by $1)"
  start_one daemon
}

start_one() {
  # Single-component start, factored out of cmd_start so
  # ensure_daemon can call it without recursion / extra arg parsing.
  local c="$1"
  local bin pidfile logfile
  bin="$(bin_for "$c")"
  pidfile="$RUNDIR/$c.pid"
  logfile="$RUNDIR/$c.log"
  if [[ ! -x "$bin" ]]; then
    echo "==> $c binary missing: $bin"
    echo "    Build it first:  $0 build $c"
    exit 1
  fi
  if is_running "$pidfile"; then
    echo "==> $c already running (PID $(cat "$pidfile"))"
    return 0
  fi
  # Daemon-specific group check: even when the user is in the
  # `input` group per `getent group`, a session started before
  # the usermod will have a stale group set. The daemon then
  # silently can't read /dev/input/event* and the radial menu
  # never activates. Detect + auto-reexec via `sg input` when
  # the group is missing from the current process.
  if [[ "$c" == "daemon" ]]; then
    if id -nG | tr ' ' '\n' | grep -qx input; then
      :
    elif getent group input | grep -q "[:,]${USER}\(,\|$\)"; then
      echo "==> shell session is missing the 'input' group (frozen at login)."
      echo "    Re-launching daemon under 'sg input' so it can read evdev."
      echo "    PID $$ groups: $(id -nG)"
      echo "    starting daemon → $logfile"
      setsid sg input -c "env RUST_LOG='$LOG_LEVEL' '$bin'" >"$logfile" 2>&1 &
      local pid=$!
      echo "$pid" >"$pidfile"
      sleep 0.8
      if is_running "$pidfile"; then
        echo "    PID $pid alive (input group acquired via sg)"
        return 0
      else
        echo "    !! failed to start under sg. Last 15 log lines:"
        tail -15 "$logfile" | sed 's/^/      /'
        rm -f "$pidfile"
        exit 1
      fi
    else
      echo "==> WARNING: user '$USER' is NOT in the 'input' group at all."
      echo "    Run: sudo usermod -aG input $USER  (then log out + back in)"
      echo "    The daemon will start anyway but won't see gesture buttons."
    fi
  fi
  echo "==> starting $c  →  $logfile"
  # nohup + setsid → child outlives the shell that started it
  setsid env RUST_LOG="$LOG_LEVEL" "$bin" >"$logfile" 2>&1 &
  local pid=$!
  echo "$pid" >"$pidfile"
  # Give it a moment to either crash or settle
  sleep 0.8
  if is_running "$pidfile"; then
    echo "    PID $pid alive"
  else
    echo "    !! failed to start. Last 15 log lines:"
    tail -15 "$logfile" | sed 's/^/      /'
    rm -f "$pidfile"
    exit 1
  fi
}

cmd_start() {
  local comps; comps="$(components_for_arg "${1:-overlay}")"
  for c in $comps; do
    # Bring the daemon up implicitly when overlay or settings is
    # being launched — the user's mental model is "start the UI",
    # not "remember to start the daemon first". `start all`
    # already iterates daemon first, so this is a no-op there.
    if [[ "$c" == "overlay" || "$c" == "settings" ]]; then
      ensure_daemon "$c"
    fi
    start_one "$c"
  done
}

cmd_stop() {
  local comps; comps="$(components_for_arg "${1:-all}")"
  for c in $comps; do
    local pidfile bin pids
    pidfile="$RUNDIR/$c.pid"
    bin="$(bin_for "$c")"
    if is_running "$pidfile"; then
      local pid; pid="$(cat "$pidfile")"
      echo "==> stopping $c (PID $pid)"
      kill "$pid" 2>/dev/null || true
      for _ in 1 2 3 4 5; do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.3
      done
      kill -9 "$pid" 2>/dev/null || true
      rm -f "$pidfile"
    else
      pids="$(pgrep -f "$bin" 2>/dev/null || true)"
      if [[ -n "$pids" ]]; then
        echo "==> killing stray $c processes: $pids"
        # shellcheck disable=SC2086
        kill $pids 2>/dev/null || true
        sleep 0.5
        # shellcheck disable=SC2086
        kill -9 $pids 2>/dev/null || true
      else
        echo "==> $c not running"
      fi
      rm -f "$pidfile"
    fi
  done
}

cmd_restart() {
  local target="${1:-overlay}"
  cmd_stop "$target"
  cmd_start "$target"
}

cmd_status() {
  printf "%-10s  %-8s  %s\n" "COMPONENT" "STATE" "PID"
  for c in daemon overlay settings; do
    local pidfile="$RUNDIR/$c.pid"
    if is_running "$pidfile"; then
      printf "%-10s  %-8s  %s\n" "$c" "running" "$(cat "$pidfile")"
    else
      printf "%-10s  %-8s  %s\n" "$c" "stopped" "-"
    fi
  done
  echo ""
  echo "Logs:    $RUNDIR/{daemon,overlay,settings}.log"
  echo "PIDs:    $RUNDIR/{daemon,overlay,settings}.pid"
}

cmd_logs() {
  local comp="${1:-overlay}"
  if [[ "$comp" != "overlay" && "$comp" != "settings" && "$comp" != "daemon" ]]; then
    echo "logs: component must be 'overlay', 'settings', or 'daemon'" >&2; exit 1
  fi
  local logfile="$RUNDIR/$comp.log"
  if [[ ! -f "$logfile" ]]; then
    echo "no log yet at $logfile (component never started)" >&2; exit 1
  fi
  if [[ "${2:-}" == "-f" ]]; then
    exec tail -f "$logfile"
  else
    tail -50 "$logfile"
  fi
}

cmd_help() {
  cat <<USAGE
juhradial-mx dev runner

  $0 build    [overlay|settings|daemon|all]   build inside distrobox=$DISTROBOX
  $0 start    [overlay|settings|daemon|all]   run on host  (default: overlay)
  $0 stop     [overlay|settings|daemon|all]   stop         (default: all)
  $0 restart  [overlay|settings|daemon|all]                  (default: overlay)
  $0 status                                   list all three components' state
  $0 logs     <overlay|settings|daemon> [-f]  tail recent log (-f to follow)

Files:
  PIDs   $RUNDIR/{daemon,overlay,settings}.pid
  Logs   $RUNDIR/{daemon,overlay,settings}.log

Env:
  JUHRADIAL_DISTROBOX=$DISTROBOX
  JUHRADIAL_LOG=$LOG_LEVEL

Daemon prerequisites:
  - User must be in 'input' group (sudo usermod -aG input \$USER + relogin)
  - udev rules at /etc/udev/rules.d/99-juhradialmx.rules (from install.sh)
USAGE
}

case "${1:-help}" in
  build)   cmd_build   "${2:-all}" ;;
  start)   cmd_start   "${2:-overlay}" ;;
  stop)    cmd_stop    "${2:-all}" ;;
  restart) cmd_restart "${2:-overlay}" ;;
  status)  cmd_status ;;
  logs)    cmd_logs    "${2:-overlay}" "${3:-}" ;;
  help|-h|--help) cmd_help ;;
  *) echo "Unknown command: $1" >&2; cmd_help; exit 1 ;;
esac
