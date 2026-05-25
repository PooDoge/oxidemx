# JuhRadial Indicator + Popup + Settings — Design Document

**Status:** Pre-implementation. Spec locked 2026-05-24. Implementation plan: [`docs/plans/indicator-implementation.md`](docs/plans/indicator-implementation.md). Original UI spec: [`design/juhradial-indicator/CLAUDE_CODE_PROMPT.md`](design/juhradial-indicator/CLAUDE_CODE_PROMPT.md). Deltas from that spec: [`design/juhradial-indicator/SPEC_ADDENDUM.md`](design/juhradial-indicator/SPEC_ADDENDUM.md).
**Date:** 2026-05-24
**Owners:**
- `gnome-extension/juhradial-indicator@dev.juhlabs.com/` (new)
- `gnome-extension/juhradial-cursor@dev.juhlabs.com/` (TS migration in scope)
- `popup-rs/` (new sibling workspace member)
- `settings-rs/src/tabs/indicator_popup.rs` (new tab)
- `juhradial-widgets/` (new sibling crate — extracted from settings-rs)
- `juhradial-window/` (new sibling crate — extracted from overlay-rs)
- D-Bus surface on `daemon/src/dbus/interface.rs`

> **One line:** A GNOME Shell panel indicator that surfaces MX-device battery, opens a daemon-rendered Iced popup with quick toggles/sliders, and acts as the single front-of-house health surface for the JuhRadial stack.

---

## 1. Goal, scope, and responsibilities

### Goal

Give Linux MX users the same at-a-glance battery + quick-controls surface that ships on macOS (Logi Options+) and Windows (Logi Options), with a UI that respects the JuhRadial design language and a stack that respects GNOME's "extensions don't own long-running processes" convention.

### Three responsibilities (co-equal — none is the "main" one)

1. **Battery + device status surface** — a top-bar panel button showing battery percentage and/or symbolic glyph, color-banded by user-editable critical/low thresholds.
2. **Quick-action popup** — a click-anywhere-to-open popup containing easy-switch host buttons, quick toggles (gaming mode, haptic feedback, radial overlay, …), and (in Power User mode) quick sliders (DPI, scroll, haptic intensity, pointer accel). The popup is **rendered by an Iced subprocess** the daemon spawns, not by GJS, so it shares the design tokens of `settings-rs` and dodges GNOME's PopupMenu styling limits.
3. **Stack supervisor + health surface** — added 2026-05-24 per the live-conversation clarification. The indicator is the single place a user sees "is JuhRadial healthy?" and the single place a user clicks to remediate. This crosses several discrete checks (§3.4) that previously had no UI home.

### In scope

- Phase 0: rename daemon D-Bus name `org.kde.juhradialmx` → `org.juhradial.Daemon`. Add `GetActiveDeviceState`, `ShowPopup`, `DeviceStateChanged` to the wire.
- Phase A: extract `juhradial-widgets` and `juhradial-window` sibling crates so `settings-rs`, `overlay-rs`, `popup-rs` share UI primitives instead of copy-pasting.
- Phase 1: ship the new GNOME extension as TypeScript; migrate `juhradial-cursor` to TypeScript at the same time (shared `tsconfig.json`).
- Phase 2: add `Tab::IndicatorPopup` to `settings-rs` between Point & Scroll and Haptic.
- Phase 3: ship `popup-rs`, the daemon-spawned Iced window.

### Out of scope (for this body of work)

- Redesigning the existing Devices page in `settings-rs`.
- Pairing UX changes.
- Drag-to-reorder gestures for quick toggles/sliders (keyboard-accessible up/down arrows only).
- Windows / macOS parity beyond what GSettings + daemon D-Bus already permit on Linux.
- Wiring the unwired quick-action ids (`highlight`, `flow`) to their eventual daemon methods — these render as toggles but log "not yet wired" on click. Wiring is a Phase 3.5 follow-up tracked at the bottom of the plan.

---

## 2. Architecture overview

