# Live-test the indicator stack on Bazzite

Step-by-step recipe to bring up the indicator + popup + settings on a Wayland GNOME session, starting from a clean (no-install) state. Tailored to Bazzite (atomic Fedora, rpm-ostree).

Followups doc: [docs/plans/followups.md](plans/followups.md) — addresses what's wired vs deferred.

## Two paths

| Path | When to use | What runs |
|---|---|---|
| **A. `./install.sh` (one-shot)** | First-time install. Atomic-Fedora-aware. | **By default, layers NOTHING via rpm-ostree.** Probes the host for runtime libs (`libdbus`/`libudev`/`libsystemd`/`libevdev`/`libhidapi`) + commands (`ydotool`) via `ldconfig` + `command -v`; on Bazzite all of those are in the base image so the script proceeds straight to build+install. If something IS missing, it suggests Homebrew / Flatpak / distrobox FIRST. rpm-ostree layering is an explicit opt-in via `OXIDEMX_USE_RPM_OSTREE=1 ./install.sh`. |
| **B. Manual steps below** | Iterating during dev — you want explicit control over each step | Same end state. Lets you reuse your dev distrobox for cargo. |

Steps 1–7 below are path B. If you take path A, skip to step 5 (smoke-test) after `./install.sh` returns.

### Path A on Bazzite: what actually runs (no layering)

```
$ ./install.sh
   ...
   Image        Atomic (rpm-ostree — base image used directly, no layering by default)
   ...
[2] Installing dependencies
   → Atomic image detected — probing host for runtime libraries
   ✓ All runtime dependencies present — no layering needed
       Runtime libs: libdbus, libudev, libsystemd, libevdev, libhidapi — all in base image
       Runtime cmds: ydotool present
       Build: cargo on PATH (or distrobox-managed via dev.sh build all)
[3] Build → [4] Install files → [5] Enable service → [6] Desktop integration → done.
```

Two sudo prompts total: (a) `install -Dm755 ... /usr/local/bin/<bin>` for the four binaries, (b) `install -Dm644 ... /etc/udev/rules.d/99-oxidemx.rules`. `rpm-ostree status` stays untouched; `usroverlay` stays read-only.

---

## 0 · Prerequisites (one-time)

| Check | Command | Expected |
|---|---|---|
| User in `input` group | `id -nG \| tr ' ' '\n' \| grep -x input` | `input` printed |
| GNOME session | `echo "$XDG_CURRENT_DESKTOP"` | contains `GNOME` |
| distrobox `claude_development` running | `distrobox list \| grep claude_development` | `Up …` |
| `gnome-extensions` CLI | `command -v gnome-extensions` | path printed |
| `glib-compile-schemas` | `command -v glib-compile-schemas` | `/usr/bin/glib-compile-schemas` |
| `oxidemx-cursor` extension installed | `ls ~/.local/share/gnome-shell/extensions/oxidemx-cursor@dev.juhlabs.com` | dir exists |
| `busctl` | `command -v busctl` | path printed |

If `input` is missing: `sudo usermod -aG input $USER && sudo udevadm trigger`, log out + in.

If udev rules are missing: `sudo install -Dm644 packaging/udev/99-oxidemx.rules /etc/udev/rules.d/99-oxidemx.rules && sudo udevadm control --reload-rules && sudo udevadm trigger`. Daemon will work without them as long as you're in `input`, but the rules give hidraw access to non-root processes consistently.

---

## 1 · Build everything (5–8 min cold, 10 s warm)

All build steps go through `dev.sh` which re-execs inside the distrobox so the host stays clean:

```bash
cd ~/path/to/oxidemx
./dev.sh build all
```

That covers `oxidemxd`, `oxidemx-overlay`, `oxidemx-settings`, and any other Rust workspace members. Verify outputs:

```bash
ls -lh target/release/oxidemxd target/release/oxidemx-overlay \
       target/release/oxidemx-settings target/release/oxidemx-popup
```

The TS extensions compile separately via `dev-install-ext.sh` (see step 2).

---

## 2 · Install both GNOME extensions + GSettings schema

