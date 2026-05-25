# Live-test the indicator stack on Bazzite

Step-by-step recipe to bring up the indicator + popup + settings on a Wayland GNOME session, starting from a clean (no-install) state. Tailored to Bazzite (atomic Fedora, rpm-ostree).

Followups doc: [docs/plans/followups.md](plans/followups.md) — addresses what's wired vs deferred.

## Two paths

| Path | When to use | What runs |
|---|---|---|
| **A. `./install.sh` (one-shot)** | First-time install, or you don't already have build tooling in distrobox | Atomic-Fedora-aware. Detects `/run/ostree-booted`; offers `rpm-ostree install --idempotent` for system deps (asks before, prompts for reboot after); installs binaries to `/usr/local/bin/`, shared data to `/usr/local/share/juhradial/`, GNOME extensions to `~/.local/share/gnome-shell/extensions/`. |
| **B. Manual steps below (this doc)** | Iterating during dev — you already have rust+cargo in distrobox and want explicit control over each step | Same end state as path A but you drive each install yourself. Lets you skip dependency layering and use the dev distrobox for cargo. |

Steps 1–7 below are path B. If you take path A, skip to step 5 (smoke-test) after `./install.sh` returns.

---

## 0 · Prerequisites (one-time)

| Check | Command | Expected |
|---|---|---|
| User in `input` group | `id -nG \| tr ' ' '\n' \| grep -x input` | `input` printed |
| GNOME session | `echo "$XDG_CURRENT_DESKTOP"` | contains `GNOME` |
| distrobox `claude_development` running | `distrobox list \| grep claude_development` | `Up …` |
| `gnome-extensions` CLI | `command -v gnome-extensions` | path printed |
| `glib-compile-schemas` | `command -v glib-compile-schemas` | `/usr/bin/glib-compile-schemas` |
| `juhradial-cursor` extension installed | `ls ~/.local/share/gnome-shell/extensions/juhradial-cursor@dev.juhlabs.com` | dir exists |
| `busctl` | `command -v busctl` | path printed |

If `input` is missing: `sudo usermod -aG input $USER && sudo udevadm trigger`, log out + in.

If udev rules are missing: `sudo install -Dm644 packaging/udev/99-juhradialmx.rules /etc/udev/rules.d/99-juhradialmx.rules && sudo udevadm control --reload-rules && sudo udevadm trigger`. Daemon will work without them as long as you're in `input`, but the rules give hidraw access to non-root processes consistently.

---

## 1 · Build everything (5–8 min cold, 10 s warm)

All build steps go through `dev.sh` which re-execs inside the distrobox so the host stays clean:

```bash
cd ~/path/to/juhradial-mx
./dev.sh build all
```

That covers `juhradiald`, `juhradial-overlay-rs`, `juhradial-settings`, and any other Rust workspace members. Verify outputs:

```bash
ls -lh target/release/juhradiald target/release/juhradial-overlay-rs \
       target/release/juhradial-settings target/release/juhradial-popup
```

The TS extensions compile separately via `dev-install-ext.sh` (see step 2).

---

## 2 · Install both GNOME extensions + GSettings schema

```bash
cd ~/path/to/juhradial-mx
./dev-install-ext.sh
```

What it does:
- `npm install` in `gnome-extension/` if needed (TypeScript toolchain bootstrap).
- `npx tsc` compiles `juhradial-cursor` + `juhradial-indicator` to `.js`.
- Copies both extensions to `~/.local/share/gnome-shell/extensions/`.
- Runs `glib-compile-schemas` on `juhradial-indicator/schemas/` so prefs / popup can read it.
- `gnome-extensions disable; enable` on both for a hot reload (works for the cursor extension's D-Bus surface; the **indicator extension's `PanelMenu.Button` requires a full re-login on Wayland** — see step 4).

Verify the indicator's schema landed:

```bash
gsettings --schemadir ~/.local/share/gnome-shell/extensions/juhradial-indicator@dev.juhlabs.com/schemas/ \
          list-keys org.gnome.shell.extensions.juhradial-indicator
```

Expected: 15 keys printed (display-mode, threshold-critical, color-critical, …).

---

## 3 · Install the daemon binary + systemd user unit (one sudo call)

The systemd unit's `ExecStart` is hard-coded to `/usr/local/bin/juhradiald`. For the indicator's supervisor to be able to `systemctl --user start juhradialmx-daemon.service`, both pieces must be in their canonical locations:

```bash
cd ~/path/to/juhradial-mx
sudo install -Dm755 target/release/juhradiald             /usr/local/bin/juhradiald
sudo install -Dm755 target/release/juhradial-popup        /usr/local/bin/juhradial-popup
sudo install -Dm755 target/release/juhradial-overlay-rs   /usr/local/bin/juhradial-overlay-rs
sudo install -Dm755 target/release/juhradial-settings     /usr/local/bin/juhradial-settings
install  -Dm644 packaging/systemd/juhradialmx-daemon.service \
                ~/.config/systemd/user/juhradialmx-daemon.service
systemctl --user daemon-reload
systemctl --user enable --now juhradialmx-daemon.service
```

