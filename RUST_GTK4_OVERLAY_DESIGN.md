# Rust overlay rewrite — design doc

Status: in progress. Toolkit pivot landed (gtk4 → iced).
Branch: `rust-gtk4-overlay` (off master). The branch name still says
"gtk4" for git history continuity; the actual implementation no
longer uses GTK at all.

## 2026-05-02 toolkit pivot — gtk4 to iced

After the dev box's first attempt to layer the gtk4 *-devel packages
broke the Bazzite install (forced reinstall), we re-validated the
core assumption: gtk4-layer-shell relies on the wlr-layer-shell
Wayland protocol, and **Mutter (stable GNOME) doesn't advertise it**
— confirmed via `wayland-info` from inside a Fedora distrobox
talking to the host compositor. So the original "layer-shell makes
positioning trivial" pitch was a fiction on GNOME from day one;
the visual smoke test would have failed at `LayerShell::init_for_window()`.

The pivot:

* Drop gtk4-rs + gtk4-layer-shell + cairo-rs + gdk-pixbuf + pango.
* Adopt **iced 0.14** (MIT, pure Rust, Canvas widget, winit-backed).
* Solve positioning by extending the existing `oxidemx-cursor`
  GNOME shell extension with a `MoveOverlay(app_id, x, y, monitor)`
  D-Bus method. The extension runs *inside* Mutter and can call
  `Meta.Window.move_frame()` directly, bypassing the protocol-level
  positioning restriction on regular xdg-shell clients.
* Build entirely inside a Fedora distrobox — no host-side
  rpm-ostree layering needed. The container holds rust + cargo +
  the small set of devel packages that iced depends on
  (libxkbcommon, expat, fontconfig, freetype, libxcb,
  vulkan-loader). Rebooting / breaking the host is impossible.

Validated by `spike-iced/` (a throwaway crate at the workspace
root) which renders the radial wheel using
`iced::widget::canvas::Path` primitives, opens a transparent
decorationless always-on-top window via the iced Application
builder's first-class `.transparent()` / `.decorations()` /
`.level()` methods, and shows the desktop through the gaps
between/around wedges. Spike will be deleted once overlay-rs is
fully ported.

What carries over unchanged from the gtk4 design:

* The `oxidemx-shared` crate (config, themes, profiles,
  conditions, app launcher) is UI-toolkit-agnostic.
* Slice math (geometry, hit-test, easing curves) is unchanged —
  cairo's `move_to` / `line_to` / `arc` map line-by-line to
  iced's `Path::new` builder + `Arc` struct.
* The daemon's D-Bus contract is unchanged.
* The configuration schema is unchanged.

What changes from the gtk4 design:

* Module `window.rs` is gone; the iced `Application` builder owns
  windowing.
* Module `ext_positioner.rs` is new — async client for the
  GNOME extension's `MoveOverlay`.
* `dbus.rs` exposes a `Stream<OverlayEvent>` for
  `iced::Subscription::run` instead of a glib-MainContext task.
* `render/slices.rs` rebuilt against iced::widget::canvas::Frame
  rather than cairo::Context.
* `tray.rs` will be KStatusNotifierItem via raw zbus rather than
  a libgtk-supplied SNI client.
* The editor window (`editor/*`) targets iced widgets directly
  instead of GTK ApplicationWindow + DrawingArea.

The rest of this doc keeps the original layer-shell-era prose for
historical context but with `gtk4` substituted for `iced` wherever
implementation details appear.

---


## Goals

1. Replace the PyQt6 overlay with a Rust crate using GTK4 + `gtk4-layer-shell`.
2. Preserve the existing UI look and the bundled theme system (Dracula, Nord, etc.).
3. Same daemon, same D-Bus interface — no daemon changes (initially).
4. Add UI-level features users asked for: submenu editor, better custom icon support.
5. Drop juhflow entirely (out of scope for the rewrite).

## Why GTK4 + layer-shell, not Qt or alternatives

- **Layer-shell solves the core positioning problem at the protocol level.** The
  Wayland `wlr-layer-shell` protocol (which Mutter implements via
  gnome-shell-extensions for layer-shell on GNOME, and natively on every other
  Wayland compositor) lets an app explicitly anchor a surface to a specific
  output with explicit margin offsets. No xcb, no xdotool, no `dpr * cursor`,
  no "Mutter migrated my window between screens." The compositor positions the
  surface for us.
- GTK4 is already a dependency in the daemon's package list (`gtk4`,
  `gtk4-layer-shell`, `libadwaita`), so there's no new system requirement.