```
                        ┌──────────────────────────────┐
                        │       GNOME Shell (Mutter)   │
                        │                              │
                        │  ┌─────────────────────────┐ │
                        │  │ juhradial-indicator     │ │  (NEW, TypeScript)
                        │  │  - panel button         │ │  Phase 1
                        │  │  - libadw prefs dialog  │ │
                        │  │  - supervisor checks    │ │
                        │  └────────────┬────────────┘ │
                        │               │              │
                        │  ┌─────────────────────────┐ │
                        │  │ juhradial-cursor        │ │  (TS migration)
                        │  │  - GetCursorPosition    │ │  Phase 1
                        │  │  - MoveOverlay          │ │
                        │  │  - GetFocusedClass      │ │
                        │  └────────────┬────────────┘ │
                        └──────────────│───────────────┘
                                       │ D-Bus (session)
                                       │
            ┌──────────────────────────┼──────────────────────────┐
            │                          │                          │
            ▼                          ▼                          ▼
  ┌──────────────────┐      ┌────────────────────┐      ┌────────────────────┐
  │   juhradiald     │      │ juhradial-overlay  │      │  juhradial-popup   │
  │  (the daemon)    │◄────►│   (radial menu)    │      │  (Iced popup —     │
  │   - HID++        │      │  Phase A: imports  │      │   daemon-spawned)  │
  │   - org.juhradial│      │  juhradial-window  │      │   Phase 3          │
  │     .Daemon      │      │                    │      │  Uses juhradial-   │
  │   - spawns popup │      │                    │      │   window +         │
  │   - reads        │      │                    │      │   juhradial-       │
  │     ~/.config/   │      │                    │      │   widgets          │
  │     juhradial/   │      │                    │      │                    │
  │     config.json  │      │                    │      │                    │
  └──────────────────┘      └────────────────────┘      └────────────────────┘
       systemd-managed         session-launched              spawned on-demand

                                       ▲
                                       │ shared via path-deps in workspace
                                       │
            ┌──────────────────────────┼──────────────────────────┐
            │                          │                          │
            ▼                          ▼                          ▼
  ┌──────────────────┐      ┌────────────────────┐      ┌────────────────────┐
  │ juhradial-shared │      │ juhradial-widgets  │      │  juhradial-window  │
  │   AppConfig      │      │   widgets + style  │      │   frameless_top    │
  │   + PopupConfig  │      │   + palette        │      │     most() helper  │
  │   (Phase 0)      │      │   (Phase A)        │      │   + CursorHelper   │
  │                  │      │                    │      │     D-Bus proxy    │
  └──────────────────┘      └────────────────────┘      └────────────────────┘
```

### Two persistence layers (do not cross)

| Layer | Owns | Format | Location | Reader / writer |
|---|---|---|---|---|
| **GSettings** | Indicator look + behaviour (display-mode, thresholds, colors, panel target, position, click-behavior, refresh-interval) | gschema XML compiled to GVDB | `org.gnome.shell.extensions.juhradial-indicator.*` | Extension reads+writes via `Gio.Settings`. `popup-rs` reads via `gio` (gtk-rs) for color/threshold parity. |
| **JSON (config.json)** | Popup contents (mode, host buttons, quick toggles, sliders, volume-on-scroll, etc.) | serde_json on `juhradial_shared::AppConfig.popup` | `~/.config/juhradial/config.json` | `settings-rs` reads+writes (Indicator Popup tab). `popup-rs` reads only, watching with `notify`. |

The split is deliberate: thresholds + colors live alongside other GNOME look-and-feel knobs and survive a daemon reinstall; popup contents live with everything else the daemon owns and follow JuhRadial backup/export.

---

## 3. The three responsibilities in detail

### 3.1 Battery + device status surface

`PanelMenu.Button` housing an `St.BoxLayout` of: optional mouse glyph (`St.Icon`, symbolic, tinted by band when `tint-mouse-glyph`), optional 3-cell battery glyph, optional `XX%` label. Composition driven by `display-mode` (`percent | icon | both`) and `show-mouse-glyph`.

