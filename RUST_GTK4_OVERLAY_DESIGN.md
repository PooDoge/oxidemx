# Rust + GTK4 + layer-shell overlay rewrite — design doc

Status: draft, not yet implemented.
Branch: `rust-gtk4-overlay` (off master).

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
  us share types with the daemon via a small `juhradial-shared` crate.
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
juhradial-mx/
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
└── juhradial-shared/             # NEW — types shared between daemon + overlay
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

Unchanged. The daemon emits `MenuRequested(x, y)` and `HideMenu()` on
`org.kde.juhradialmx` with `(double, double)` cursor coords in Mutter logical
pixels. The new overlay subscribes to these via `zbus`.

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
`~/.config/juhradial/config.json` by hand. The settings GUI already covers
most non-radial settings; the radial slices need a proper visual editor.

### Editor UI

- Opens from the tray icon or `juhradial-settings`.
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
      `~/.local/share/juhradial/icons/` and rewrites the slice to
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
   to `~/.local/share/juhradial/themes/<name>.toml` so users can drop in
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
  syncthing on `~/.config/juhradial`).

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
juhradial-overlay-rs`:

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

`juhradial-shared` has no system dependencies and compiles + tests
cleanly without any of the above (`cargo test -p juhradial-shared`).

These layered packages will be folded into `install.sh`'s
`install_deps_fedora_atomic()` once `overlay-rs` is the default
overlay; for now they're a manual step for anyone building the
rewrite branch.

## Status (2026-05-02 working tree)

What's landed on `rust-gtk4-overlay`:

  * Workspace skeleton: daemon + juhradial-shared + overlay-rs.
  * `juhradial-shared`: AppConfig / Slice / RadialMenuConfig serde
    types, ThemeName, ActionKind, all 11 themes from the Python
    overlay (vector + 3D), Theme::load + Theme::catalogue, hex →
    RGBA helpers. 8/8 tests passing.
  * `overlay-rs/src/dbus.rs`: typed zbus #[proxy] for
    MenuRequested(i32,i32) / HideMenu() / CursorMoved(i32,i32),
    one task per signal stream, GTK MainContext integration.
  * `overlay-rs/src/window.rs`: layer-shell window + show_at()
    that resolves cursor → monitor → margins.
  * `overlay-rs/src/radial.rs`: RadialState + RadialWidget wired to
    the renderer; on_cursor_moved drives the slice highlight.
  * `overlay-rs/src/render/slices.rs`: cairo translation of
    `_draw_slice` — donut wedges, hover overlay, glow ring, icon
    background. Icon glyph is a placeholder pending the
    icon-resolver port.
  * `overlay-rs/src/theme.rs`: ActiveTheme wrapper with explicit
    fallback + tests.
  * `overlay-rs/src/input.rs`: slice_index_at hit-test + 8 unit
    tests covering each cardinal/diagonal slot, deadzone, max
    radius, boundary rounding.
  * `overlay-rs/src/geometry.rs`: layout constants, Geometry
    struct.

What's stubbed (TODO each becomes one focused commit):

  * `render/icons.rs`: three-tier icon resolution (path / internal
    id / theme).
  * `render/animation.rs`: 60Hz frame ticker, easing curves, in-flight
    tweens for highlight progress.
  * `editor/`: window + slice panel + icon picker + live preview.
  * `tray.rs`: KStatusNotifierItem with Edit / Settings / Quit.
  * Per-app profile loader (window_tracker integration).
  * Quick-search overlay.
  * Drag-and-drop targets.
  * Hotkey + button combos.
  * 3D-theme renderer (`radial_image` + `radial_params` path).
  * Submenu pop-out renderer (cairo translation of `_draw_submenu`).

The next natural pause point is once the icon resolver lands and you
can layer the `*-devel` packages and try `cargo run -p
juhradial-overlay-rs` against the running daemon — that's the first
moment where visual non-automated testing is needed.

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

- Daemon refactor: do we want to extract a `juhradial-shared` workspace
  member now, or keep config types duplicated for the first iteration and
  unify later? (Recommendation: extract now, it's small and avoids drift.)
- Editor: separate window vs. embedded in the existing settings UI?
  (Recommendation: separate window opened from the settings UI's "Edit menu"
  button — easier to live-preview without competing with other settings
  controls.)
- Per-app profiles: store as separate config files
  (`~/.config/juhradial/profiles/<app>.json`) or as a nested object inside
  the main config? (Recommendation: separate files — easier to share, easier
  to delete, easier to detect via inotify.)