Why `/usr/local/bin/` (not `/usr/bin/`): Bazzite's `/usr` is read-only; `/usr/local/` is the writable overlay. The systemd unit was already set up for this path.

Verify the daemon is up + claimed its D-Bus name:

```bash
systemctl --user status juhradialmx-daemon.service --no-pager
busctl --user list | grep org.juhradial.Daemon
busctl --user introspect org.juhradial.Daemon /org/juhradial/Daemon \
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
gnome-extensions enable juhradial-indicator@dev.juhlabs.com
gnome-extensions list --enabled | grep juhradial
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
busctl --user call org.juhradial.Daemon /org/juhradial/Daemon \
       org.juhradial.Daemon GetActiveDeviceState
# expect: (ybsss) battery percent, charging bool, connection, name, id
```

The indicator's top-bar text should match the battery percent reported here. Colour band depends on threshold-critical / threshold-low GSettings keys (defaults: 15% red, 30% yellow).

### 5.2 Indicator popup

Click the panel icon. The daemon should spawn `juhradial-popup` and the cursor-helper extension should position it under the icon.

Direct test (skip the click):

```bash
# Grab the indicator's panel rect by clicking it — or just shoot a synthetic ShowPopup:
busctl --user call org.juhradial.Daemon /org/juhradial/Daemon \
       org.juhradial.Daemon ShowPopup iiii 1800 32 32 32
# (x=1800, y=32, w=32, h=32 — somewhere in your top-right; tweak for your screen)
```

A 360×480 frameless popup should appear. ESC dismisses; clicking another window dismisses (focus-loss).

### 5.3 Stack supervisor — daemon-down remediation

```bash
systemctl --user stop juhradialmx-daemon.service
```

Within `refresh-interval` seconds (default 30), the panel icon turns critical-coloured. Right-click → "Start daemon" should bring it back. Verify with:

```bash
systemctl --user status juhradialmx-daemon.service --no-pager
```

### 5.4 Settings tab — Indicator Popup

```bash
juhradial-settings
```

The sidebar should list "Indicator Popup" between "Point & Scroll" and "Haptic Feedback". Open it; verify:
- Mode (Simple / Power User) toggles
- Reorder up/down arrows enable/disable correctly at edges
- Toggling "Volume on scroll" survives a restart of `juhradial-settings`

### 5.5 Haptic-feedback toggle (the one P2.2 quick-action we wired)

```bash
busctl --user call org.juhradial.Daemon /org/juhradial/Daemon \
       org.juhradial.Daemon SetHapticsEnabled b true
# inspect that config.haptics.enabled changed
jq '.haptics.enabled' ~/.config/juhradial/config.json
# … toggle off:
busctl --user call org.juhradial.Daemon /org/juhradial/Daemon \
       org.juhradial.Daemon SetHapticsEnabled b false
jq '.haptics.enabled' ~/.config/juhradial/config.json
```

Both writes should persist on disk via `Config::save()`.

### 5.6 Prefs dialog

```bash
gnome-extensions prefs juhradial-indicator@dev.juhlabs.com
```

The 7-group libadwaita window should open: Preview → Display → Battery colors → Placement → Behavior → About → Reset. Drag a threshold spin-row; the preview pill should recolor live.

---

## 6 · Logs

Three places things can log:

```bash
# daemon (systemd-journald)
journalctl --user -u juhradialmx-daemon.service -f

# indicator extension (GJS via Shell)
journalctl --user -u org.gnome.Shell -f | grep -F '[juhradial-indicator]'

# popup-rs (foreground stdout when spawned manually; otherwise journald)
journalctl --user -t juhradial-popup -f
```

If the indicator extension fails to enable:

```bash
gnome-extensions show juhradial-indicator@dev.juhlabs.com | head -20
# look for the "errors:" line
```

---

## 7 · Tear-down

```bash
systemctl --user stop juhradialmx-daemon.service
systemctl --user disable juhradialmx-daemon.service
rm ~/.config/systemd/user/juhradialmx-daemon.service
sudo rm /usr/local/bin/{juhradiald,juhradial-popup,juhradial-overlay-rs,juhradial-settings}
gnome-extensions disable juhradial-indicator@dev.juhlabs.com
gnome-extensions disable juhradial-cursor@dev.juhlabs.com
rm -rf ~/.local/share/gnome-shell/extensions/juhradial-{indicator,cursor}@dev.juhlabs.com
```

Re-login to fully remove the panel button.

---

## Known limitations during live test

Tracked in [docs/plans/followups.md](plans/followups.md):

- Quick-toggle for **Radial Overlay** / **Cursor Highlight** / **Flow** logs "not yet wired" on click (daemon doesn't expose those methods yet).
- Quick-slider for **Scroll sensitivity** / **Haptic intensity** / **Pointer accel** is the same — sliders render but don't apply.
- `panel-target='both'` falls back to top-bar-only with a label hint in prefs.
- `daemon/src/hidpp/tests.rs` test target compiles + passes (266/0/7) — confirmed during followups close-out.