Band classification:
- `charging` if charging.
- `critical` if `pct ≤ threshold-critical` (default 15).
- `low` if `pct ≤ threshold-low` (default 30).
- `healthy` otherwise.

Colors: GSettings keys `color-critical / color-low / color-healthy / color-charging`. Live preview in prefs dialog re-renders on every `changed::*`.

Critical-band one-shot notification (confirmed UX, 2026-05-24):
- Per device, track an `_armed` boolean.
- On crossing into `critical && !charging` while `_armed`: emit `Gio.Notification` (urgency=critical, title `"MX device low"`, body `"<deviceName> at <pct>% — connect charging cable"`), set `_armed = false`.
- Re-arm when the band exits or charging starts.

### 3.2 Quick-action popup

Click dispatch is per `click-behavior` GSetting:
- `popup` → `org.juhradial.Daemon.ShowPopup(panel_x, panel_y, panel_w, panel_h)` — daemon spawns `juhradial-popup` subprocess with the indicator's stage-absolute panel rect on `argv`.
- `settings` → spawn `juhradial-settings`.
- `none` → no-op.

Right-click always opens a small `PopupMenu` with `Open Settings / Open Extension Preferences / About`.

Popup layout (mirrors `design/juhradial-indicator/popup.jsx`):
- Header: device pill + connection dot. Dropdown switches active device.
- Battery ring (88px svg-style circular progress, color-banded same as the indicator).
- Easy-switch host buttons (configurable: show/hide; label `hostname` vs `channel`).
- Quick toggles (re-orderable).
- Quick sliders (Power User mode only).

Popup window setup (via the new `juhradial-window` crate):
- `frameless_topmost(APP_ID, Size::new(360, 480))` — no decorations, transparent root, always on top, non-resizable, app_id `org.juhradial.popup`.
- After first frame, `move_overlay(APP_ID, anchor_x, anchor_y, -1)` against the cursor extension to dock under the indicator.
- Click-outside dismisses (transparent fullscreen `mouse_area` backdrop). `Esc` also dismisses.

### 3.3 Settings tab (`Tab::IndicatorPopup`)

Section order (canonical, from the design):
1. **Mode** — Simple / Power User two-button segmented.
2. **Easy-Switch host buttons** — show/hide toggle + `Hostname` vs `Channel` radio + live preview row.
3. **Quick toggles** — re-orderable list with up/down/trash on each row; "Available" rail below.
4. **Quick sliders** — same UX, Power User mode only.
5. **Interactions** — three toggles (`Adjust system volume on scroll while popup is focused`, `Close on action`, `Animations`).

Volume-on-scroll implementation (popup side): when the popup has focus and `popup.volume_on_scroll == true`, capture `Event::Mouse(WheelScrolled { delta })`, emit `Message::VolumeStep(sign)`, update arm spawns `Task::perform(wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%{+,-})`. Focus loss disables capture.

### 3.4 Stack supervisor + unified health surface

**Added 2026-05-24.** The indicator is the single user-visible surface that answers "is the JuhRadial stack working right now?"

Health probes (run on extension `enable()`, on every popup open, and on every `refresh-interval` tick):