- Rust + GTK4 (`gtk4-rs`) is mature, has good async/glib integration, and lets
  us share types with the daemon via a small `oxidemx-shared` crate.
- Cairo (which GTK4 uses) is a great fit for the existing hand-drawn radial
  style — most of `overlay_painting.py` translates more or less line-by-line.

Alternatives considered and rejected:

- **Qt + cxx-qt**: keeps the coordinate-space stack we just unwound. Doesn't
  pay for itself.
- **iced / slint**: less mature layer-shell support, less polished for
  transparent always-on-top overlays with custom drawing.
- **Hand-rolled wgpu/skia + smithay-client-toolkit**: too much undertaking for
  too little gain.

## Architecture

```
oxidemx/
├── daemon/                       # unchanged
├── overlay/                      # unchanged (kept temporarily for parity)
├── overlay-rs/                   # NEW — the Rust overlay
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs               # entry, glib mainloop wiring
│   │   ├── app.rs                # GtkApplication setup, lifecycle
│   │   ├── window.rs             # layer-shell window construction
│   │   ├── radial.rs             # GtkDrawingArea radial widget
│   │   ├── render/
│   │   │   ├── mod.rs
│   │   │   ├── slices.rs         # slice geometry, hit-testing
│   │   │   ├── icons.rs          # icon resolution (theme + custom svg/png)
│   │   │   └── animation.rs      # easing curves, frame ticker
│   │   ├── theme.rs              # bundled themes, hex → cairo color
│   │   ├── actions.rs            # action loading, slice tuples
│   │   ├── config.rs             # config.json read/write (shared with daemon)
│   │   ├── dbus.rs               # MenuRequested / HideMenu signal listener
│   │   ├── input.rs              # cursor delta from daemon, click handling
│   │   ├── tray.rs               # status icon (settings, exit, …)
│   │   └── editor/               # NEW — submenu / slice editor
│   │       ├── mod.rs
│   │       ├── window.rs         # editor main window
│   │       ├── slice_panel.rs    # per-slice editing UI
│   │       ├── icon_picker.rs    # browse system theme + custom files
│   │       └── preview.rs        # live preview of edited menu
│   └── tests/
│       └── ...
└── oxidemx-shared/             # NEW — types shared between daemon + overlay
    ├── Cargo.toml
    └── src/
        ├── config.rs             # serde structs for config.json
        ├── theme.rs              # theme name → palette mapping
        └── action.rs             # ActionType enum, etc.
```

### Process model

Two processes still — daemon and overlay — same as today. Communication via
the existing D-Bus interface so we can roll out the overlay incrementally
(keep the Python overlay as a fallback while we stabilise the Rust one).
A future iteration can fold them into one process, but not for the rewrite.

### Daemon → overlay D-Bus

Unchanged shape. The daemon emits `MenuRequested(x, y)` and `HideMenu()` with
`(double, double)` cursor coords in Mutter logical pixels. The new overlay
subscribes to these via `zbus`.

> **Naming note (2026-05-24):** the service formerly known as
> `org.kde.oxidemx` is being renamed to `org.oxidemx.Daemon` in Phase 0
> of the indicator work. See [`INDICATOR_DESIGN.md`](INDICATOR_DESIGN.md) §4
> and [`docs/plans/indicator-implementation.md`](docs/plans/indicator-implementation.md)
> Task 0.0. This doc still references the legacy name in historical-context
> paragraphs; the running implementation uses the new name once Phase 0 lands.

### Layer-shell positioning sketch

```rust
// On MenuRequested(x, y):
let monitor = display.monitor_at_point(x, y);            // Mutter logical → which output
let mon_geom = monitor.geometry();                       // logical, contiguous (Mutter is the source of truth)
let local_x = (x as i32) - mon_geom.x();                 // monitor-local logical
let local_y = (y as i32) - mon_geom.y();
let half = WINDOW_SIZE / 2;
let mx = (local_x - half).clamp(0, mon_geom.width()  - WINDOW_SIZE);
let my = (local_y - half).clamp(0, mon_geom.height() - WINDOW_SIZE);

window.set_monitor(&monitor);
window.set_anchor(Edge::Left, true);
window.set_anchor(Edge::Top,  true);
window.set_margin(Edge::Left, mx);
window.set_margin(Edge::Top,  my);
window.present();
```

That's the entire positioning code. No multi-system coord conversions.

## Theme + UI parity

- Bundle the existing themes from `daemon/src/bundled_themes.rs` directly —
  the daemon already has them as Rust constants, so we just `use` them.
