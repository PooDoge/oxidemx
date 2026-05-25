# JuhRadial Indicator + Popup + Settings — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a GNOME Shell indicator that shows MX-device battery in the top bar, opens a daemon-rendered Iced popup on click, and is configured via a new "Indicator Popup" tab in `settings-rs`.

**Architecture:** Four cooperating components, two persistence layers, three new workspace crates.

1. **`gnome-extension/juhradial-indicator@dev.juhlabs.com/`** — new TypeScript extension, draws the panel button, polls `org.juhradial.Daemon.GetActiveDeviceState`, dispatches click → `org.juhradial.Daemon.ShowPopup()` (new D-Bus method we add).
2. **`gnome-extension/juhradial-indicator@dev.juhlabs.com/prefs.ts`** — libadwaita single-page prefs.
3. **`settings-rs/src/tabs/indicator_popup.rs`** — new tab in the existing settings app, slotted between `PointScroll` and `Haptic` in the `Tab` enum.
4. **`popup-rs/`** — NEW sibling workspace member; tiny Iced binary `juhradial-popup` that the daemon spawns when `ShowPopup` is called. (Daemon stays UI-free; mirrors the overlay-rs split.)
5. **`juhradial-widgets/`** — NEW sibling workspace crate that owns the reusable Iced widgets, style closures, and palette (extracted from `settings-rs/src/{widgets,style,palette}.rs`). Imported by `settings-rs`, `overlay-rs` (where applicable), and `popup-rs`.
6. **`juhradial-window/`** — NEW sibling workspace crate that owns the iced window-shell helper (the 6-line `iced::window::Settings` for a frameless, transparent, always-on-top xdg-shell window) plus the `CursorHelper.MoveOverlay` D-Bus client extracted from `overlay-rs/src/ext_positioner.rs`. Imported by `overlay-rs` and `popup-rs`.

Two persistence layers:
- **Extension prefs** → `gsettings` under `org.gnome.shell.extensions.juhradial-indicator.*`. Daemon reads via `gio` (gtk-rs) so popup battery-ring color matches the panel icon.
- **Popup prefs** (mode, quick toggles/sliders, host labels, volume-on-scroll) → new `popup: PopupConfig` field on `juhradial_shared::AppConfig`, lands in `~/.config/juhradial/config.json` alongside everything else.

**Tech Stack:**
- Rust 2021 + Iced 0.14 (matches `settings-rs/`, `overlay-rs/`)
- zbus 5 (matches everything else in the workspace)
- GJS / TypeScript (`@girs/gjs`, `@girs/gnome-shell`) + libadwaita
- `gio` (gtk-rs) — already a transitive dep via system D-Bus, added directly for GSettings

---

## CORRECTIONS TO THE ORIGINAL PROMPT (read before planning)

The repo state diverged from the prompt in several load-bearing ways. The 2026-05-24 clarification round resolved the eight blocking ambiguities. The table records the prompt's assumption, the actual repo state, and the **confirmed decision** going forward.

| Prompt assumed | Repo state | Confirmed decision |
|---|---|---|
| D-Bus name `org.juhradial.Daemon` | Daemon currently claims `org.kde.juhradialmx` at `/org/kde/juhradialmx/Daemon`, interface `org.kde.juhradialmx.Daemon`. Constants live in `daemon/src/dbus/mod.rs`. | **Rename to `org.juhradial.Daemon` in Phase 0.** Flag-day across `daemon/` (5 files), `overlay-rs/` (4 files), `settings-rs/src/daemon.rs`, `packaging/org.kde.juhradialmx.settings.desktop` (rename file), `install.sh`, `local-test-install.sh`, `dev-test.sh`, `CONTRIBUTING-BAZZITE-PR.md`, and the docstring/comment trail. Path becomes `/org/juhradial/Daemon`. |
| `~/.config/juhradial/config.toml` (serde TOML) | `~/.config/juhradial/config.json` (serde_json). `juhradial_shared::config::AppConfig` is the root struct. | **Keep JSON.** Add `popup: PopupConfig` field on `AppConfig`. Reuses the overlay's existing inotify watcher and `settings-rs/src/persist.rs` debounce. |
| Popup is "owned by the daemon (in-process Iced window)" | Daemon binary `juhradiald` has zero UI deps. | **Separate `juhradial-popup` binary.** New workspace member `popup-rs/`. Daemon's `ShowPopup` D-Bus handler spawns it via `tokio::process::Command`. Mirrors the overlay split. |
| Extract `widgets/` into `juhradial-widgets` crate | `settings-rs/src/widgets.rs` (~120 LOC) + `style.rs` + `palette.rs` are inline modules. | **Extract.** New workspace crate `juhradial-widgets/` owns `widgets.rs` + `style.rs` + `palette.rs`. `settings-rs` keeps a thin façade re-exporting through `pub use` for its own tab files. `popup-rs` imports directly. Done in Phase A. |
| Extract `overlay-rs/src/window.rs` → `juhradial-window` | No `window.rs` exists; the 6-line iced window setup is inlined in `overlay-rs/src/app.rs::run()`; the `MoveOverlay` D-Bus client is in `overlay-rs/src/ext_positioner.rs`. | **Extract.** New workspace crate `juhradial-window/` owns: (a) a `frameless_topmost(app_id, size)` helper returning a configured `iced::window::Settings`, and (b) the `CursorHelper` zbus proxy from `ext_positioner.rs`. `overlay-rs` and `popup-rs` both import it. Done in Phase A. |
| Wayland layer-shell for the popup | Mutter does not advertise wlr-layer-shell on stable GNOME. | **No layer-shell.** Popup is a regular xdg-shell window positioned by the cursor extension's `MoveOverlay`, same as the overlay. |
| Settings app uses `nav.ts` / `pages/X.tsx` | Settings app is Rust. Nav is the `Tab` enum in `settings-rs/src/main.rs`. | **New tab file:** `settings-rs/src/tabs/indicator_popup.rs` (snake_case, like every other tab). `Tab::IndicatorPopup` slotted between `PointScroll` and `Haptic`. |
| GNOME extension is TypeScript | Existing `juhradial-cursor@dev.juhlabs.com` is plain ES-modules JavaScript. | **Migrate both extensions to TypeScript in Phase 1.** Single shared `tsconfig.json` and `@girs/*` typing dep tree under `gnome-extension/`. The existing cursor extension converts at the same time as the new indicator is added. |
| Daemon already exposes `GetActiveDeviceState() -> {battery, charging, connection, deviceName, deviceId}` and `DeviceStateChanged` signal | Daemon exposes only `get_battery_status() -> (u8 percent, bool charging)`. No connection / name / id; no change signal. | **Add in Phase 0.** New zbus method `get_active_device_state() -> (u8, bool, String, String, String)` and signal `DeviceStateChanged(yysss)` emitted from `daemon/src/battery.rs` after each poll when any field changed. |
| Critical-battery: no notification spec | n/a | **Emit one-shot `Gio.Notification` (urgency=critical)** when the indicator first observes battery crossing into critical. Re-armable: clears once battery goes back above critical or device charges. Title: "MX device low" · body: "{deviceName} at {pct}% — connect charging cable". Implemented in extension (`lib/battery.ts`), not the daemon. |
| Quick-toggle ids `highlight`, `flow` map to daemon methods | Daemon doesn't expose them yet. | **Render anyway, log "not yet wired" on click.** Popup shows a transient inline notice. Full wiring tracked as a Phase 3.5 follow-up. Settings catalog stays complete per the design. |
| Coordinate space for `ShowPopup(panel_rect)` | n/a | **Stage-absolute logical pixels.** Indicator passes `actor.get_transformed_extents()` directly. Popup forwards to `MoveOverlay` with `monitor=-1`; the cursor extension resolves the monitor. |