| Probe | Check | Remediation surface |
|---|---|---|
| **Daemon process** | `systemctl --user is-active juhradialmx-daemon.service` returns "active" AND `org.juhradial.Daemon` is on the session bus. | Indicator turns red-orange (`is-critical` ring class), tooltip says "JuhRadial daemon is not running". Right-click menu adds `Start daemon` → `systemctl --user start juhradialmx-daemon.service`. Popup shows a footer "Daemon down" with one-click `Start` button. |
| **Daemon ↔ device link** | `GetActiveDeviceState` returns `connection != "off"` and a non-empty `deviceId`. | Indicator shows the `DisconnectedIndicator` lozenge (`— no device`). Popup hides quick toggles, shows "No MX device paired" with link to Devices tab in settings. |
| **Radial overlay process** | When the user toggles `Radial Overlay` to ON: D-Bus probe for `org.juhradial.overlay` name. If missing, spawn `juhradial-overlay-rs` via `Gio.Subprocess` (or call a new daemon method `EnsureOverlayRunning()` that wraps the spawn — preferred, since the daemon already owns the overlay-lifecycle decision). When toggled OFF: send `HideMenu` and leave the process (low cost). |
| **Gaming-mode bridges** | Gaming mode toggle ON: confirm daemon's `GetGamingState()` reports active and the gamepad-haptic bridge thread is running inside the daemon (already daemon-internal per `HAPTIC_GAMEPAD_BRIDGE_DESIGN.md` §2 — no separate process to supervise). |
| **Haptic feedback** | Toggle ON: daemon's `haptic_supported()` returns true and the HID++ haptic feature index is cached. No process supervision needed — daemon-internal. |
| **GNOME cursor helper extension** | `Main.extensionManager.lookup('juhradial-cursor@dev.juhlabs.com')` returns an enabled extension. Required by overlay positioning. | If missing, indicator surfaces a one-time warning in the popup footer. |

**Architectural rules:**
- The extension **never** spawns long-lived processes itself. It either calls `systemctl --user start <unit>` or asks the daemon to do the spawn via a new D-Bus method. This keeps the Shell process lean and the daemon's job tree consistent with what `journalctl --user` shows.
- The extension's supervisor checks are **read-only** by default; remediation is **user-confirmed** (a click on a "Start" / "Repair" button). The exception is auto-start of the overlay when the user explicitly toggles `Radial Overlay` ON — that toggle is itself the consent.
- "Health" state is observable on three surfaces with consistent semantics: the panel button color, the popup header line, and the right-click menu item labels.

**New D-Bus method introduced for this responsibility:**

```rust
/// Ensures the radial overlay process is running. Idempotent —
/// returns Ok(()) immediately if the overlay is already on the bus.
/// On miss, spawns `juhradial-overlay-rs` via tokio::process::Command
/// and returns Ok(()) once the process appears on the bus (timeout 3s)
/// or Err(...) on spawn failure.
async fn ensure_overlay_running(&self) -> fdo::Result<()>;
```

Implemented in `daemon/src/dbus/interface.rs`. Backed by a small `OverlaySpawner` struct in `daemon/src/overlay_spawner.rs` (new module) that holds the child handle so a future `StopOverlay` method can `kill()` it cleanly.

---

## 4. The D-Bus surface (full delta)

Existing methods/signals stay the same (modulo the bus-name rename in Phase 0). The new methods + signal added in Phase 0 / 1:

| Member | Direction | Signature | Owner | Caller |
|---|---|---|---|---|
| `GetActiveDeviceState` | method | `() → (yysss)` `(battery, charging, connection, name, id)` | daemon | indicator extension (poll fallback) |
| `ShowPopup` | method | `(iiii) → ()` `(panel_x, panel_y, panel_w, panel_h)` stage-absolute logical px | daemon (spawns `juhradial-popup`) | indicator extension on left-click |
| `EnsureOverlayRunning` | method | `() → ()` | daemon (spawns `juhradial-overlay-rs`) | indicator extension when radial toggle goes ON |
| `DeviceStateChanged` | signal | `(yysss)` | daemon (emitted from `battery.rs` on change) | indicator extension subscribes for push updates |

The cursor-helper extension's existing surface (`org.juhradial.CursorHelper.{GetCursorPosition, MoveOverlay, RaiseOverlay, ListMonitors, GetFocusedWindowClass}`) is unchanged but reused by the popup.

---

## 5. GSettings schema (full key list)

Path: `/org/gnome/shell/extensions/juhradial-indicator/`

