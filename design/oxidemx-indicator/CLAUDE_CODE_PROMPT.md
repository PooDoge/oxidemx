# Claude Code prompt — Implement OxideMX MX indicator + popup + settings

> Paste the contents of this file as the **opening message** to Claude Code in the OxideMX repository. It is structured as a directive role + scoped plan request + acceptance criteria, not as a code dump. **Do not delete sections** — each one is load-bearing for getting the plan right.

---

## ROLE

You are a senior platform engineer pair-programming with the project owner. You write:

- Idiomatic, strict-typed **GJS/TypeScript** for GNOME 45+ shell extensions (the only language Shell extensions support)
- Idiomatic, `clippy`-clean **Rust** with the [**Iced**](https://iced.rs) GUI library for the OxideMX Settings app, Overlay app, and daemon-rendered popup (everything that isn't the Shell extension)

You favor:

- Small, composable modules over god-files
- GSettings schemas as the single source of truth for **extension** preferences
- TOML/JSON config files (via `serde`) under `~/.config/oxidemx/` as the single source of truth for **app** preferences
- D-Bus over ad-hoc IPC for daemon ↔ extension communication (`zbus` on the Rust side, `Gio.DBus*` on the GJS side)
- Iced's `Task` / `Subscription` model over thread spawning
- libadwaita conventions in the GJS prefs dialog (`Adw.PreferencesPage`, `Adw.PreferencesGroup`, `Adw.ActionRow`)
- Visual fidelity to the provided designs — match spacing, radii, accent rules exactly

You **never start writing code before producing a written plan** the human approves. You **ask focused clarifying questions** when a design implication is ambiguous, but you do not re-litigate decisions that are already settled in the design artifacts.

---

## CONTEXT — what this change is

OxideMX is a Logitech MX-family configuration utility for Linux:

- **OxideMX daemon** (long-running, **Rust**) — talks to mice over HID/Bluetooth via `hidapi` / `bluer`, exposes state on D-Bus via `zbus`. Owns the in-process Iced popup window too.
- **OxideMX MX Settings** (existing desktop app, **Rust + Iced**, dark themed, orange/cyan accent) — lives at `settings-rs/` in this repo. Full configuration UI.
- **OxideMX Radial Menu Overlay** (separate overlay app, **Rust + Iced**) — lives at `overlay-rs/`. The on-screen radial.
- **OxideMX GNOME Indicator** (this PR's new shell extension, **GJS/TypeScript**) — small status icon in the top panel. Required to be GJS because GNOME Shell extensions cannot be written in any other language.

This work delivers three coupled deliverables:

1. **A GNOME Shell extension** (`gnome-shell-extension-oxidemx-indicator/`, **GJS/TypeScript**) that puts a battery/mouse icon in the top bar. Click behavior opens the OxideMX daemon's popup — the popup itself is NOT rendered by GJS.
2. **An extension preferences dialog** (the cog in GNOME Extensions app, **GJS/TypeScript + libadwaita**) — single-page libadwaita prefs, **icon-only options** because of GJS popup-styling limitations.
3. **A new "Indicator Popup" tab inside `settings-rs/`** (the existing OxideMX MX Settings app, **Rust + Iced**) — sits in the sidebar **directly under "Point & Scroll"** — owns every preference for what the popup displays.
4. **A new daemon-side popup window** (**Rust + Iced**, lives next to or inside `settings-rs/` and shares its widget library) — the actual visual popup the indicator click opens. Reads the same config the "Indicator Popup" tab writes.

> **Critical architectural rule:** The popup is rendered by the OxideMX daemon (a small Iced window opened on demand) so its theming, accent colors, and quick-settings catalog stay consistent with the rest of OxideMX — because it literally **shares Rust + Iced widget code with `settings-rs/`**. The GNOME extension only renders the panel indicator icons and proxies clicks over D-Bus.
>
> Two persistence layers, do not cross them:
> - **Extension prefs** (icon display, threshold colors, panel placement) live in **GSettings** under `org.gnome.shell.extensions.oxidemx-indicator.*`. The Rust daemon reads these via `gio::Settings` (gtk-rs) so the popup's battery-ring color matches the panel icon color exactly.
> - **Popup prefs** (mode, quick toggles, sliders, host labels, volume-on-scroll, etc.) live in `~/.config/oxidemx/config.toml` (or your existing serde format), read+written by `settings-rs/` and read by the daemon popup.

---

## DESIGN ARTIFACTS — read these first

All design artifacts live in `design/oxidemx-indicator/` (copied from the visual review project). Open each before planning:

| File | What it shows |
|---|---|
| `design/oxidemx-indicator/index.html` | Master canvas — open in a browser. Sections include indicator-in-context, popup variants, indicator state strip, Devices page, **new "Indicator Popup" settings tab (Simple + Power User)**, and **GNOME extension prefs dialog**. |
| `design/oxidemx-indicator/styles.css` | Authoritative token + component CSS. Variables prefixed `--jr-*` are the design system; mirror them in your GTK CSS / `.css` files. |
| `design/oxidemx-indicator/popup.jsx` | Reference layout for the popup. Spacing, padding, easy-switch segment, quick-toggle list, footer button order. |
| `design/oxidemx-indicator/indicator-popup-page.jsx` | The "Indicator Popup" settings tab. Section order is canonical: Mode → Easy-Switch → Quick toggles → Quick sliders (power only) → Interactions. |
| `design/oxidemx-indicator/ext-prefs.jsx` | The libadwaita extension prefs. Section order: Preview → Display → Battery level colors → Placement → Behavior → About → Reset. |
| `design/oxidemx-indicator/devices-page.jsx` | Existing Devices page in the settings app — for nav reference. The new "Indicator Popup" item goes **between "Point & Scroll" and "Haptic Feedback"** in `APP_NAV`. |
| `design/oxidemx-indicator/app-shell.jsx` | Shell chrome (header, sidebar, status bar) reused across every settings page. |
| `design/oxidemx-indicator/indicator.jsx` | Top-bar indicator placement + battery glyph rules. |

> When in doubt, the rendered HTML in `index.html` wins. Run `python3 -m http.server -d design/oxidemx-indicator 7000` and visit `localhost:7000` to see the source-of-truth.

### Design tokens you MUST inherit

Pull these directly from `styles.css` and re-express them in your real stylesheet (`stylesheet.css` for the extension, GTK CSS for the daemon popup):

```
--jr-bg-app:      #0C0D11
--jr-surface-1:   #12141A
--jr-surface-2:   #181B22
--jr-surface-3:   #1F232C
--jr-fg:          #E7E9EE
--jr-fg-muted:    #8B919C
--jr-orange:      #EE7E1A    /* default accent */
--jr-cyan:        #4FD0CC    /* alt accent */
--jr-yellow:      #F2C94C    /* low battery default */
--jr-red:         #FF3E5A    /* critical battery default */
--jr-green:       #5BE095    /* charging / healthy battery default */
```

Battery threshold defaults: **critical ≤ 15%, low ≤ 30%, healthy > 30%**. Thresholds AND colors are user-editable (sliders on a band visualization + color swatches).

---

## SCOPE — what to build

### Part 1 · GNOME Shell extension (new repo or new top-level dir)

```
gnome-shell-extension-oxidemx-indicator/
├── metadata.json               # uuid: oxidemx-indicator@<owner>
├── extension.ts                # Indicator class, panel-button lifecycle
├── prefs.ts                    # Adw.PreferencesWindow — single page, sections stacked
├── lib/
│   ├── settings.ts             # GSettings wrapper, typed getters/setters
│   ├── battery.ts              # D-Bus client to OxideMX daemon (org.oxidemx.Daemon)
│   ├── placement.ts            # Top-bar vs Dash-to-Panel auto-detect
│   └── format.ts               # Battery-band classification, color resolution
├── schemas/
│   └── org.gnome.shell.extensions.oxidemx-indicator.gschema.xml
├── stylesheet.css              # Indicator-only styling
└── icons/
    ├── jr-mouse-symbolic.svg
    ├── jr-mouse-low-symbolic.svg
    ├── jr-mouse-critical-symbolic.svg
    └── jr-mouse-charging-symbolic.svg
```

#### Extension behavior
- **PanelMenu.Button** with an `St.BoxLayout` of: optional mouse glyph (St.Icon, symbolic, optionally tinted) + optional battery glyph + optional percentage `St.Label`. Layout driven by `display-mode` (`percent`|`icon`|`both`) and `show-mouse-glyph`.
- **Click** does NOT open a `PopupMenu` — instead it dispatches per `click-behavior` GSetting: `popup` calls `org.oxidemx.Daemon.ShowPopup()` on D-Bus; `settings` spawns `oxidemx-settings`; `none` is a no-op. Right-click always opens a minimal menu with "Open Settings", "Open Extension Preferences", "About".
- **Battery state** comes from `org.oxidemx.Daemon.GetActiveDeviceState()` (returns `{ battery: int, charging: bool, connection: string, deviceName: string, deviceId: string }`). Poll every `refresh-interval` seconds AND subscribe to the `DeviceStateChanged` signal for instant updates.
- **Placement** — read `panel-target`: `auto` (probe Dash to Panel's DBus name; if present, use DTP's `getStatusAreaSection` API, else top bar), `topbar`, `dtp`, `both`. Honor `position` (`left`|`center`|`right`) and `position-index`.
- **No popup rendering inside GJS.** The popup view is owned by the daemon for theme consistency and to bypass GJS popup limitations.

#### GSettings keys (full list — these are the prefs dialog's surface area)

```xml
<key name="display-mode"        type="s" default="'both'"/>     <!-- percent | icon | both -->
<key name="show-mouse-glyph"    type="b" default="true"/>
<key name="tint-mouse-glyph"    type="b" default="true"/>
<key name="threshold-critical"  type="i" default="15"/>
<key name="threshold-low"       type="i" default="30"/>
<key name="color-critical"      type="s" default="'#FF3E5A'"/>
<key name="color-low"           type="s" default="'#F2C94C'"/>
<key name="color-healthy"       type="s" default="'#5BE095'"/>
<key name="color-charging"      type="s" default="'#5BE095'"/>
<key name="apply-color-to-text" type="b" default="true"/>
<key name="panel-target"        type="s" default="'auto'"/>     <!-- auto | topbar | dtp | both -->
<key name="position"            type="s" default="'right'"/>    <!-- left | center | right -->
<key name="position-index"      type="i" default="0"/>
<key name="click-behavior"      type="s" default="'popup'"/>    <!-- popup | settings | none -->
<key name="refresh-interval"    type="i" default="30"/>          <!-- seconds, 5-120 -->
```

#### Prefs dialog (`prefs.ts`)
- One `Adw.PreferencesPage` with **stacked** `Adw.PreferencesGroup`s — NO sidebar (single-page-stacked layout per design).
- Groups in order: **Preview**, **Display**, **Battery level colors**, **Placement**, **Behavior**, **About**, footer **Reset to defaults**.
- "Preview" group is a non-interactive row that mirrors `ext-prefs.jsx`'s preview pill — re-render whenever any setting changes.
- "Battery level colors" group: stacked layout with a 3-band horizontal bar (custom drawing on a `Gtk.DrawingArea`) and three `Adw.ActionRow`s with `Gtk.ColorButton` and editable threshold spin buttons.
- "About" group: version, GNOME version compat, license, "Open daemon", "Report issue".

### Part 2 · Settings app changes (existing OxideMX MX Settings repo / dir)

```
oxidemx-settings/
└── src/
    ├── nav.ts                  # add "Indicator Popup" entry between "point" and "haptic"
    ├── pages/
    │   └── indicator-popup.tsx # NEW — the entire new tab
    └── state/
        └── popup-config.ts     # config schema + persistence
```

#### `nav.ts` — exact placement

```ts
// Existing order with new item:
{ id: 'buttons',   label: 'Mouse Buttons',   icon: 'mouse' },
{ id: 'menu',      label: 'Menu',            icon: 'palette' },
{ id: 'point',     label: 'Point & Scroll',  icon: 'point' },
{ id: 'indicator', label: 'Indicator Popup', icon: 'pulse' },   // ← NEW, exactly here
{ id: 'haptic',    label: 'Haptic Feedback', icon: 'haptic' },
{ id: 'devices',   label: 'Devices',         icon: 'monitor' },
// …rest unchanged
```

#### `indicator-popup.tsx` — sections in order

1. **Mode** — two-button segmented (Simple / Power User). Default: `simple`. Selection persists to `config.popup.mode`.
2. **Easy-Switch host buttons** — card containing:
   - Toggle: "Show host buttons" (`config.popup.showHostButtons`)
   - Radio: "Label style" — `hostname` (default, falls back to `Channel N`) vs `channel` (always numeric)
   - Live preview segment using the active device's `hosts: [{ name }]` array
3. **Quick toggles** — reorderable list with **up/down arrow buttons** on each row (GNOME-accessible reorder, NO drag gesture). "Available" rail below for unused items. Catalog:
   - `gaming` Gaming Mode · `haptics` Haptic Feedback · `radial` Radial Overlay (defaults in this order)
   - `flow` Flow · `smart` SmartShift · `highlight` Cursor highlight
   - Each row has icon, label, sub-text, kind pill ("TOGGLE"), and trash button.
4. **Quick sliders** — **only visible in Power User mode**. Same reorder UX. Catalog:
   - `dpi` Pointer DPI (200–6,400) · `scroll` Scroll sensitivity (1–10)
   - `haptic_i` Haptic intensity · `accel` Pointer acceleration (-1.0–1.0)
   - Defaults active: `dpi`, `scroll`.
5. **Interactions** — three toggles:
   - **"Adjust system volume on scroll while popup is focused"** (default ON) — implementation: while popup is `:focus`, capture scroll events; emit `Gio.Subprocess` `wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%±` (or PulseAudio/PipeWire equivalent). Release on focus loss / click-outside.
   - "Close on action" (default OFF)
   - "Animations" (default ON)

> Use up/down arrow buttons for reorder. Do NOT implement drag-to-reorder gestures; the chosen approach is keyboard-accessible and matches GNOME conventions.

### Part 3 · Daemon popup renderer (Rust + Iced) — mirrors `overlay-rs/`

The popup is a small Iced window the daemon shows on demand when the indicator dispatches `org.oxidemx.Daemon.ShowPopup(panel_rect, indicator_rect)`. It is **not** a GJS PopupMenu and it must **not** be styled by the window manager.

#### Window setup — mirror `overlay-rs/` exactly

The radial overlay already solves the "frameless, transparent, on-top, compositor-positioned, click-outside-to-dismiss" problem. The popup uses the **same** approach so we get rounded corners and full skinning for free and so the two windows don't drift apart visually. Before writing any window code, **read `overlay-rs/src/window.rs` (or the equivalent module)** and copy the pattern:

- **No OS titlebar / decorations.** `decorations: false` in `iced::window::Settings`, or the layer-shell equivalent. Rounded corners are drawn by the outermost `container`'s `border_radius`, not by the WM.
- **Transparent root window** (`transparent: true`) so the rounded-corner cutout reveals the desktop underneath at the corners.
- **Always on top** (`level: AlwaysOnTop`), **non-resizable** (`resizable: false`), skip taskbar.
- **Wayland: layer-shell** — if `overlay-rs/` uses `iced_layershell` (or `smithay-client-toolkit` directly), use the **same crate and same anchor conventions**. Anchor to the panel edge (`Anchor::TOP | Anchor::RIGHT` typically), with margins derived from `indicator_rect` so the popup tip lines up with the indicator center.
- **X11 fallback**: regular Iced `Application` with the same flags + manual position calculation from `panel_rect`/`indicator_rect`.
- **Factor out shared window-shell code** — if `overlay-rs/src/window.rs` isn't already a reusable module, extract it into a sibling crate (`oxidemx-window`) as part of this PR so the radial and the popup share one source of truth for window setup. Do **not** copy-paste.
- **Outer container styling**: 14px corner radius (matches the `.jr-popup` token in the design CSS), 1px hairline border at `JR_FG @ 12% alpha`, soft drop shadow drawn as a slightly-larger semi-transparent container behind the popup if the compositor doesn't render WM shadows for undecorated windows.
- **Dismissal**: click-outside closes the popup. On layer-shell use `KeyboardInteractivity::OnDemand` + focus-loss listener; on X11 wrap the popup in a fullscreen transparent `mouse_area` backdrop. `Esc` also closes.

#### Config + state

- **Reuse the widget library from `settings-rs/`**: extract `widgets/` into a sibling crate (`oxidemx-widgets`) and import it from `settings-rs/`, `overlay-rs/`, and the new popup. This is the architectural reason for keeping the popup in Rust.
- **Config loading**: on show, deserialize `~/.config/oxidemx/config.toml`'s `[popup]` table into `PopupConfig`. Watch the file with `notify` (the crate) and reload on change so settings tweaks reflect without restarting the daemon.
- **GSettings consumption** (battery thresholds + colors): use `gio` via `gtk-rs`. Subscribe to `changed::*` on `org.gnome.shell.extensions.oxidemx-indicator` and re-render. The battery ring color band is computed using the **same** thresholds the extension uses for the panel icon — they must never disagree.
- **Volume-on-scroll**: when the popup has focus and `popup.volume_on_scroll == true`, register a `mouse_area` on the root container, consume `Event::Mouse(mouse::Event::WheelScrolled { delta })`, emit `Message::VolumeStep(±5)`. The update arm spawns `Task::perform(async { … })` running `wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%{+,-}` via `tokio::process::Command`. Use a focus-tracking subscription so consumption only fires while focused.
- **Easy-Switch hostnames**: read from the daemon's own state cache (`Device::hosts: [HostInfo { name: Option<String> }]`).

---

## NON-GOALS — do not do these

- ❌ Do not implement the popup itself inside GJS. The popup is daemon-owned.
- ❌ Do not add pairing UX changes — the Devices page already exists and is out of scope.
- ❌ Do not add drag-to-reorder gestures. Up/down arrows only.
- ❌ Do not add color-picker logic beyond `Gtk.ColorButton` / `Gtk.ColorChooserDialog` for the threshold colors.
- ❌ Do not redesign the existing Devices page. Just add the nav item.
- ❌ Do not invent new battery bands. Three bands (critical / low / healthy) with charging override is the contract.
- ❌ Do not let the WM/compositor draw the popup's titlebar or frame. The popup is **undecorated** and skinned by Iced, identically to the radial overlay.
- ❌ Do not fork the radial overlay's window-setup code. Share it via a `oxidemx-window` crate.

---

## PLAN-FIRST WORKFLOW

Before writing any code:

1. **Produce a written implementation plan** with: file tree diff per part, ordered task list, D-Bus method/signal signatures, GSettings schema XML diff, list of new dependencies (should be ~zero). Save it as `docs/plans/indicator-implementation.md`.
2. **Surface ambiguities as a numbered question list** at the end of the plan. Wait for answers before proceeding past Part 1.
3. **Implement in this order**, opening a PR per part:
   - Part 1a: GSettings schema + `settings.ts` wrapper
   - Part 1b: `extension.ts` + `placement.ts` + `battery.ts` (D-Bus client with mock fallback so the extension is testable without the daemon)
   - Part 1c: `prefs.ts` — full UI with live preview
   - Part 1d: `stylesheet.css` + symbolic icons
   - Part 2a (Rust): `config/popup.rs` — serde struct + load/save + tests against fixture files
   - Part 2b (Rust): `nav.rs` change + empty page stub registered in `pages/mod.rs`
   - Part 2c (Rust): `pages/indicator_popup.rs` — full Iced view + update implementation
   - Part 2d (Rust): missing widget primitives (only what's not already in `widgets/`) and any `theme.rs` additions
   - Part 3 (Rust): daemon popup window + GSettings subscription + volume-on-scroll task
4. **For each PR**, include:
   - Screenshots matching the corresponding artboard in `design/oxidemx-indicator/index.html`
   - Unit tests:
     - GJS side: `format.ts` (band classification), `placement.ts` (DTP detection), `settings.ts` (schema round-trip)
     - Rust side: `config::popup` serde round-trip, reorder operations (move up/down/swap on edges), default catalog integrity (every default ID exists in the catalog)
   - `cargo clippy --all-targets -- -D warnings` clean for any Rust touched
   - `cargo fmt --check` clean
   - Manual-test recipe in the PR description

---

## ACCEPTANCE CRITERIA

A reviewer should be able to confirm visually that:

- Top-bar indicator at 30% battery shows yellow glyph + "30%" text, matching `indicator-states` artboard.
- Clicking the indicator opens the daemon popup (not a GJS PopupMenu).
- Extension prefs window opens as a libadwaita single-page dialog matching the `ext-prefs-window` artboard.
- Dragging the threshold handles updates the GSettings keys in real time AND the preview pill recolors live.
- "Reset to defaults" restores all listed GSettings keys to schema defaults.
- The new "Indicator Popup" sidebar item appears between "Point & Scroll" and "Haptic Feedback" in the settings app.
- Switching Simple ↔ Power User in the settings tab reveals/hides the "Quick sliders" section AND changes which quick-toggle defaults are active.
- Quick-toggle rows can be reordered with up/down arrows, removed with the trash button, and added back from the Available rail. Order persists across app restart.
- Easy-Switch host buttons render hostnames when `hostLabelStyle === 'hostname'` AND `device.hosts[i].name` is non-null; otherwise fall back to "Channel N".
- With "Volume on scroll" enabled, scrolling the mouse wheel while the popup has focus moves `wpctl` (or PA) volume in 5% steps.
- All text remains AA-contrast on the chosen background per accent (orange and cyan both).

---

## CLARIFYING QUESTIONS YOU MAY ASK

When you produce the plan, ask about anything still ambiguous. Common ones:
- What exact Iced version does `settings-rs/Cargo.toml` pin? (The plan must be written against that version's API surface, not the latest.)
- Is the popup window an `iced_layershell` Wayland layer-shell, or a regular Iced `Application` positioned manually?
- Are existing widget primitives (`SegmentedRadio`, `SwitchRow`, etc.) already in `settings-rs/src/widgets/`, or do I need to add them?
- Should the extension's "Critical battery" band emit a `Notify` once when first crossed, or never (UX decision)?
- Is there an existing D-Bus interface document for the daemon, or should I propose the IDL (zbus `#[interface]` on the Rust side, matching `Gio.DBusProxy` consumer on the GJS side)?
- Do you want the daemon popup invoked via D-Bus method (sync), via a `oxidemx-popup` subprocess (async, crash-isolated), or via a long-lived `org.oxidemx.Daemon.ShowPopup()` IPC into the already-running daemon?
- For Dash to Panel detection — is probing `org.gnome.Shell.Extensions.dash-to-panel` acceptable, or should I use the older settings-schema probe?
- Is the "Indicator Popup" config a new top-level `[popup]` table in `config.toml`, or nested under an existing key like `[ui.popup]`?
- Should the Rust widget library be extracted into a sibling crate (`oxidemx-widgets`) right now so the daemon popup can import it, or is that out of scope for this PR?

Do **not** ask about color values, section order, default toggle set, or wording — those are settled in the design artifacts.

---

## STYLE & ENGINEERING NOTES

### GJS / TypeScript (extension)
- `strict: true`, no `any`, return types on public functions.
- ESM `import` + `.js` suffix for GJS imports (per GNOME Shell convention).
- Prefer composition over inheritance for `St` widgets.
- Wrap every D-Bus / async call in cancellable `Gio.Cancellable` patterns and tear down on `disable()`.
- All user-facing strings go through `gettext` (`_()`), domain matches `metadata.json`.
- Log via `console.log('[oxidemx]', …)` — never `print`.

### Rust / Iced (settings app + daemon popup)
- Edition 2021 or 2024 (match what `settings-rs/Cargo.toml` already declares — do not bump).
- `clippy::pedantic` warnings worth fixing case-by-case; `clippy::all` errors must be zero.
- No `unwrap()` outside tests; use `?` with `anyhow::Result` at app boundaries, `thiserror`-derived errors inside library modules.
- `Message` enums implement `Debug + Clone`. Do not derive `Copy` for messages that carry `String`.
- View functions take `&self` and return `Element<'_, Message>`; never clone the whole state into a view.
- Async work is `Task::perform(async move { … }, Message::…)` — do not call `tokio::spawn` directly inside `update`.
- File I/O through `tokio::fs` (or `std::fs` when explicitly synchronous, like config load on startup).
- All user-facing strings live in `i18n.rs` / `fluent` if the repo already uses it; otherwise inline `&'static str` consts at module top.

---

## FIRST RESPONSE FORMAT

Reply with:

```
## Read so far
- list every design artifact you opened, one bullet each
- list every existing Rust file you read in settings-rs/ and overlay-rs/, with the line range scanned

## Iced version + existing widgets discovered
- Iced version pinned in settings-rs/Cargo.toml: X.Y.Z
- Existing widgets I can reuse: [list]
- Existing theme constants I can reuse: [list]
- Existing config persistence module: path + brief description

## Plan
- the written plan, structured as the "PLAN-FIRST WORKFLOW" section requires

## Ambiguities
1. Question…
2. Question…
```

Do not write any source code in your first response. Wait for the plan to be approved.