- Cairo paths translate cleanly from QPainter — every `painter.draw_*` call
  has an analog: `cr.move_to`, `cr.arc`, `cr.fill`, `cr.stroke`. Fonts via
  Pango (which Cairo + GTK use natively).
- The radial wheel artwork lives in `assets/radial-wheels/`; load via
  `gdk-pixbuf` and composite onto the cairo surface.
- Animations: 60Hz `glib::timeout_add` driven; same easing curves
  (OutBack, OutQuad) ported.
- KDE/Hyprland/COSMIC code paths drop entirely — layer-shell handles them
  natively.

## What's the editor and why

A persistent gap in the current overlay: customising the menu requires editing
`~/.config/oxidemx/config.json` by hand. The settings GUI already covers
most non-radial settings; the radial slices need a proper visual editor.

### Editor UI

- Opens from the tray icon or `oxidemx-settings`.
- Live preview of the menu (same widget as the overlay, in a non-layer-shell
  window for editability).
- Per-slice panel: label, action type (exec / submenu / macro / easy-switch),
  command, color, icon picker.
- Submenu builder: drag-and-drop subitems, recursive (subitems can also be
  submenus, up to a sensible depth).
- Save → writes config.json, daemon reloads via inotify (already supported).

### Icon picker

- "Browse theme": searchable list of every freedesktop symbolic icon
  available on the system (via `gtk::IconTheme::new` enumeration).
- "Browse files": pick a SVG or PNG from disk.
- "Tint to slice color": option to tint custom icons (already done for
  themed icons in the current Python overlay).
- Preview at the actual rendered size in the menu.

### Application launcher tab (the Flatpak ask)

The icon picker doubles as an application launcher when the slice's
action kind is `Exec`. A dedicated "Apps" tab enumerates every
desktop-registered application — both system .desktop entries and
**Flatpak apps** — and lets the user pick one to fill in *both* the
slice's command and its icon at once.

Sources:

- **Flatpak**: walk
  `~/.local/share/flatpak/exports/share/applications/*.desktop` and
  `/var/lib/flatpak/exports/share/applications/*.desktop`. Each
  Flatpak app exports a `.desktop` file with an `X-Flatpak=<appid>`
  hint and an `Exec=` line that already starts with `/usr/bin/flatpak
  run …`. We don't need the Flatpak CLI; reading these directories
  works out of the box on every Flatpak install (system or per-user).
- **Native apps**: standard XDG paths (`/usr/share/applications/`,
  `/usr/local/share/applications/`, `~/.local/share/applications/`).
- Parse via `freedesktop_entry_parser` or the simpler `ini` crate;
  either works.

UI:

- Search box that fuzzy-matches `Name` / `GenericName` / `Comment`.
- Filter chips: All / Native / Flatpak (filtered by the
  `X-Flatpak` key).
- Clicking an app fills the slice's `command` with the entry's `Exec`
  line (with field codes like `%u %f` stripped) and its `icon` with
  the entry's `Icon` field, which the resolver in `render::icons`
  already knows how to handle:
    * Bare names (e.g. `org.gnome.Console`) → `gtk::IconTheme` lookup
      finds them via the Flatpak icon export dirs that Flatpak adds to
      `XDG_DATA_DIRS` automatically.
    * Absolute paths → loaded as-is via gdk-pixbuf (covers the rare
      case of an app shipping its icon outside the standard export
      tree).
    * If the icon isn't in any theme search path (for instance because
      the user disabled the Flatpak portal), offer "Extract icon" —
      copies the icon file from the Flatpak export tree into
      `~/.local/share/oxidemx/icons/` and rewrites the slice to
      reference the absolute path. Lossless and self-contained.

This collapses the "find the right command-line incantation" and "find
a matching icon" steps into a single click — for both native and
Flatpak apps without distinguishing between them in the user's
mental model.

## New features the rewrite makes possible

The rewrite isn't an excuse to scope-creep, but layer-shell + GTK4 + the
editor architecture unlock several genuinely useful things at low marginal
cost. Ranked roughly by user value vs implementation effort:

### High value, low effort

1. **Per-app menu profiles.** The daemon already has `window_tracker.rs`.
   Add a profile field to config: "use menu A when Firefox is focused, menu B
   for Code, default otherwise." Hugely valuable for power users.
2. **Quick search inside the menu.** Type a letter or two while menu is open
   → matching slices highlight, others dim. Especially good for menus that
   reach 16-24 actions across submenus.
3. **Live config reload on edit.** The daemon already watches config.json
   via inotify. The new overlay should subscribe to the same change and
   re-render — so saving in the editor immediately updates a visible menu.