---

## PHASE ORDER

| Phase | PR | Contents | Depends on |
|---|---|---|---|
| 0 | #1 | Shared types (`PopupConfig`) + D-Bus rename + new D-Bus methods/signal + GSettings schema | — |
| A | #2 | Extract `juhradial-widgets` and `juhradial-window` crates; update `settings-rs`, `overlay-rs` to consume them | Phase 0 |
| 1 | #3 | GNOME extensions — new indicator (TS) + migrate cursor to TS; libadwaita prefs; symbolic icons | Phase 0 |
| 2 | #4 | Settings tab `IndicatorPopup` | Phase 0, Phase A |
| 3 | #5 | `popup-rs` binary | Phase 0, Phase A, Phase 1 (uses CursorHelper proxy) |

Each phase ships an independently mergeable PR.

---

## TASK BREAKDOWN

### Phase 0 — D-Bus rename + shared types + new wire surface

**Why first:** every other phase consumes types from `juhradial-shared` and methods from the daemon's D-Bus interface. The rename is bundled here so the entire workspace lands on the new name in one atomic change rather than dribbling rename commits across every later PR.

#### Task 0.0 — Rename `org.kde.juhradialmx*` → `org.juhradial*` across the workspace

**Files (every one needs editing):**
- Modify: `daemon/src/dbus/mod.rs` (constants `DBUS_INTERFACE`, `DBUS_PATH`, `DBUS_NAME`; the two assertion tests)
- Modify: `daemon/src/dbus/interface.rs:278` (`#[interface(name = …)]`)
- Modify: `daemon/src/hidraw.rs:590-591`, `daemon/src/evdev.rs:839-840`, `daemon/src/cursor.rs:203-204` (KWin-script `callDBus` triplets)
- Modify: `daemon/src/main.rs:1158, 1179, 1203` (interface-name string literals)
- Modify: `overlay-rs/src/dbus.rs` (`DAEMON_PATH`, `#[proxy]` annotations, log string)
- Modify: `overlay-rs/src/haptic_client.rs` (`DAEMON_SERVICE`, `DAEMON_PATH`, `#[proxy]` annotations)
- Modify: `overlay-rs/src/main.rs:4`, `overlay-rs/Cargo.toml:30` (doc comments)
- Modify: `overlay-rs/src/app.rs:27` (`APP_ID = "org.juhradial.overlay"`)
- Modify: `overlay-rs/src/ext_positioner.rs:114` (doc comment)
- Modify: `settings-rs/src/daemon.rs:12-18` (`DAEMON_BUS`, `DAEMON_PATH`, `#[proxy]`)
- Rename: `packaging/org.kde.juhradialmx.settings.desktop` → `packaging/org.juhradial.settings.desktop`
- Modify: `install.sh:525`, `local-test-install.sh:68-69`, `dev-test.sh:132-138` (path + `busctl` lookups)
- Modify: `CONTRIBUTING-BAZZITE-PR.md:240`, `RUST_GTK4_OVERLAY_DESIGN.md:158` (docstring trail)

**Naming policy (locked):**
- D-Bus name: `org.juhradial.Daemon`
- D-Bus path: `/org/juhradial/Daemon`
- Interface: `org.juhradial.Daemon`
- Overlay app_id: `org.juhradial.overlay`
- Popup app_id: `org.juhradial.popup`
- Desktop file: `org.juhradial.settings.desktop`
- Cursor-helper extension service stays `org.juhradial.CursorHelper` (already correct namespace)

- [ ] **Step 1: Update daemon-side constants + interface attribute**

Edit `daemon/src/dbus/mod.rs`:
```rust
pub const DBUS_INTERFACE: &str = "org.juhradial.Daemon";
pub const DBUS_PATH:      &str = "/org/juhradial/Daemon";
pub const DBUS_NAME:      &str = "org.juhradial.Daemon";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn naming_locked() {
        assert_eq!(DBUS_INTERFACE, "org.juhradial.Daemon");
        assert_eq!(DBUS_PATH,      "/org/juhradial/Daemon");
        assert_eq!(DBUS_NAME,      "org.juhradial.Daemon");
    }
}
```

Edit `daemon/src/dbus/interface.rs:278`:
```rust
#[interface(name = "org.juhradial.Daemon")]
impl JuhRadialService { … }
```

- [ ] **Step 2: Update all daemon string-literal D-Bus calls (KWin-script callDBus paths)**

Replace `org.kde.juhradialmx` → `org.juhradial.Daemon` and `/org/kde/juhradialmx/Daemon` → `/org/juhradial/Daemon` in:
- `daemon/src/hidraw.rs:590-591`
- `daemon/src/evdev.rs:839-840`
- `daemon/src/cursor.rs:203-204`
- `daemon/src/main.rs:1158, 1179, 1203`

The KWin script bodies these literals get pushed into still reference the same on-the-wire name, so the rename is the *only* change needed — no protocol restructuring.

- [ ] **Step 3: Update overlay + settings consumers**

Edit:
- `overlay-rs/src/dbus.rs:20-32`: `DAEMON_PATH = "/org/juhradial/Daemon"`, `default_service = "org.juhradial.Daemon"`, `default_path = "/org/juhradial/Daemon"`, interface attr `"org.juhradial.Daemon"`.
- `overlay-rs/src/haptic_client.rs:18-24`: same triplet.
- `overlay-rs/src/app.rs:27`: `APP_ID = "org.juhradial.overlay"`.
- `settings-rs/src/daemon.rs:12-18`: `DAEMON_BUS = "org.juhradial.Daemon"`, `DAEMON_PATH = "/org/juhradial/Daemon"`, interface attr `"org.juhradial.Daemon"`.

- [ ] **Step 4: Rename packaging desktop file + update installers**

```bash
git mv packaging/org.kde.juhradialmx.settings.desktop packaging/org.juhradial.settings.desktop
sed -i 's|org\.kde\.juhradialmx|org.juhradial|g' packaging/org.juhradial.settings.desktop install.sh local-test-install.sh dev-test.sh
```

Verify the `Icon=` / `Exec=` fields inside the desktop file are still valid (they reference binary names, not D-Bus names — should be unaffected).

- [ ] **Step 5: Sweep for stragglers**

```bash
rg "org\.kde\.juhradialmx|kde/juhradialmx" -g '!target/' -g '!node_modules/' -g '!design/'
```

Expected: zero hits except in `CHANGELOG.md` (historical) and any contributing-doc blockquotes that quote the old name as historical context. Update doc trail (`CONTRIBUTING-BAZZITE-PR.md`, `RUST_GTK4_OVERLAY_DESIGN.md`) in the same commit.

- [ ] **Step 6: Build + test + commit**

```bash
cargo build --workspace
cargo test --workspace
```

Expected: clean build, all existing tests pass (the rename is a string substitution; behavior is unchanged).