| Key | Type | Default | Range | Purpose |
|---|---|---|---|---|
| `display-mode` | `s` | `'both'` | `percent`/`icon`/`both` | What's drawn in the panel button |
| `show-mouse-glyph` | `b` | `true` | — | Adds the mouse icon |
| `tint-mouse-glyph` | `b` | `true` | — | Mouse icon picks up band color |
| `threshold-critical` | `i` | `15` | `1–99` | Critical band upper bound |
| `threshold-low` | `i` | `30` | `1–99` | Low band upper bound |
| `color-critical` | `s` | `'#FF3E5A'` | hex | Critical band color |
| `color-low` | `s` | `'#F2C94C'` | hex | Low band color |
| `color-healthy` | `s` | `'#5BE095'` | hex | Healthy band color |
| `color-charging` | `s` | `'#5BE095'` | hex | Charging override color |
| `apply-color-to-text` | `b` | `true` | — | Tint percentage text too |
| `panel-target` | `s` | `'auto'` | `auto`/`topbar`/`dtp`/`both` | Where the indicator renders |
| `position` | `s` | `'right'` | `left`/`center`/`right` | Within the panel |
| `position-index` | `i` | `0` | — | Order within panel section |
| `click-behavior` | `s` | `'popup'` | `popup`/`settings`/`none` | Left-click dispatch |
| `refresh-interval` | `i` | `30` | `5–120` (s) | Battery poll fallback when no signal traffic |

---

## 6. `PopupConfig` (full schema)

Lives on `juhradial_shared::config::AppConfig` as `pub popup: PopupConfig`. Serializes to the `popup` key in `config.json`.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PopupConfig {
    pub mode: PopupMode,                   // simple | power
    pub show_host_buttons: bool,           // default true
    pub host_label_style: HostLabelStyle,  // hostname | channel
    pub simple_toggles: Vec<String>,       // default ["gaming","haptics","radial"]
    pub power_toggles:  Vec<String>,       // default ["gaming","haptics","radial","flow"]
    pub power_sliders:  Vec<String>,       // default ["dpi","scroll"]
    pub volume_on_scroll: bool,            // default true
    pub close_on_action:  bool,            // default false
    pub animations:       bool,            // default true
}
```

Catalogs (`QUICK_TOGGLE_CATALOG`, `QUICK_SLIDER_CATALOG`) are `&'static [QuickEntry]` constants in `juhradial-shared`. Every default id is asserted to live in the catalog by a unit test in Phase 0.

---

## 7. Workspace deltas

```
juhradial-mx/
├── juhradial-shared/                  [MODIFIED — add PopupConfig + catalogs]
├── juhradial-widgets/                 [NEW — Phase A]
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── widgets.rs                 [MOVED from settings-rs/src/widgets.rs]
│       ├── style.rs                   [MOVED from settings-rs/src/style.rs]
│       └── palette.rs                 [MOVED from settings-rs/src/palette.rs]
├── juhradial-window/                  [NEW — Phase A]
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── settings.rs                [frameless_topmost()]
│       └── cursor_helper.rs           [MOVED from overlay-rs/src/ext_positioner.rs]
├── popup-rs/                          [NEW — Phase 3]
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── app.rs
│       ├── config_watcher.rs
│       ├── gsettings_bridge.rs
│       ├── actions.rs                 [quick-toggle → daemon-method mapping]
│       └── view.rs
├── gnome-extension/
│   ├── tsconfig.json                  [NEW shared]
│   ├── package.json                   [NEW shared dev deps]
│   ├── juhradial-cursor@dev.juhlabs.com/
│   │   ├── metadata.json              [unchanged]
│   │   └── extension.ts               [Phase 1 — migrated from .js]
│   └── juhradial-indicator@dev.juhlabs.com/   [NEW]
│       ├── metadata.json
│       ├── extension.ts
│       ├── prefs.ts
│       ├── lib/
│       │   ├── settings.ts
│       │   ├── battery.ts
│       │   ├── placement.ts
│       │   ├── format.ts
│       │   └── supervisor.ts          [§3.4 — health probes + remediation]
│       ├── schemas/
│       │   └── org.gnome.shell.extensions.juhradial-indicator.gschema.xml
│       ├── stylesheet.css
│       └── icons/
│           ├── jr-mouse-symbolic.svg
│           ├── jr-mouse-low-symbolic.svg
│           ├── jr-mouse-critical-symbolic.svg
│           └── jr-mouse-charging-symbolic.svg
├── settings-rs/
│   └── src/
│       ├── main.rs                    [MODIFIED — Tab::IndicatorPopup]
│       └── tabs/
│           └── indicator_popup.rs     [NEW — Phase 2]
├── daemon/
│   └── src/
│       ├── dbus/
│       │   ├── mod.rs                 [MODIFIED — bus-name rename]
│       │   └── interface.rs           [MODIFIED — +GetActiveDeviceState, +ShowPopup, +EnsureOverlayRunning, +DeviceStateChanged]
│       ├── battery.rs                 [MODIFIED — emit DeviceStateChanged]
│       └── overlay_spawner.rs         [NEW — §3.4 EnsureOverlayRunning backing]
└── overlay-rs/
    └── src/                           [MODIFIED — import juhradial-window]
```

