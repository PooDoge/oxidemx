# Pull Request: Atomic Fedora support + supervisor hardening

> **Working draft for upstream contribution to** [JuhLabs/juhradial-mx](https://github.com/JuhLabs/juhradial-mx).
> This file documents every change made on top of upstream `master` so the PR
> body can be assembled cleanly. Delete this file before opening the PR (or
> move its contents into the PR description).

## Summary

Adds first-class support for atomic/immutable Fedora distributions
(Bazzite, Silverblue, Kinoite, Bluefin, Aurora, Fedora Asahi Remix) and
fixes a long-standing supervisor bug that causes the radial menu to silently
stop responding to button presses after a USB suspend, mouse sleep/wake,
or Easy-Switch channel change.

The two issues are tracked together because the second only became
diagnosable while testing the first on Bazzite — but the supervisor fix is
distro-agnostic.

## Motivation

### 1. Installer fails on atomic Fedora

`install.sh` currently dispatches Bazzite/Silverblue/Kinoite/etc. into
`install_deps_fedora()`, which calls `sudo dnf install`. uBlue-based images
ship a `dnf` shim that aborts immediately:

```
ERROR: Fedora Atomic images utilize rpm-ostree instead (and is discouraged to use).
Please, read our documentation
https://docs.bazzite.gg/Installing_and_Managing_Software/
```

Even if `dnf` worked, `/usr/share` is read-only on these images, so
`install_files()`'s subsequent `sudo cp ... /usr/share/juhradial/` would
fail. The installer is unusable on what is one of the fastest-growing
Linux gaming/content distributions.

### 2. Split-brain supervisor causes silent breakage on reconnect

After a successful install, the menu works initially but stops responding
"after a while". Diagnosis on a real Bazzite GNOME/Wayland system showed:

- The `juhradialmx-daemon.service` (systemd user unit) starts at session
  bring-up. So does the `juhradial-mx` launcher (when the user opens it
  from the application menu, or autostarts it).
- The launcher does `pkill -f juhradiald` before spawning its own daemon.
  But on a fresh login the systemd service may not be active yet when
  the launcher runs, so pkill hits nothing — and a few seconds later
  systemd starts a *second* daemon. Now two daemons share the same
  Bolt-receiver hidraw fd.
- HID++ is a request/response protocol over a single shared report
  channel. With two clients, responses get interleaved and each daemon
  parses replies meant for the other. Logs from a real failure event:

  ```
  10:21:52.275 juhradiald[570851]:           reprog_controls=true
  10:21:53.562 juhradial-mx.desktop[568920]: reprog_controls=true
  10:21:53.825 juhradiald[570851]:           reprog_controls=false   ← divert flag corrupted
  10:21:55.360 juhradial-mx.desktop[568920]: reprog_controls=true
  ```

  The `reprog_controls=false` reply is a real HID++ response that one
  daemon parsed against the wrong outgoing query. The losing daemon
  concludes `REPROG_CONTROLS_V4 (0x1B04)` is unavailable and skips
  re-diverting. The thumb button reverts to firmware default behavior,
  no HID++ button reports hit hidraw, and the overlay goes deaf.
- Restarting the launcher fixes it because `pkill` takes out *both*
  daemons; only one starts back, and HID++ divert succeeds.
- The `Restart=on-abnormal` policy also exacerbates this: SIGTERM from
  `pkill` is "clean", so the systemd unit is left dead — making the
  resurrected single-daemon path the launcher's daemon, which is then
  vulnerable to whoever next runs `pkill`.

## Changes

### `install.sh` — Atomic Fedora support

#### Distro-family list (`resolve_distro_family`)

Added explicit IDs so atomic uBlue images are recognized as fedora
without depending on the `ID_LIKE` fallback (which is also there but
brittle for derivative spins):

```diff
-        fedora|rhel|centos|rocky|almalinux|nobara|ultramarine)
+        fedora|rhel|centos|rocky|almalinux|nobara|ultramarine|bazzite|silverblue|kinoite|bluefin|aurora|fedora-asahi-remix)
             DISTRO_FAMILY="fedora"
             ;;
```

#### Atomic detection (`check_atomic`)

New function that flags rpm-ostree-managed images and switches the
installation paths from `/usr/share/...` (read-only on atomic) to
`/usr/local/share/...` (writable, in `XDG_DATA_DIRS` by default):

```bash
check_atomic() {
    if [ -f /run/ostree-booted ]; then
        IS_ATOMIC=true
    elif command -v rpm-ostree &> /dev/null && rpm-ostree status &> /dev/null; then
        IS_ATOMIC=true
    fi

    if [ "$IS_ATOMIC" = true ]; then
        SHARE_DIR="/usr/local/share/juhradial"
        APP_DIR="/usr/local/share/applications"
        ICON_DIR="/usr/local/share/icons/hicolor/scalable/apps"
    fi
}
```

`SHARE_DIR`, `APP_DIR`, `ICON_DIR` are new top-level variables with
their non-atomic defaults set to the original `/usr/share` paths, so
non-atomic installs are byte-identical to the pre-change behavior.

#### Atomic dependency installation (`install_deps_fedora_atomic`)

Replaces the `sudo dnf install` path with `sudo rpm-ostree install
--idempotent` when `IS_ATOMIC=true`. Behavior:

- Filters the package list against `rpm -q` first, so subsequent runs
  (after the user reboots and re-runs the installer) short-circuit
  with "All required packages are already layered" and the script
  proceeds straight to build + install.
- Aligns with the [Bazzite docs](https://docs.bazzite.gg/Installing_and_Managing_Software/rpm-ostree/):
  warns that layering can delay future image upgrades, prompts for
  reboot, exits cleanly so the user re-runs after reboot.
- Does **not** try `--apply-live`. That flag is documented as
  experimental and Bazzite docs explicitly warn against relying on it.

#### Path-variable substitution in `install_files()`

Every previously-hardcoded `/usr/share/juhradial`, `/usr/share/applications`,
or `/usr/share/icons/...` is now `"$SHARE_DIR"`, `"$APP_DIR"`, `"$ICON_DIR"`.
Also replaces `cp -r overlay/flow /usr/share/juhradial/flow` with a
preceding `sudo rm -rf "$SHARE_DIR/flow"` because the unconditional `cp`
would create a nested `flow/flow/` directory on subsequent installs.

#### System-info panel

Added a single-line "Image: Atomic (rpm-ostree — layering required)"
indicator so users see at a glance which install path will be taken.

#### Overlay autostart entry

The current install ships only the systemd service for the daemon, with
no autostart for the Python overlay. The overlay is what subscribes to
the daemon's D-Bus signals and draws the menu, so without it the button
captures events with no visible effect. Today users have to manually
launch the `juhradial-mx` app every session.

`install.sh` now writes `~/.config/autostart/juhradial-overlay.desktop`
during `install_files()`:

```ini
[Desktop Entry]
Type=Application
Name=JuhRadial MX Overlay
Comment=Radial menu overlay for Logitech MX Master mice
Exec=python3 /usr/local/share/juhradial/juhradial-overlay.py
Icon=juhradial-mx
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
```

This is intentionally a per-user XDG autostart, not a second systemd
unit — the overlay is a Qt/PyQt6 GUI that needs the live graphical/D-Bus
session, not the user's headless `default.target`.

### `scripts/juhradial-mx.sh` — systemd-aware launcher

Rewritten so that **when the systemd user service is already managing the
daemon, the launcher does not pkill it and does not spawn a duplicate**.
It only (re)starts the overlay. When the service is inactive (e.g. user
disabled it, or runs from a `.git` source tree), behavior matches the
old script.

This is the second half of the split-brain fix: after the systemd unit
change below, even if a misbehaving caller invokes `juhradial-mx`,
HID++ protocol races no longer happen.

### `scripts/juhradial-settings.sh` — atomic-aware path lookup

Checks `/usr/local/share/juhradial/settings_dashboard.py` before
`/usr/share/juhradial/...`, matching the install layout on atomic.

### `packaging/systemd/juhradialmx-daemon.service` — supervisor hardening

```diff
-# Restart on crashes (SIGSEGV, SIGABRT, etc.) and watchdog kills.
-# on-abnormal = non-clean signals, watchdog, timeout — NOT SIGTERM/clean exit.
-Restart=on-abnormal
+# Restart on ANY exit reason (crashes, SIGTERM from rogue pkill, OOM, …).
+# Previously `on-abnormal` excluded clean signals, so a stray pkill would
+# permanently disable the unit. See PR for full root-cause analysis of
+# the split-brain D-Bus / HID++ races this prevents.
+Restart=always
 RestartSec=5s

-StartLimitBurst=5
+# Raised from 5 → 20 so a brief storm of HID++ reconnects (caused by
+# Bolt receiver hand-off during USB suspend/resume) doesn't permanently
+# disable the service.
+StartLimitBurst=20
```

This is the smallest change that closes the supervision gap. It should
be safe regardless of the launcher script change — the two changes are
defense in depth.

## Files changed

| File | Reason |
|---|---|
| `install.sh` | Atomic detection, rpm-ostree dependency install, path redirection, overlay autostart |
| `scripts/juhradial-mx.sh` | Systemd-aware launcher, no duplicate daemon |
| `scripts/juhradial-settings.sh` | Path lookup checks `/usr/local/share` first |
| `packaging/systemd/juhradialmx-daemon.service` | `Restart=always`, `StartLimitBurst=20` |

No source code (`daemon/`, `overlay/`, `gnome-extension/`) is touched.

## Validation

Verified end-to-end on **Bazzite GNOME NVIDIA-Open (`bazzite-gnome-nvidia-open:stable`,
Fedora 43 base)** with a Logitech MX Master 4 paired through a Logi Bolt receiver:

1. Fresh install on atomic system:
   - `install.sh` correctly detects Bazzite as `fedora`-family + `IS_ATOMIC=true`
   - Layers required RPMs via `rpm-ostree install --idempotent`
   - On second run after reboot, short-circuits the dependency step and
     proceeds to clone → build → install → enable
   - All files land under `/usr/local/share/juhradial/` (not `/usr/share`)
   - Desktop integration appears correctly in GNOME (icon in app grid)

2. Single-daemon ownership confirmed:
   ```
   org.kde.juhradialmx  →  PID 1860026 (juhradialmx-daemon.service)
   ```
   No second `juhradiald` process and no orphan D-Bus connections from
   stale launcher children.

3. HID++ divert succeeds without races on every restart:
   ```
   Diverting button cid="0x00C3"
   Button diverted successfully cid="0x00C3" response="[00, C3, 03, 00, 00]"
   Diverting button cid="0x01A0"
   Button diverted successfully cid="0x01A0" response="[01, A0, 03, 00, 00]"
   ```

4. Overlay autostart at login (no manual app launch required):
   - `~/.config/autostart/juhradial-overlay.desktop` triggers the overlay
     to come up under GNOME's autostart mechanism
   - Overlay binds to D-Bus and `MenuRequested` signals trigger the menu

5. Recovery from supervisor kill:
   - `kill -TERM <daemon-pid>` → systemd respawns within 5s with
     `NRestarts` incremented (proving `Restart=always` is in effect)
   - Re-divert is observed cleanly in the post-restart logs
   - Button works again immediately

## Backwards compatibility

- **Non-atomic Fedora**: detection short-circuits in `check_atomic()`
  (no `/run/ostree-booted`), `IS_ATOMIC` stays `false`, all paths are
  identical to upstream master. `install_deps_fedora()` is unchanged.
- **Arch/Debian/openSUSE**: untouched — no changes in their install paths.
- **Existing installations on traditional Fedora**: the systemd unit
  change (`Restart=always`) is the only behavioral difference. For
  users not using the launcher, this is a pure resilience improvement.
- **Users who depended on launcher always restarting daemon**: the new
  launcher detects the systemd service via `systemctl --user is-active
  --quiet juhradialmx-daemon`. If the service is inactive (e.g. user
  disabled it intentionally), the legacy "kill and respawn" behavior
  is preserved verbatim.

## Open questions / discussion points for review

1. **`X-GNOME-Autostart-enabled=true`**: this key is GNOME-specific but
   harmless on KDE/Hyprland/etc. We could drop it for purity. Worth a
   reviewer's call.
2. **Overlay autostart in `~/.config/autostart` vs `/etc/xdg/autostart`**:
   the former is per-user, the latter is system-wide. Per-user matches
   the existing pattern (systemd `--user` unit). System-wide would
   simplify multi-user setups but requires sudo on each install step.
3. **Should the launcher script's `pkill` fallback be removed entirely**?
   The systemd-aware path is strictly better. Keeping the legacy path
   is a courtesy to users running from a `.git` clone. Could be removed
   in a follow-up if reviewers prefer.

## Bazzite docs cross-references

- [Installing & Managing Software](https://docs.bazzite.gg/Installing_and_Managing_Software/)
- [Package Layering with rpm-ostree](https://docs.bazzite.gg/Installing_and_Managing_Software/rpm-ostree/)