```bash
cd ~/path/to/oxidemx
./dev-install-ext.sh
```

What it does:
- `npm install` in `gnome-extension/` if needed (TypeScript toolchain bootstrap).
- `npx tsc` compiles `oxidemx-cursor` + `oxidemx-indicator` to `.js`.
- Copies both extensions to `~/.local/share/gnome-shell/extensions/`.
- Runs `glib-compile-schemas` on `oxidemx-indicator/schemas/` so prefs / popup can read it.
- `gnome-extensions disable; enable` on both for a hot reload (works for the cursor extension's D-Bus surface; the **indicator extension's `PanelMenu.Button` requires a full re-login on Wayland** — see step 4).

Verify the indicator's schema landed:

```bash
gsettings --schemadir ~/.local/share/gnome-shell/extensions/oxidemx-indicator@dev.juhlabs.com/schemas/ \
          list-keys org.gnome.shell.extensions.oxidemx-indicator
```

Expected: 15 keys printed (display-mode, threshold-critical, color-critical, …).

---

## 3 · Install the daemon binary + systemd user unit (one sudo call)

The systemd unit's `ExecStart` is hard-coded to `/usr/local/bin/oxidemxd`. For the indicator's supervisor to be able to `systemctl --user start oxidemx-daemon.service`, both pieces must be in their canonical locations:

```bash
cd ~/path/to/oxidemx
sudo install -Dm755 target/release/oxidemxd             /usr/local/bin/oxidemxd
sudo install -Dm755 target/release/oxidemx-popup        /usr/local/bin/oxidemx-popup
sudo install -Dm755 target/release/oxidemx-overlay   /usr/local/bin/oxidemx-overlay
sudo install -Dm755 target/release/oxidemx-settings     /usr/local/bin/oxidemx-settings
install  -Dm644 packaging/systemd/oxidemx-daemon.service \
                ~/.config/systemd/user/oxidemx-daemon.service
systemctl --user daemon-reload
systemctl --user enable --now oxidemx-daemon.service
```

Why `/usr/local/bin/` (not `/usr/bin/`): Bazzite's `/usr` is read-only; `/usr/local/` is the writable overlay. The systemd unit was already set up for this path.

Verify the daemon is up + claimed its D-Bus name:

```bash
systemctl --user status oxidemx-daemon.service --no-pager
busctl --user list | grep org.oxidemx.Daemon
busctl --user introspect org.oxidemx.Daemon /org/oxidemx/Daemon \
  | grep -E "GetActiveDeviceState|ShowPopup|EnsureOverlayRunning|DeviceStateChanged|SetHapticsEnabled"
```

Expected: service active; bus name listed; the 5 new methods/signal all printed.

---

## 4 · Re-login (Wayland requirement)

GNOME Shell on Wayland cannot dynamically load a new extension that creates a `PanelMenu.Button`. The cursor extension's pure-D-Bus surface hot-reloaded fine in step 2, but the new indicator needs a Shell restart to register its panel button.

```bash
# Either log out + log back in via the GNOME menu, OR (X11 only):
busctl --user --no-pager call org.gnome.Shell /org/gnome/Shell org.gnome.Shell Eval s 'Meta.restart("Restarting Shell…", global.context)'
```

On Wayland, only log-out-and-back-in works. There's no `r` shortcut.

After re-login, confirm the indicator is enabled:

```bash
gnome-extensions enable oxidemx-indicator@dev.juhlabs.com
gnome-extensions list --enabled | grep oxidemx
```

You should now see the mouse icon + battery percent in the top bar.

---

## 5 · Smoke-test the new features

Run the helper:

```bash
./scripts/smoke-indicator.sh
```

Or step through manually:

### 5.1 Battery + device surface

```bash
busctl --user call org.oxidemx.Daemon /org/oxidemx/Daemon \
       org.oxidemx.Daemon GetActiveDeviceState
# expect: (ybsss) battery percent, charging bool, connection, name, id
```

The indicator's top-bar text should match the battery percent reported here. Colour band depends on threshold-critical / threshold-low GSettings keys (defaults: 15% red, 30% yellow).

### 5.2 Indicator popup