### High value, medium effort

4. **Hotkey + button combos.** Hold the gesture button + tap a keyboard key
   → fires the slice in that direction-key position (Q/W/E/A/S/D/Z/X). Adds
   keyboard activation without giving up the radial UI; multiplies practical
   menu capacity.
5. **Conditional slices.** Hide a slice unless a predicate matches:
   "Mute only when audio is playing", "Stop only when media is playing",
   "Git commands only in a Git repo." Predicates can be cheap shell
   one-liners or D-Bus checks.
6. **Drag-and-drop into slice.** Drag a file onto an open menu, drop on a
   slice → slice executes with the dropped path as `$1`. Turns the menu
   into a contextual "open with" replacement.

### Medium value

7. **Workspace-aware menus.** Per-workspace overrides; daemon already knows
   the workspace.
8. **Action templates.** Curated preset menus ("Coding", "Media",
   "Browser") that users can import as a starting point. Pure config files;
   ship a few in `assets/templates/`.
9. **Theme contributions.** Currently themes are baked into the daemon. Move
   to `~/.local/share/oxidemx/themes/<name>.toml` so users can drop in
   community themes without rebuilding.

### Nice-to-have

10. **Optional usage stats** (local, opt-in): "Terminal: 73 uses this week.
    Consider moving to top slice." Only if it's not creepy — purely local,
    purely opt-in, with a "clear stats" button.
11. **Tooltip on hover** (likely already in the python; carry over).
12. **Sound effects per action** (configurable, default off).

### Out of scope for the rewrite

- juhflow integration (explicit user request).
- Plugin / scripting system (too complex for the value).
- AI integration beyond the existing AI submenu (keys, accounts, scope creep).
- Settings sync across machines (already works via standard tools like
  syncthing on `~/.config/oxidemx`).

## Migration plan

1. **Build feature-parity overlay-rs first.** No editor, no new features —
   just visually identical to the current Python overlay, driven by the
   existing D-Bus contract. Run side-by-side: keep the Python overlay
   working, ship the Rust one as opt-in via a config flag.
2. **Validate against the user's setup** (1.25× fractional, vertical
   layout). Layer-shell + Mutter logical coords should make this trivial,
   but verify.
3. **Add the editor.** Replace the radial-slice-editing parts of the
   current Python settings GUI with a GTK4 panel. The rest of the settings
   GUI (in `overlay/settings_*.py`) can stay Python for now.
4. **Add per-app profiles + quick search** — the two biggest user-facing
   wins.
5. **Deprecate the Python overlay** once parity is solid and the new
   features are stable. Remove `overlay/` after one or two release cycles.

## Build prerequisites

`overlay-rs` links against the GTK4 stack. The base Bazzite image ships
the runtime libraries but not the `*-devel` headers needed for
`pkg-config` discovery. Layer these once before `cargo build -p
oxidemx-overlay`:

```
sudo rpm-ostree install \
    cairo-devel \
    gdk-pixbuf2-devel \
    gtk4-devel \
    gtk4-layer-shell-devel \
    pango-devel \
    glib2-devel
sudo systemctl reboot
```

(`gtk4` itself, `gtk4-layer-shell`, `cairo`, `pango`, and
`gdk-pixbuf2` runtimes are already layered for the Python overlay's
sake — these `*-devel` siblings just add the headers.)

`oxidemx-shared` has no system dependencies and compiles + tests
cleanly without any of the above (`cargo test -p oxidemx-shared`).

These layered packages will be folded into `install.sh`'s
`install_deps_fedora_atomic()` once `overlay-rs` is the default
overlay; for now they're a manual step for anyone building the
rewrite branch.

## Status (2026-05-02 working tree, end of pre-GTK-link push)

The GTK4 dev libraries aren't layered on the dev box yet, so visual
testing is gated on a `rpm-ostree install … && reboot`. The chunks
below are everything that can land *without* needing that — they
either compile + test in pure Rust (`oxidemx-shared`) or are
overlay-rs code that links once the devel packages are in place.