```bash
git add -A   # explicitly preferred over `git add .` to capture the desktop-file rename
git commit -m "rename(dbus): org.kde.juhradialmx -> org.juhradial.Daemon

Flag-day rename across daemon, overlay, settings, packaging, and
install/test scripts. No behaviour change. Stops the indicator
work shipping with a legacy KDE-era name on the wire."
```

#### Task 0.1 — Add `PopupConfig` to `juhradial-shared`

**Files:**
- Create: `juhradial-shared/src/popup.rs`
- Modify: `juhradial-shared/src/lib.rs` (add `mod popup; pub use popup::*;`)
- Modify: `juhradial-shared/src/config.rs` (add `#[serde(default)] pub popup: PopupConfig,` field on `AppConfig`)
- Test: `juhradial-shared/src/popup.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing serde round-trip test**

```rust
// juhradial-shared/src/popup.rs (bottom)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips() {
        let p = PopupConfig::default();
        let s = serde_json::to_string(&p).unwrap();
        let r: PopupConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(p, r);
    }

    #[test]
    fn missing_fields_deserialize_to_defaults() {
        // Older config.json with no `popup` block at all.
        let s = "{}";
        let p: PopupConfig = serde_json::from_str(s).unwrap();
        assert_eq!(p, PopupConfig::default());
    }

    #[test]
    fn simple_mode_defaults_match_design() {
        let p = PopupConfig::default();
        assert_eq!(p.mode, PopupMode::Simple);
        assert_eq!(p.simple_toggles, vec!["gaming", "haptics", "radial"]);
    }

    #[test]
    fn power_sliders_only_in_power_mode_by_design() {
        // Catalog membership: every default id exists in the catalog.
        for id in PopupConfig::default().simple_toggles {
            assert!(QUICK_TOGGLE_CATALOG.iter().any(|q| q.id == id),
                "default simple toggle {id:?} missing from catalog");
        }
        for id in PopupConfig::default().power_toggles {
            assert!(QUICK_TOGGLE_CATALOG.iter().any(|q| q.id == id));
        }
        for id in PopupConfig::default().power_sliders {
            assert!(QUICK_SLIDER_CATALOG.iter().any(|q| q.id == id));
        }
    }
}
```

- [ ] **Step 2: Run tests — expect failure**

Run: `cargo test -p juhradial-shared popup`
Expected: FAIL with "cannot find type `PopupConfig`".

- [ ] **Step 3: Implement `PopupConfig`**

```rust
// juhradial-shared/src/popup.rs (top)
//! Indicator-popup preferences. Persisted as the `popup` table in
//! the same `~/.config/juhradial/config.json` everything else lives
//! in (one file = one inotify event = one reload).
//!
//! Read by:
//!   * settings-rs (Indicator Popup tab — edits + saves)
//!   * popup-rs    (displays the popup using these knobs)
//! NOT read by the GNOME extension. The extension's prefs are
//! visual/icon-only and live in GSettings (separate concern; see
//! org.gnome.shell.extensions.juhradial-indicator).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PopupMode {
    Simple,
    Power,
}