---

## 8. Validation matrix

Per-phase acceptance: see [implementation plan](docs/plans/indicator-implementation.md#acceptance-walkthrough--how-the-reviewer-verifies). Headline visual / functional checks:

1. Top-bar indicator at 30% battery shows yellow glyph + "30%" — matches `design/juhradial-indicator/index.html#indicator-states`.
2. Clicking opens the daemon-spawned popup (not a GJS PopupMenu).
3. Prefs window matches `ext-prefs-window` artboard.
4. Dragging threshold handle updates GSettings live, preview pill recolors.
5. "Reset to defaults" restores all keys to schema defaults.
6. Sidebar in `juhradial-settings`: "Indicator Popup" appears between "Point & Scroll" and "Haptic Feedback".
7. Switching Simple ↔ Power User reveals/hides "Quick sliders".
8. Quick-toggle rows reorder via up/down, persist across restart in `config.json`.
9. Volume-on-scroll scrolls `wpctl get-volume @DEFAULT_AUDIO_SINK@` while popup is focused.
10. **§3.4 supervisor:** Stopping `systemctl --user stop juhradialmx-daemon.service` turns the indicator red within `refresh-interval` seconds and surfaces a `Start daemon` action in the right-click menu and popup footer.

---

## 9. What carries over from prior design docs

- The daemon's HID++ + battery + haptic stack ([daemon/src/](daemon/src/)) — unchanged except for the bus-name rename + three new D-Bus members + one new module.
- The overlay's iced + canvas rendering — unchanged; only the window-setup + cursor-helper-client lines move into the new `juhradial-window` crate.
- The settings app's tab + theming architecture — unchanged; tabs continue to live under `settings-rs/src/tabs/`.
- Persistence layout — unchanged; `popup` is a new field on the existing root config struct.

## 10. What changes from prior design docs

- D-Bus name `org.kde.juhradialmx` is gone; long-tail of references touched in Phase 0.
- `settings-rs` no longer owns `widgets.rs / style.rs / palette.rs` — they migrate to `juhradial-widgets`. `settings-rs` keeps a thin `pub use` façade for back-compat.
- `overlay-rs` no longer owns `ext_positioner.rs` — it migrates to `juhradial-window`.
- `juhradial-cursor` GNOME extension changes from plain JS to TypeScript; runtime behaviour unchanged.

---

## See also

- Implementation plan: [`docs/plans/indicator-implementation.md`](docs/plans/indicator-implementation.md)
- Original UI spec (preserved as historical record): [`design/juhradial-indicator/CLAUDE_CODE_PROMPT.md`](design/juhradial-indicator/CLAUDE_CODE_PROMPT.md)
- Spec deltas (what changed between the prompt and this design): [`design/juhradial-indicator/SPEC_ADDENDUM.md`](design/juhradial-indicator/SPEC_ADDENDUM.md)
- Sibling design docs: [`RUST_GTK4_OVERLAY_DESIGN.md`](RUST_GTK4_OVERLAY_DESIGN.md), [`HAPTIC_GAMEPAD_BRIDGE_DESIGN.md`](HAPTIC_GAMEPAD_BRIDGE_DESIGN.md)
- Visual canvas: `python3 -m http.server -d design/juhradial-indicator 7000` → `localhost:7000`