Click the panel icon. The daemon should spawn `oxidemx-popup` and the cursor-helper extension should position it under the icon.

Direct test (skip the click):

```bash
# Grab the indicator's panel rect by clicking it — or just shoot a synthetic ShowPopup:
busctl --user call org.oxidemx.Daemon /org/oxidemx/Daemon \
       org.oxidemx.Daemon ShowPopup iiii 1800 32 32 32
# (x=1800, y=32, w=32, h=32 — somewhere in your top-right; tweak for your screen)
```

A 360×480 frameless popup should appear. ESC dismisses; clicking another window dismisses (focus-loss).

### 5.3 Stack supervisor — daemon-down remediation

```bash
systemctl --user stop oxidemx-daemon.service
```

Within `refresh-interval` seconds (default 30), the panel icon turns critical-coloured. Right-click → "Start daemon" should bring it back. Verify with:

```bash
systemctl --user status oxidemx-daemon.service --no-pager
```

### 5.4 Settings tab — Indicator Popup

```bash
oxidemx-settings
```

The sidebar should list "Indicator Popup" between "Point & Scroll" and "Haptic Feedback". Open it; verify:
- Mode (Simple / Power User) toggles
- Reorder up/down arrows enable/disable correctly at edges
- Toggling "Volume on scroll" survives a restart of `oxidemx-settings`

### 5.5 Haptic-feedback toggle (the one P2.2 quick-action we wired)

```bash
busctl --user call org.oxidemx.Daemon /org/oxidemx/Daemon \
       org.oxidemx.Daemon SetHapticsEnabled b true
# inspect that config.haptics.enabled changed
jq '.haptics.enabled' ~/.config/oxidemx/config.json
# … toggle off:
busctl --user call org.oxidemx.Daemon /org/oxidemx/Daemon \
       org.oxidemx.Daemon SetHapticsEnabled b false
jq '.haptics.enabled' ~/.config/oxidemx/config.json
```

Both writes should persist on disk via `Config::save()`.

### 5.6 Prefs dialog

```bash
gnome-extensions prefs oxidemx-indicator@dev.juhlabs.com
```

The 7-group libadwaita window should open: Preview → Display → Battery colors → Placement → Behavior → About → Reset. Drag a threshold spin-row; the preview pill should recolor live.

---

## 6 · Logs

Three places things can log:

```bash
# daemon (systemd-journald)
journalctl --user -u oxidemx-daemon.service -f

# indicator extension (GJS via Shell)
journalctl --user -u org.gnome.Shell -f | grep -F '[oxidemx-indicator]'

# popup-rs (foreground stdout when spawned manually; otherwise journald)
journalctl --user -t oxidemx-popup -f
```

If the indicator extension fails to enable:

```bash
gnome-extensions show oxidemx-indicator@dev.juhlabs.com | head -20
# look for the "errors:" line
```

---

## 7 · Tear-down

```bash
systemctl --user stop oxidemx-daemon.service
systemctl --user disable oxidemx-daemon.service
rm ~/.config/systemd/user/oxidemx-daemon.service
sudo rm /usr/local/bin/{oxidemxd,oxidemx-popup,oxidemx-overlay,oxidemx-settings}
gnome-extensions disable oxidemx-indicator@dev.juhlabs.com
gnome-extensions disable oxidemx-cursor@dev.juhlabs.com
rm -rf ~/.local/share/gnome-shell/extensions/oxidemx-{indicator,cursor}@dev.juhlabs.com
```

Re-login to fully remove the panel button.

---

## Known limitations during live test

Tracked in [docs/plans/followups.md](plans/followups.md):

- Quick-toggle for **Radial Overlay** / **Cursor Highlight** / **Flow** logs "not yet wired" on click (daemon doesn't expose those methods yet).
- Quick-slider for **Scroll sensitivity** / **Haptic intensity** / **Pointer accel** is the same — sliders render but don't apply.
- `panel-target='both'` falls back to top-bar-only with a label hint in prefs.
- `daemon/src/hidpp/tests.rs` test target compiles + passes (266/0/7) — confirmed during followups close-out.