impl Default for PopupMode {
    fn default() -> Self { PopupMode::Simple }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostLabelStyle {
    /// Show paired hostname (falls back to "Channel N" when unknown).
    Hostname,
    /// Always show numeric channel ("Channel 1", "Channel 2", …).
    Channel,
}

impl Default for HostLabelStyle {
    fn default() -> Self { HostLabelStyle::Hostname }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PopupConfig {
    #[serde(default)] pub mode: PopupMode,
    #[serde(default = "default_show_host_buttons")] pub show_host_buttons: bool,
    #[serde(default)] pub host_label_style: HostLabelStyle,
    #[serde(default = "default_simple_toggles")] pub simple_toggles: Vec<String>,
    #[serde(default = "default_power_toggles")] pub power_toggles: Vec<String>,
    #[serde(default = "default_power_sliders")] pub power_sliders: Vec<String>,
    #[serde(default = "default_true")] pub volume_on_scroll: bool,
    #[serde(default)] pub close_on_action: bool,
    #[serde(default = "default_true")] pub animations: bool,
}

fn default_true() -> bool { true }
fn default_show_host_buttons() -> bool { true }
fn default_simple_toggles() -> Vec<String> { vec!["gaming".into(), "haptics".into(), "radial".into()] }
fn default_power_toggles() -> Vec<String> { vec!["gaming".into(), "haptics".into(), "radial".into(), "flow".into()] }
fn default_power_sliders() -> Vec<String> { vec!["dpi".into(), "scroll".into()] }

impl Default for PopupConfig {
    fn default() -> Self {
        Self {
            mode: PopupMode::default(),
            show_host_buttons: default_show_host_buttons(),
            host_label_style: HostLabelStyle::default(),
            simple_toggles: default_simple_toggles(),
            power_toggles: default_power_toggles(),
            power_sliders: default_power_sliders(),
            volume_on_scroll: true,
            close_on_action: false,
            animations: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct QuickEntry { pub id: &'static str, pub label: &'static str, pub icon: &'static str, pub desc: &'static str }

pub const QUICK_TOGGLE_CATALOG: &[QuickEntry] = &[
    QuickEntry { id: "gaming",    label: "Gaming Mode",     icon: "applications-games-symbolic", desc: "Hides the radial, bumps DPI" },
    QuickEntry { id: "haptics",   label: "Haptic Feedback", icon: "audio-volume-high-symbolic",  desc: "Click-and-hold ticks" },
    QuickEntry { id: "radial",    label: "Radial Overlay",  icon: "applications-graphics-symbolic", desc: "Toggle the radial menu" },
    QuickEntry { id: "flow",      label: "Flow",            icon: "view-grid-symbolic",          desc: "Cross-device scroll & paste" },
    QuickEntry { id: "smart",     label: "SmartShift",      icon: "system-switch-user-symbolic", desc: "Free-spin scroll wheel" },
    QuickEntry { id: "highlight", label: "Cursor highlight", icon: "preferences-desktop-cursors-symbolic", desc: "Pulse ring on shake" },
];

pub const QUICK_SLIDER_CATALOG: &[QuickEntry] = &[
    QuickEntry { id: "dpi",      label: "Pointer DPI",          icon: "preferences-desktop-cursors-symbolic", desc: "200 – 6,400 dpi" },
    QuickEntry { id: "scroll",   label: "Scroll sensitivity",   icon: "system-switch-user-symbolic",          desc: "1 – 10" },
    QuickEntry { id: "haptic_i", label: "Haptic intensity",     icon: "audio-volume-high-symbolic",           desc: "Off – Strong" },
    QuickEntry { id: "accel",    label: "Pointer acceleration", icon: "view-grid-symbolic",                   desc: "-1.0 – 1.0" },
];
```

```rust
// juhradial-shared/src/lib.rs (add)
mod popup;
pub use popup::*;
```

```rust
// juhradial-shared/src/config.rs (modify AppConfig)
// Add field:
#[serde(default)]
pub popup: crate::popup::PopupConfig,
```

- [ ] **Step 4: Run tests — expect pass**

Run: `cargo test -p juhradial-shared`
Expected: PASS, all popup tests green.

- [ ] **Step 5: Add reorder helper + unit tests**

```rust
// juhradial-shared/src/popup.rs (bottom, before #[cfg(test)])
impl PopupConfig {
    /// Move `id` up in whichever vec contains it. No-op if not present
    /// or already first.
    pub fn move_up(list: &mut Vec<String>, id: &str) {
        if let Some(i) = list.iter().position(|x| x == id) {
            if i > 0 { list.swap(i, i - 1); }
        }
    }
    pub fn move_down(list: &mut Vec<String>, id: &str) {
        if let Some(i) = list.iter().position(|x| x == id) {
            if i + 1 < list.len() { list.swap(i, i + 1); }
        }
    }
}

#[cfg(test)]
mod reorder_tests {
    use super::*;
    #[test] fn move_up_swaps() { let mut v: Vec<String> = vec!["a".into(),"b".into()]; PopupConfig::move_up(&mut v, "b"); assert_eq!(v, vec!["a","b"].into_iter().map(String::from).collect::<Vec<_>>().reverse_clone()); }
    #[test] fn move_up_first_is_noop() { let mut v = vec!["a".into(),"b".into()]; PopupConfig::move_up(&mut v, "a"); assert_eq!(v[0], "a"); }
    #[test] fn move_down_last_is_noop() { let mut v: Vec<String> = vec!["a".into(),"b".into()]; PopupConfig::move_down(&mut v, "b"); assert_eq!(v[1], "b"); }
    #[test] fn move_unknown_is_noop() { let mut v: Vec<String> = vec!["a".into()]; PopupConfig::move_up(&mut v, "z"); assert_eq!(v, vec!["a"]); }
}

// helper for the test (since Vec::reverse returns ()):
trait ReverseClone { fn reverse_clone(self) -> Self; }
impl<T> ReverseClone for Vec<T> {
    fn reverse_clone(mut self) -> Self { self.reverse(); self }
}
```

- [ ] **Step 6: Run + commit**

Run: `cargo test -p juhradial-shared && cargo clippy -p juhradial-shared --all-targets -- -D warnings`

```bash
git add juhradial-shared/src/popup.rs juhradial-shared/src/lib.rs juhradial-shared/src/config.rs
git commit -m "feat(shared): add PopupConfig + quick-action catalogs"
```

#### Task 0.2 — Extend `org.juhradial.Daemon` D-Bus interface

**Files:**
- Modify: `daemon/src/dbus/interface.rs` (add `get_active_device_state`, `show_popup`, `ensure_overlay_running`, `device_state_changed` signal)
- Modify: `daemon/src/dbus/service.rs` (add `device_name`, `device_id`, `connection_kind`, `popup_spawner`, `overlay_spawner` fields)
- Modify: `daemon/src/dbus/init.rs` (plumb the new fields)
- Create: `daemon/src/overlay_spawner.rs` (process-handle holder + idempotent `ensure_running()` helper)
- Modify: `daemon/src/main.rs` (`mod overlay_spawner;` + construct the spawner before init_dbus_service)

The wire surface (zbus interface annotations, single block) becomes:

```rust
// Returns: (battery_percent, charging, connection_kind, device_name, device_id)
// connection_kind ∈ {"bluetooth", "unifying", "bolt", "usb", "off"}
async fn get_active_device_state(&self) -> fdo::Result<(u8, bool, String, String, String)> { … }

// Spawns the popup binary at the given panel-anchor rect.
// panel_x/y/w/h are stage-absolute Mutter logical pixels of the
// indicator's panel rect so the popup can position its tip beneath the
// icon. Fire-and-forget — the popup is one-shot, exits on its own
// close/dismiss path.
async fn show_popup(&self, panel_x: i32, panel_y: i32, panel_w: i32, panel_h: i32) -> fdo::Result<()> { … }

// Idempotent. Returns Ok(()) immediately if `org.juhradial.overlay` is
// already on the session bus. Otherwise spawns `juhradial-overlay-rs`
// and waits up to 3s for the bus name to appear; returns Err(...) on
// spawn failure or timeout. Backs the indicator's "Radial Overlay"
// toggle so the extension never has to spawn long-lived processes
// itself (§3.4 of INDICATOR_DESIGN.md).
async fn ensure_overlay_running(&self) -> fdo::Result<()> { … }

// Emitted whenever any of (battery, charging, connection_kind, device_name)
// changes. Indicator subscribes for push updates so it doesn't have to
// poll on the critical path.
#[zbus(signal)]
async fn device_state_changed(
    emitter: &SignalEmitter<'_>,
    battery: u8, charging: bool, connection: String, device_name: String, device_id: String,
) -> zbus::Result<()>;
```

`overlay_spawner.rs` outline (~60 LOC):

```rust
//! Owns the child process handle for juhradial-overlay-rs.
//!
//! Wraps tokio::process::Command::spawn so the daemon's ensure_overlay_running
//! D-Bus handler can call it without duplicating the bus-name-probe + spawn +
//! wait-for-bus dance. Idempotent: a second call while the overlay is alive
//! is a no-op. The held Child is dropped (not killed) on daemon shutdown so
//! the user's overlay stays up across daemon restarts.

use std::sync::Mutex;
use tokio::process::{Child, Command};
use tokio::time::{Duration, Instant};
use zbus::Connection;

const OVERLAY_BUS_NAME: &str = "org.juhradial.overlay";
const SPAWN_WAIT_TIMEOUT: Duration = Duration::from_secs(3);
const SPAWN_WAIT_POLL: Duration = Duration::from_millis(100);

pub struct OverlaySpawner {
    child: Mutex<Option<Child>>,
}

impl OverlaySpawner {
    pub fn new() -> Self { Self { child: Mutex::new(None) } }

    pub async fn ensure_running(&self, conn: &Connection) -> Result<(), String> {
        if name_owned(conn, OVERLAY_BUS_NAME).await {
            return Ok(());
        }
        let child = Command::new("juhradial-overlay-rs")
            .spawn()
            .map_err(|e| format!("spawn juhradial-overlay-rs: {e}"))?;
        *self.child.lock().unwrap() = Some(child);

        let deadline = Instant::now() + SPAWN_WAIT_TIMEOUT;
        while Instant::now() < deadline {
            if name_owned(conn, OVERLAY_BUS_NAME).await { return Ok(()); }
            tokio::time::sleep(SPAWN_WAIT_POLL).await;
        }
        Err(format!("overlay did not appear on the bus within {:?}", SPAWN_WAIT_TIMEOUT))
    }
}

async fn name_owned(conn: &Connection, name: &str) -> bool {
    use zbus::fdo::DBusProxy;
    let Ok(proxy) = DBusProxy::new(conn).await else { return false };
    proxy.name_has_owner(name.try_into().unwrap()).await.unwrap_or(false)
}
```

`ShowPopup` spawner uses the same `tokio::process::Command` pattern:

```rust
let _ = Command::new("juhradial-popup")
    .args(["--panel-rect", &format!("{panel_x},{panel_y},{panel_w},{panel_h}")])
    .spawn();
```

Fire-and-forget — if the binary is missing, log warn (mirroring the overlay launch pattern). No process-handle retention; popup is one-shot.

Add `DeviceStateChanged` emit in `daemon/src/battery.rs` after each successful battery poll **only when any field changed** (debounce). The device_name + connection are read from the existing hidpp manager's device-info cache; when the daemon isn't currently linked to a device, return empty strings + `"off"` for the connection.

Steps mirror Task 0.1 — write failing test in `daemon/src/dbus/tests.rs` for the new methods using a fake battery state + a stub OverlaySpawner that doesn't actually spawn, then implement.

#### Task 0.3 — Write the GSettings schema

**Files:**
- Create: `gnome-extension/juhradial-indicator@dev.juhlabs.com/schemas/org.gnome.shell.extensions.juhradial-indicator.gschema.xml`

Verbatim contents (mirrors prompt's spec; lower-case kebab keys as GNOME convention):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<schemalist gettext-domain="juhradial-indicator">
  <schema id="org.gnome.shell.extensions.juhradial-indicator"
          path="/org/gnome/shell/extensions/juhradial-indicator/">
    <key name="display-mode" type="s">
      <choices><choice value="percent"/><choice value="icon"/><choice value="both"/></choices>
      <default>'both'</default>
      <summary>What to render in the panel button</summary>
    </key>
    <key name="show-mouse-glyph" type="b"><default>true</default></key>
    <key name="tint-mouse-glyph" type="b"><default>true</default></key>
    <key name="threshold-critical" type="i">
      <range min="1" max="99"/><default>15</default>
    </key>
    <key name="threshold-low" type="i">
      <range min="1" max="99"/><default>30</default>
    </key>
    <key name="color-critical" type="s"><default>'#FF3E5A'</default></key>
    <key name="color-low" type="s"><default>'#F2C94C'</default></key>
    <key name="color-healthy" type="s"><default>'#5BE095'</default></key>
    <key name="color-charging" type="s"><default>'#5BE095'</default></key>
    <key name="apply-color-to-text" type="b"><default>true</default></key>
    <key name="panel-target" type="s">
      <choices><choice value="auto"/><choice value="topbar"/><choice value="dtp"/><choice value="both"/></choices>
      <default>'auto'</default>
    </key>
    <key name="position" type="s">
      <choices><choice value="left"/><choice value="center"/><choice value="right"/></choices>
      <default>'right'</default>
    </key>
    <key name="position-index" type="i"><default>0</default></key>
    <key name="click-behavior" type="s">
      <choices><choice value="popup"/><choice value="settings"/><choice value="none"/></choices>
      <default>'popup'</default>
    </key>
    <key name="refresh-interval" type="i">
      <range min="5" max="120"/><default>30</default>
    </key>
  </schema>
</schemalist>
```

Compile to GVDB at install time via `glib-compile-schemas`. `dev-install-ext.sh` does this for the existing extension; we extend that script in Task 1.6.

**Phase 0 ships as PR #1.** Everything else depends on Phase 0 being merged.

---

### Phase A — Extract `juhradial-widgets` and `juhradial-window` sibling crates

**Why a separate phase:** confirmed in the 2026-05-24 clarification round. Doing it after Phase 0 (so the rename has landed) and before Phase 1/2/3 means the new consumers can import from the new crates from day one, instead of binding to a moving target.

#### Task A.1 — Stand up `juhradial-widgets` crate

**Files:**
- Create: `juhradial-widgets/Cargo.toml`
- Create: `juhradial-widgets/src/lib.rs`
- Move (not copy): `settings-rs/src/widgets.rs` → `juhradial-widgets/src/widgets.rs`
- Move: `settings-rs/src/style.rs` → `juhradial-widgets/src/style.rs`
- Move: `settings-rs/src/palette.rs` → `juhradial-widgets/src/palette.rs`
- Modify: `settings-rs/src/main.rs` (drop the three `mod` declarations; replace with `use juhradial_widgets::{widgets, style, palette};`)
- Modify: every `settings-rs/src/tabs/*.rs` that uses `crate::widgets` / `crate::style` / `crate::palette` (~12 files): replace `crate::widgets::X` with `juhradial_widgets::widgets::X`, similarly for `style` and `palette`. Most callsites already use `use crate::widgets::{labeled_slider, …};`, so the rename is `crate::` → `juhradial_widgets::`.
- Modify: `Cargo.toml` (workspace `members` list)
- Modify: `settings-rs/Cargo.toml` (add `juhradial-widgets = { path = "../juhradial-widgets" }`)

`Cargo.toml` for the new crate:
```toml
[package]
name = "juhradial-widgets"
version = "0.0.1"
edition = "2021"
license = "GPL-3.0"
description = "Shared iced widgets, style closures, and palette for JuhRadial UIs"

[dependencies]
iced = { version = "0.14", features = ["wayland", "x11"] }
# No tokio / image / canvas — leaf widgets only. Consumers add features.
```

`juhradial-widgets/src/lib.rs`:
```rust
//! Shared UI primitives for JuhRadial MX (settings, overlay, popup).
//!
//! Three modules:
//!   * [`widgets`] — small composite widgets (labeled sliders, section
//!     headers, etc.).
//!   * [`style`] — palette-keyed `Fn(&Theme) -> Style` closures.
//!   * [`palette`] — the catppuccin-style colour palette plus accent
//!     resolution.
//!
//! Originally lived inside `settings-rs/`; extracted in 2026-05 so
//! `popup-rs/` (and eventually `overlay-rs/`) could consume them
//! without copy-paste.

pub mod widgets;
pub mod style;
pub mod palette;
```

- [ ] **Step 1: Move files, register crate, update settings-rs imports**
- [ ] **Step 2: `cargo build --workspace` — expect clean compile**
- [ ] **Step 3: `cargo test --workspace` — every existing test still passes (the move is a relocation, no logic change)**
- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor: extract juhradial-widgets sibling crate

Moves widgets.rs / style.rs / palette.rs out of settings-rs into a
new sibling crate so popup-rs (Phase 3) can import the same UI
primitives without copy-paste. settings-rs and (later) overlay-rs
remain the public consumers; no behaviour change."
```

#### Task A.2 — Stand up `juhradial-window` crate

**Files:**
- Create: `juhradial-window/Cargo.toml`
- Create: `juhradial-window/src/lib.rs`
- Create: `juhradial-window/src/settings.rs` — `frameless_topmost(app_id: &str, size: iced::Size) -> iced::window::Settings`
- Create: `juhradial-window/src/cursor_helper.rs` — extracted from `overlay-rs/src/ext_positioner.rs` (the `CursorHelperProxy`, `move_overlay`, `get_focused_window_class`, `list_monitors`, `raise_overlay`)
- Modify: `overlay-rs/src/main.rs` — drop `mod ext_positioner;`
- Modify: `overlay-rs/src/app.rs` — change `use crate::ext_positioner::{…}` to `use juhradial_window::cursor_helper::{…}`; replace the 6-line iced window construction with `let window = juhradial_window::frameless_topmost(APP_ID, iced::Size::new(WINDOW_SIZE as f32, WINDOW_SIZE as f32));`.
- Delete: `overlay-rs/src/ext_positioner.rs`
- Modify: `overlay-rs/Cargo.toml` — add `juhradial-window = { path = "../juhradial-window" }`
- Modify: `Cargo.toml` (workspace members list)

`juhradial-window/Cargo.toml`:
```toml
[package]
name = "juhradial-window"
version = "0.0.1"
edition = "2021"
license = "GPL-3.0"
description = "Frameless topmost xdg-shell window helpers + GNOME-cursor-helper D-Bus client"

[dependencies]
iced = { version = "0.14", features = ["wayland", "x11"] }
zbus = "5"
tracing = "0.1"
```

`juhradial-window/src/settings.rs`:
```rust
//! Frameless, transparent, always-on-top xdg-shell window helper.
//!
//! Returns an `iced::window::Settings` shaped for an overlay-style
//! window: no decorations, transparent root, non-resizable, always
//! on top, fixed size. Caller supplies the wayland `app_id` (which
//! becomes the xdg-toplevel app_id) and the size in logical px.
//!
//! Used by the radial overlay and the indicator popup. Mutter
//! doesn't advertise wlr-layer-shell on stable GNOME, so neither
//! window can position itself; both delegate to the cursor-helper
//! GNOME extension via [`crate::cursor_helper`].

use iced::window::{Level, Settings};

pub fn frameless_topmost(app_id: &str, size: iced::Size) -> Settings {
    let mut s = Settings::default();
    s.size = size;
    s.decorations = false;
    s.transparent = true;
    s.resizable = false;
    s.level = Level::AlwaysOnTop;
    s.platform_specific.application_id = app_id.to_string();
    s
}
```

`juhradial-window/src/cursor_helper.rs` is a verbatim move of `overlay-rs/src/ext_positioner.rs` into the new crate's namespace.

- [ ] **Step 1: Move, register, update overlay imports + 6-line window setup**
- [ ] **Step 2: `cargo build --workspace && cargo test --workspace`**
- [ ] **Step 3: Sanity-run the overlay against a running daemon** (`cargo run -p juhradial-overlay-rs --release` + `busctl --user call org.juhradial.Daemon /org/juhradial/Daemon org.juhradial.Daemon ShowMenu xy 400 300`) — overlay still appears under the cursor.
- [ ] **Step 4: Commit**

**Phase A ships as PR #2.**

---

### Phase 1 — GNOME Shell extensions (new indicator + migrate cursor to TS)

#### Task 1.0 — Migrate `juhradial-cursor@dev.juhlabs.com/extension.js` to TypeScript

Confirmed in the clarification round: both extensions land on the same tsconfig.

**Files:**
- Create: `gnome-extension/tsconfig.json` (shared)
- Create: `gnome-extension/package.json` (shared — dev deps `typescript`, `@girs/gjs`, `@girs/gnome-shell-49`, `@girs/meta-15`, `@girs/gtk-4.0`, `@girs/glib-2.0`, `@girs/gio-2.0`)
- Create: `gnome-extension/juhradial-cursor@dev.juhlabs.com/extension.ts` (typed rewrite of existing `extension.js`)
- Delete: `gnome-extension/juhradial-cursor@dev.juhlabs.com/extension.js` (tracked under git but compiled output going forward)
- Add: `gnome-extension/.gitignore` entry for compiled `*.js` (compiled in CI / dev install)
- Modify: `dev-install-ext.sh` — run `npx tsc -p gnome-extension/tsconfig.json` then install both extensions

`tsconfig.json` targets ES2022, module ESNext, `outDir`/`rootDir` configured per-extension so `extension.ts` compiles to `extension.js` alongside.

Behaviour-preservation test: after migration, `busctl --user call org.juhradial.CursorHelper /org/juhradial/CursorHelper org.juhradial.CursorHelper GetCursorPosition` still returns `(ii)` and `MoveOverlay` still works against the overlay. Run `dev-test.sh` to verify the overlay still positions correctly.

This task lands as its own commit at the top of PR #3 so the indicator work isn't entangled with the cursor migration if either needs to be reverted.

#### Task 1.1 — Scaffold the indicator extension dir

**Files:**
- Create: `gnome-extension/juhradial-indicator@dev.juhlabs.com/metadata.json`

`metadata.json` mirrors `juhradial-cursor`'s: `shell-version: ["45","46","47","48","49","50","51"]`, `uuid: juhradial-indicator@dev.juhlabs.com`. The shared `tsconfig.json` from Task 1.0 already covers this extension's TS build.

#### Task 1.2 — `lib/settings.ts` — typed GSettings wrapper + round-trip test

`getString('display-mode')` is `as DisplayMode`. Subscribe with `connect('changed::*', cb)` and return an `unsub()` closure. Round-trip test runs under `gjs --module` with a stub backend.

#### Task 1.3 — `lib/format.ts` — band classification + color resolution

Pure functions, easy unit tests:

```ts
export type Band = 'critical' | 'low' | 'healthy' | 'charging';
export function bandFor(pct: number, charging: boolean, thresholds: Thresholds): Band {
  if (charging) return 'charging';
  if (pct <= thresholds.critical) return 'critical';
  if (pct <= thresholds.low) return 'low';
  return 'healthy';
}
```

Tests cover the exact thresholds from the design (15 / 30) and the charging override.

#### Task 1.4 — `lib/battery.ts` — D-Bus client + critical-band one-shot notify

`Gio.DBusProxy` against `org.juhradial.Daemon`. Subscribe to the `DeviceStateChanged` signal for push updates; fall back to `refresh-interval`-spaced `GetActiveDeviceState` polls when the signal stream stalls. Mock mode (env var `JUHRADIAL_INDICATOR_MOCK=1`) returns a synthetic cycling battery for prefs-only development. Cancellable; tear down on `disable()`.

Critical-band notification (confirmed UX):
- Track `_armed: boolean = true` per-device.
- On each state update, recompute `bandFor(pct, charging, thresholds)`.
- If `band === 'critical' && !charging && _armed`: emit `Gio.Notification` with `urgency=CRITICAL`, title `_("MX device low")`, body `_("%s at %d%% — connect charging cable").format(deviceName, pct)`, set `_armed = false`.
- If `band !== 'critical' || charging`: set `_armed = true` (re-arm).
- Tests in `lib/battery.test.ts` cover arm/disarm transitions across the 30% → 14% → charging → 12% sequence.

#### Task 1.4b — `lib/supervisor.ts` — stack health probes + remediation

**Confirmed scope addition (2026-05-24).** The indicator is the single front-of-house surface for "is the JuhRadial stack working" and the single click-to-remediate surface. Implements §3.4 of `INDICATOR_DESIGN.md`.

**Files:**
- Create: `gnome-extension/juhradial-indicator@dev.juhlabs.com/lib/supervisor.ts`
- Modify: `gnome-extension/juhradial-indicator@dev.juhlabs.com/extension.ts` (subscribe `supervisor.health$` to update panel-icon class + right-click menu items)

Public surface:

```ts
export type StackHealth = {
    daemonRunning: boolean;     // org.juhradial.Daemon on the bus AND systemctl active
    deviceLinked:  boolean;     // GetActiveDeviceState returned connection != "off"
    overlayRunning: boolean;    // org.juhradial.overlay on the bus
    cursorExtension: boolean;   // juhradial-cursor enabled
    daemonName: string;         // active device's display name when linked
};

export class Supervisor {
    constructor(private settings: Gio.Settings, private cancellable: Gio.Cancellable);
    /** Async observable of stack health. Emits on enable, then on each
     *  refresh-interval tick, and on demand via `poll()`. */
    health$: AsyncIterable<StackHealth>;
    poll(): Promise<StackHealth>;
    /** systemctl --user start juhradialmx-daemon.service */
    startDaemon(): Promise<void>;
    /** Asks the daemon to spawn the overlay process (NOT us). Returns when
     *  org.juhradial.overlay appears on the bus or the daemon errors out. */
    ensureOverlay(): Promise<void>;
    destroy(): void;
}
```

Implementation rules (mirrors the architecture rules in `INDICATOR_DESIGN.md` §3.4):

- **Never** spawn long-lived processes from the extension. `startDaemon()` uses `Gio.Subprocess.new(['systemctl', '--user', 'start', 'juhradialmx-daemon.service'], …)`. `ensureOverlay()` calls `org.juhradial.Daemon.EnsureOverlayRunning()` — the daemon owns the spawn.
- Daemon-running probe uses both signals (bus-name probe via `Gio.DBusConnection.list_names()` AND `systemctl --user is-active`) to distinguish "daemon crashed, systemd will restart" from "daemon not installed".
- Remediation actions are **user-confirmed** by default (right-click menu items, popup footer buttons). The exception is `ensureOverlay()` when the user toggles `Radial Overlay` ON — that toggle is itself the consent.
- `destroy()` cancels every in-flight `Gio.DBusProxy` and `Gio.Subprocess` via the shared `cancellable`. Called from `extension.disable()`.

- [ ] **Step 1: Write unit tests for the band-classification + remediation logic that don't need a live D-Bus**

```ts
// supervisor.test.ts
describe('Supervisor', () => {
    it('reports daemon down when bus name is absent', async () => { … });
    it('reports daemon down when systemctl says inactive even if bus name is present', async () => { … });
    it('reports overlay missing only when daemon is up but org.juhradial.overlay is absent', async () => { … });
});
```

- [ ] **Step 2: Implement Supervisor against `Gio.DBusConnection`, `Gio.Subprocess`, and the daemon proxy from `lib/battery.ts`**

- [ ] **Step 3: Wire into `extension.ts`**

```ts
// extension.ts (enable())
this._supervisor = new Supervisor(this._settings, this._cancellable);
for await (const h of this._supervisor.health$) {
    this._panelButton.setHealth(h);             // updates icon color + tooltip
    this._rightClickMenu.setHealth(h);          // adds/removes "Start daemon" item
}
```

- [ ] **Step 4: Integration smoke test under `dev-test.sh`**

```bash
systemctl --user stop juhradialmx-daemon.service
# panel icon turns critical-color within `refresh-interval` seconds
# right-click → "Start daemon" appears
# clicking it brings the daemon back; icon goes healthy
```

- [ ] **Step 5: Commit**

```bash
git add gnome-extension/juhradial-indicator@dev.juhlabs.com/lib/supervisor.ts \
        gnome-extension/juhradial-indicator@dev.juhlabs.com/extension.ts
git commit -m "feat(indicator): add stack supervisor / unified health surface"
```

#### Task 1.5 — `lib/placement.ts` — DTP detection

Probe `Main.extensionManager.lookup('dash-to-panel@jderose9.github.com')`. If present + enabled, call its public `getStatusAreaSection(box, monitorIndex)` API. Otherwise add to `Main.panel`. Honors `position` (`_leftBox|_centerBox|_rightBox`) and `position-index`.

#### Task 1.6 — `extension.ts` — `PanelMenu.Button` + click dispatch

Pure composition of the lib modules above. Right-click: tiny `PopupMenu` with three items (Open Settings / Open Extension Preferences / About). Left-click: dispatch by `click-behavior`.

For `popup`: call `proxy.ShowPopup(panel_x, panel_y, panel_w, panel_h)`, sourcing rect from `this.actor.get_transformed_extents()`.

#### Task 1.7 — `prefs.ts` — single-page Adw

Groups in design order: Preview, Display, Battery level colors, Placement, Behavior, About, Reset. Live preview re-renders on every `changed::*`. Threshold "drag-the-handles" is a `Gtk.DrawingArea` overlay with two `Gtk.GestureDrag` handlers; band colors are `Gtk.ColorButton`. Reset writes schema defaults via `Gio.Settings.reset(key)` for every key.

#### Task 1.8 — `stylesheet.css` + symbolic SVG icons

Four icons in `icons/`: `jr-mouse-symbolic.svg` and three battery-band variants. Symbolic = single-color, restyled by St via CSS `color:` token. Pull stroke widths to match `--jr-fg` typography of the design.

#### Task 1.9 — Update `dev-install-ext.sh` and add a smoke-test recipe

Install both extensions, restart Shell in nested-mode (`dbus-run-session -- gnome-shell --nested --wayland`), open the popup, screenshot.

**Phase 1 ships as PR #2.**

---

### Phase 2 — Settings app: "Indicator Popup" tab

#### Task 2.1 — Add `Tab::IndicatorPopup` to the enum

**Files:**
- Modify: `settings-rs/src/main.rs` (add variant + label + glyph + icon_name + tag in their five matches; insert into `ALL` array between `PointScroll` and `Haptic`)

Stub the new tab — register it in `mod tabs { pub mod indicator_popup; }` and add a placeholder `view` that returns `text("TODO")`. Land this as one small task so the rest of the settings UI compiles + runs.

#### Task 2.2 — `state/popup_config_view.rs` — bridge `PopupConfig` ↔ UI messages

In `settings-rs/src/main.rs` next to the existing `Message` enum, add `Message::SetPopupMode(PopupMode)`, `Message::TogglePopupHostButtons(bool)`, `Message::SetPopupHostLabelStyle(HostLabelStyle)`, `Message::PopupMoveUp(QuickListId, String)`, `Message::PopupMoveDown(...)`, `Message::PopupRemove(...)`, `Message::PopupAdd(...)`, `Message::SetPopupVolumeOnScroll(bool)`, `Message::SetPopupCloseOnAction(bool)`, `Message::SetPopupAnimations(bool)`. `QuickListId` is an enum `{ Toggles, Sliders }` so we route reorder ops to the right vec without four times the code.

Each update arm mutates the in-memory `AppConfig` and schedules the same `persist::save` debounce the rest of the app uses (see `tabs/scroll.rs` for the pattern).

#### Task 2.3 — `tabs/indicator_popup.rs` — full view

Mirror `tabs/scroll.rs` structure: section_header + a card per section. Sections in the canonical design order:

1. **Mode** — two `button`s side-by-side, the active one styled with `style::btn_primary(pal)`, the other `style::btn_secondary(pal)`.
2. **Easy-Switch host buttons** — `toggler` + two-button radio. Live preview using `state.daemon.active_device.hosts`.
3. **Quick toggles** — reorderable list. Each row is a `row![icon, label, sub, Space::Fill, kind_pill("TOGGLE"), up_btn, down_btn, trash_btn]`, with `up_btn`/`down_btn`/`trash_btn` disabled at the edges. "Available" rail below with `add_btn` rows.
4. **Quick sliders** — same structure, only rendered when `mode == Power`.
5. **Interactions** — three `toggler` rows.

#### Task 2.4 — End-to-end test under `cargo test`

Write `settings-rs/tests/popup_tab_compiles.rs` that boots the app with `iced::application::Application::run_with` in a headless mode, switches to `Tab::IndicatorPopup`, and asserts the view produces an `Element` (no panic). Iced 0.14 doesn't have full headless rendering but the message-dispatch tests catch most regressions.

**Phase 2 ships as PR #3.**

---

### Phase 3 — `popup-rs` (the daemon-spawned popup binary)

#### Task 3.1 — Add `popup-rs/` to the workspace

**Files:**
- Create: `popup-rs/Cargo.toml`
- Create: `popup-rs/src/main.rs`
- Modify: `Cargo.toml` (workspace member list)

`Cargo.toml` mirrors `overlay-rs/Cargo.toml`'s iced + zbus + tokio + serde_json + notify deps. Add `gio = { version = "0.20", features = ["v2_74"] }` for GSettings consumption.

#### Task 3.2 — Window setup via the new `juhradial-window` crate

```rust
// popup-rs/src/main.rs
use juhradial_window::frameless_topmost;
use juhradial_window::cursor_helper::move_overlay;

const APP_ID: &str = "org.juhradial.popup";
const POPUP_W: f32 = 360.0;
const POPUP_H: f32 = 480.0;

let window = frameless_topmost(APP_ID, iced::Size::new(POPUP_W, POPUP_H));
iced::application(boot, update, view)
    .title("JuhRadial Popup")
    .window(window)
    .style(transparent_style)
    .subscription(subscription)
    .run()?;
```

After first frame (`Message::FirstFrame`), `Task::perform(move_overlay(APP_ID.into(), anchor_x, anchor_y, -1), Message::Positioned)`. CLI args (`--panel-rect x,y,w,h` passed by the daemon's `ShowPopup` spawn) tell the popup where the indicator is.

Outer container: 14px radius, 1px `JR_FG @ 12%` border. Drop shadow as a slightly larger semi-transparent container behind. Click-outside dismissal: fullscreen transparent `mouse_area` backdrop (the X11 fallback approach from the prompt; layer-shell can't run on GNOME anyway, so this is the only path).

#### Task 3.3 — Config loading + watching

Load `~/.config/juhradial/config.json` via `juhradial_shared::config::load()`, watch with `notify::recommended_watcher`, broadcast reloads as `Message::ConfigReloaded(AppConfig)`. Mirror `overlay-rs/src/config.rs`.

#### Task 3.4 — GSettings consumption for thresholds + colors

```rust
let settings = gio::Settings::new("org.gnome.shell.extensions.juhradial-indicator");
let crit = settings.int("threshold-critical") as u8;
let low  = settings.int("threshold-low")      as u8;
let color_crit = settings.string("color-critical").to_string();
// …
settings.connect_changed(Some("threshold-critical"), |_, _| { /* send Message::GsettingsChanged */ });
```

Drives the popup's battery ring color band.

#### Task 3.5 — Quick toggles + sliders bound to daemon D-Bus

Each toggle/slider id maps to a daemon D-Bus method (gaming → `SetGamingMode`, dpi → `SetDpi`, scroll → `SetSmartShift`/`SetWheelMode`, etc.). The daemon already exposes most of these; for `radial`, `haptics`, `flow`, `smart`, `highlight`, `haptic_i`, `accel` we use whatever the equivalent existing methods are (audit during implementation).

#### Task 3.6 — Volume-on-scroll

Root container wraps `mouse_area`. Capture `iced::Event::Mouse(mouse::Event::WheelScrolled { delta })`. If `popup_config.volume_on_scroll && window_focused`, emit `Message::VolumeStep(sign)`. Update arm spawns `Task::perform(async { tokio::process::Command::new("wpctl").args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("5%{}", if sign > 0 {"+"} else {"-"})]).status().await }, |_| Message::Noop)`.

Focus-tracking: `window::events()` subscription gives us `window::Event::Focused / Unfocused`. Click-outside on the backdrop emits `Message::Dismiss`.

#### Task 3.7 — `install.sh` updates

Add `juhradial-popup` to the installed binaries (cargo build --release -p juhradial-popup-rs → /usr/local/bin/juhradial-popup, matching where the overlay lands).

**Phase 3 ships as PR #4.**

---

## ACCEPTANCE WALKTHROUGH — how the reviewer verifies

Run from a fresh shell session after `dev-install-ext.sh`:

1. Both extensions are enabled — `gnome-extensions list --enabled`.
2. Daemon is running — `busctl --user introspect org.kde.juhradialmx /org/kde/juhradialmx/Daemon` shows `ShowPopup`, `GetActiveDeviceState`, `DeviceStateChanged`.
3. Top-bar indicator at 30% battery shows yellow glyph + "30%" — visually compare with `design/juhradial-indicator/index.html#indicator-states`.
4. Click → daemon spawns `juhradial-popup`, window appears tip-aligned under the icon. **Not** a GJS PopupMenu — verify via `gnome-extensions show juhradial-indicator@dev.juhlabs.com` debug log: "ShowPopup dispatched".
5. Prefs window matches `ext-prefs-window` artboard. Drag threshold handle → `gsettings get org.gnome.shell.extensions.juhradial-indicator threshold-critical` reflects the new value.
6. `Reset to defaults` writes every key back to the schema defaults — verify with `gsettings list-recursively org.gnome.shell.extensions.juhradial-indicator`.
7. Sidebar in `juhradial-settings`: "Indicator Popup" appears between "Point & Scroll" and "Haptic Feedback".
8. Switching to Power User reveals "Quick sliders" section.
9. Up/down on a quick-toggle row reorders persistently — confirm by reading `~/.config/juhradial/config.json` `.popup.simple_toggles`.
10. With volume-on-scroll on, wheel-scroll while the popup is focused moves `wpctl get-volume @DEFAULT_AUDIO_SINK@`.

---

## SELF-REVIEW

- **Spec coverage:** every section of the prompt (extension scope, prefs scope, settings tab scope, popup window scope, NON-GOALS, acceptance criteria) maps to a task above. Two prompt items — `juhradial-widgets` crate extraction and `juhradial-window` crate extraction — are *explicitly deferred* with rationale in the "Corrections" table.
- **Placeholder scan:** none.
- **Type consistency:** `PopupConfig` field names match between Phase 0 (defined), Phase 2 (consumed by settings UI), and Phase 3 (consumed by popup binary). D-Bus signature `(yybsss)` for `GetActiveDeviceState` and `(yysss)` for the `DeviceStateChanged` signal arguments are consistent across daemon impl + extension consumer + popup consumer.

---

## AMBIGUITIES — resolved 2026-05-24

| # | Decision |
|---|---|
| 1 | Subprocess popup binary (`juhradial-popup`) spawned by daemon's `ShowPopup` handler. |
| 2 | **Rename to `org.juhradial.Daemon`** in Phase 0. Flag-day across daemon, overlay, settings, packaging, install scripts. |
| 3 | Popup config as a new field on existing `config.json`. |
| 4 | **Extract both** `juhradial-widgets` and `juhradial-window` sibling crates in Phase A. |
| 5 | **Migrate the cursor extension to TypeScript** alongside the new indicator in Phase 1. Shared tsconfig under `gnome-extension/`. |
| 6 | **Emit critical-band notification once per crossing** (re-armable on charge / band exit). UX copy in §Phase 1 Task 1.4. |
| 7 | Render unwired quick-toggles, log "not yet wired" on click. Full wiring as Phase 3.5 follow-up. |
| 8 | `ShowPopup` panel_rect is **stage-absolute logical pixels**; popup passes `monitor=-1` to `MoveOverlay`. |

One open implementation detail remaining; resolves during Phase 1 work without blocking Phase 0:

- **Dash-to-Panel detection method** — plan defaults to `Main.extensionManager.lookup('dash-to-panel@jderose9.github.com')` (modern API). Falls back to the legacy `Gio.Settings.new(SCHEMA).get_strv('panel-positions')` schema probe if `lookup` returns null on older DTP builds.

---

## EXECUTION HANDOFF

Plan complete and saved to `docs/plans/indicator-implementation.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch with checkpoints for review.

Which approach?