Landed (`rust-gtk4-overlay` → 65b916d):

  * Workspace skeleton: daemon + oxidemx-shared + overlay-rs.
  * `oxidemx-shared` (36/36 tests passing, no system deps):
      - AppConfig / Slice / RadialMenuConfig serde types.
      - ThemeName, all 11 themes from the Python overlay
        (vector + 3D), Theme::load / Theme::catalogue, hex →
        RGBA helpers.
      - ActionKind enum.
      - applications: native + Flatpak `.desktop` enumeration,
        clean_exec_line for field-code + `@@u`/`@@U` stripping,
        case-insensitive ranked search. Exercises against 145
        real entries on the dev box.
      - profiles: `ProfileResolver::menu_for(focused_class)`
        with case-insensitive matching, fallback when the
        profile file is missing, save_main / save_profile for
        the editor's write path.
      - conditions: `visible_if` predicate enum (executable,
        file_exists, process_running, env_set / env_equals,
        all / any / not). Pure-Rust evaluator.
      - examples/list_apps.rs smoke test.

  * `overlay-rs` (links once devel packages are layered):
      - dbus.rs: typed zbus `#[proxy]` for the daemon's
        MenuRequested / HideMenu / CursorMoved signals,
        per-stream tasks, glib MainContext integration.
      - window.rs: layer-shell window with show_at() →
        monitor lookup + Edge margin offsets. Sidesteps the
        entire xcb / dpr / xdotool stack.
      - radial.rs: RadialState (theme + slices + per-slice
        highlight + Rc<IconCache>) shared between draw_func
        and the event pump.
      - render/slices.rs: cairo donut-wedge slices, hover
        overlay, glow ring, icon background; glyph composite
        from the icon resolver.
      - render/icons.rs: three-tier resolver (path /
        freedesktop name / placeholder), tinted via
        Operator::SourceIn, cached by
        (source, size, color_packed_u32).
      - theme.rs: ActiveTheme with fallback + tests.
      - input.rs: slice_index_at hit-test + 8 unit tests
        (will run with the GTK link).
      - geometry.rs: layout constants + Geometry struct.

What still needs the GTK link (queue for after the reboot):

  * Animation timer (glib::timeout_add) → smooth highlight tweens.
  * Toggle-mode hover (gtk::EventControllerMotion) + click
    (gtk::GestureClick) + action dispatch.
  * Submenu pop-out renderer (cairo translation of `_draw_submenu`).
  * 3D-theme renderer (radial_image + radial_params path).
  * Internal-id custom glyphs (legacy "play_pause", "folder",
    "easy_switch", "os_*" — direct cairo paths).
  * editor/: window + slice panel + icon picker (with the
    Flatpak app launcher tab using `oxidemx_shared::applications`)
    + live preview.
  * tray.rs: KStatusNotifierItem.
  * Wire ProfileResolver into RadialState (one-line swap, but
    needs the link to compile-test).
  * Apply `Condition::eval` filter in the slice render loop.
  * Config inotify watcher → `RadialWidget::reload_from`.

What's only blocked on the daemon side (independent of the overlay
build):

  * Daemon → oxidemx-shared type migration (eliminate drift).
    Daemon's existing test suite gives us the safety net.
  * Daemon emitting a focused-class signal so the overlay can
    drive ProfileResolver::menu_for().

The next natural pause point is the visual smoke test — see the
Build prerequisites section above and `overlay-rs/run-smoke-test.sh`.

## Risks / unknowns

- **Layer-shell on GNOME requires a shell extension or wlr-layer-shell
  passthrough.** Verify Mutter / GNOME 50 supports this directly. If not,
  bundle a small layer-shell-bridge extension alongside the cursor helper,
  or fall back to the existing xcb path on GNOME-without-layer-shell.
- **Cursor-coordinate source.** The cursor helper extension currently powers
  cursor reads on GNOME. Keep it as the input — layer-shell solves
  *positioning*, not *cursor query*. The extension stays.
- **Animations 60Hz on Cairo.** Should be fine. If not, switch the radial
  widget to a `gtk::GLArea` with shaders later.
- **Crate selection.** `gtk4` 0.9+, `gtk4-layer-shell` 0.5+, `cairo-rs`,
  `gdk-pixbuf`, `glib`, `zbus`, `serde`. All maintained, all in
  Bazzite-layerable Fedora packages.

## Open questions for the user

- Daemon refactor: do we want to extract a `oxidemx-shared` workspace
  member now, or keep config types duplicated for the first iteration and
  unify later? (Recommendation: extract now, it's small and avoids drift.)
- Editor: separate window vs. embedded in the existing settings UI?
  (Recommendation: separate window opened from the settings UI's "Edit menu"
  button — easier to live-preview without competing with other settings
  controls.)
- Per-app profiles: store as separate config files
  (`~/.config/oxidemx/profiles/<app>.json`) or as a nested object inside
  the main config? (Recommendation: separate files — easier to share, easier
  to delete, easier to detect via inotify.)
