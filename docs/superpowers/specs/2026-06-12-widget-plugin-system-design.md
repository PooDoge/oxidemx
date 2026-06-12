# Widget Plugin System — Design Spec

**Date:** 2026-06-12 (rev 2 — folded in the Claude Design handoff "Slice Widget
Selector & Downloader" draft 0.9; design bundle archived at
`/tmp/omx-design/oxidemx-design-system/`, primary files
`spec-widget-selector.jsx` + `menu-page.jsx`)
**Status:** Approved
**Branch:** rust-gtk4-overlay

## 1. Goal

Make widgets first-class citizens of radial-menu slices. Users install third-party
widgets as single files, assign any widget to any slice on any page through a
single-click picker, and configure each widget's options — globally (all
instances of that widget) or per slice instance — from the settings app. Widget
developers write normal Rust against a small published API, compile once, and
the artifact runs on every architecture.

### Non-goals (v1)

- Migrating the seven built-in `WidgetSource`s to the plugin runtime (they stay
  native; they appear in the picker's Widgets group alongside custom widgets).
- Registry *browsing* UI (ratings, categories, auto-update). v1 ships the
  downloader dialog shell, install-from-file, install-from-URL, and the install
  pipeline; the populated store lands later.
- Fully custom option UIs shipped by widgets — the options card is generated
  from the manifest schema only.
- Widgets on the center hub (schema permits it later via instance keys; layout
  work deferred — see Open questions).
- Drag-from-picker onto the ring preview.
- Non-Rust widget authoring (the WASM boundary permits it later; we only
  publish a Rust PDK in v1).

## 2. Decisions and rationale

| Decision | Choice | Why |
|---|---|---|
| Plugin format | **WebAssembly** (`wasm32-wasip1` module) | One artifact for x86_64 + aarch64; sandboxed (a faulty widget cannot crash an always-on overlay; no FS, no spawn); install-by-file/URL. Native dylibs rejected (per-arch builds, `abi_stable` dormant, no isolation); scripting rejected (weaker typing/isolation, slower). Precedent: Zed, Zellij. The design handoff's `entry: "weather.js"` placeholder is superseded by this approved decision — manifest `entry` points at the `.wasm` module. |
| Engine | **wasmi 1.x** (interpreter) | Instant startup, tiny host footprint, no JIT compile/cache step. Widget logic runs off the frame path so interpreter throughput is irrelevant. Zellij moved wasmtime → wasmi for exactly this profile. |
| Wire format across the boundary | `oxidemx-widget-proto` crate: `#[non_exhaustive]` serde enums, **postcard** encoding, integer `api_version` handshake | Forwards-compatible by construction (Zellij's protobuf lesson) without component-model toolchain weight. wasmi has no component-model support, so WIT is out anyway. |
| Rendering | **Retained display list** ("Scene"): widget returns draw primitives only when its state changes; host bakes them into the iced canvas layer and replays per frame | No production system lets plugins draw per-frame. Keeps the WASM boundary entirely off the 120fps frame path. |
| Interactivity | **Fully interactive**: widgets receive click / scroll / hover events on their wedge and can trigger permission-gated host actions | First-class means a media widget can play/pause and a weather widget can scroll forecast days. |
| Drawing freedom | **Free-form primitives within the wedge clip**, plus a `tile()` template helper (big value / sublabel / sparkline / label) in the PDK | Simple widgets stay ~10 lines; ambitious ones draw anything. |
| Settings | **Manifest-declared typed options schema**; settings app renders the card generically; two-bag storage (global + instance) with merge resolution | No custom settings-rs code per widget; blast radius always explicit in the UI. |
| Data fetching | **Host-mediated**: `http_get` host function gated by manifest permissions, host-side response cache and rate limiting; refresh via widget-requested timers + lifecycle events | Sandbox stays closed; greedy widgets can't spam APIs; identical fetches dedupe across instances. |
| Bundle | **`.omxw`** zip (manifest + wasm + assets), **ed25519-signed** | Single-file install; registry key pinned, sideloads prompt with the signer fingerprint. |
| Install location | `~/.config/oxidemx/widgets/<id>/` | Matches the design handoff and keeps all OxideMX state under one watched root. |
| Picker/editor UX | Behavior chip → inline picker panel → schema-driven options card, per the design handoff (§10) | Single-click apply, live previews, scope toggle with blast-radius banner. |

## 3. Architecture overview

```
┌────────────────────────────────────────────────────────────────────┐
│ settings-rs                                                        │
│  • slice editor: behavior chip + picker panel + options card (§10) │
│  • widget mini-previews via embedded widget host (1 fps, 0.5×)     │
│  • downloader dialog + install pipeline                            │
└──────────────┬─────────────────────────────────────────────────────┘
               │ writes ~/.config/oxidemx/config.json (atomic, watched)
               │ installs to ~/.config/oxidemx/widgets/<id>/
┌──────────────▼─────────────────────────────────────────────────────┐
│ overlay-rs                                                         │
│  ┌──────────────────────────┐   scenes + events (channels)         │
│  │ oxidemx-widget-host      │◄───────────────────────────────►UI   │
│  │  registry / loader       │   iced subscription + messages       │
│  │  wasmi instances         │                                      │
│  │  timers / http / cache   │   render: ring.rs replays cached     │
│  │  (dedicated worker)      │   Scene inside the wedge clip        │
│  └──────────────────────────┘                                      │
└────────────────────────────────────────────────────────────────────┘

new crates:
  oxidemx-widget-proto  — shared message/scene types (host + guest)
  oxidemx-widget-host   — wasmi runtime, registry, lifecycle (host side;
                          used by BOTH overlay-rs and settings-rs)
  oxidemx-widget-api    — guest PDK: Widget trait, register_widget!, helpers
```

### Units and boundaries

- **`oxidemx-widget-proto`** (no_std-friendly, serde + postcard): `Event`,
  `HostCmd`, `Scene`/`Prim`, `WedgeGeom`, `OptionSpec`/`SettingValue`,
  `ApiVersion`. The only crate both sides depend on. All enums
  `#[non_exhaustive]`; unknown variants decode to `Unknown` so old widgets
  survive new hosts and vice versa.
- **`oxidemx-widget-host`**: owns `WidgetRegistry` (scans install dir, parses +
  signature-verifies manifests, validates `api_version`), one wasmi `Store` +
  instance per *widget instance* (a widget placed on two slices = two instances
  with separate state), the timer wheel, the HTTP fetch pool (reqwest, shared
  cache keyed by URL, per-widget rate limit), settings resolution (§6), and the
  event router. Runs on a dedicated tokio task; communicates with the iced loop
  via `async_channel` (same pattern as `ai_client.rs` / the sampler).
  settings-rs embeds the same host for live mini-previews and the options
  card's wedge preview.
- **`oxidemx-widget-api`** (published for developers): wraps the raw ABI in
  `trait Widget { init, on_event, render }` + `register_widget!`, plus `Ctx`
  (set_timer, http_get, open_url, exec, haptic, log, settings access) and the
  `tile()` scene builder. Developers never see postcard or extern "C".
- **overlay-rs changes**: `actions.rs` routes click/scroll/hover on a custom-
  widget slice to the host; `ring.rs` replays the instance's cached `Scene`
  (translated + clipped to the wedge); config watcher additionally watches the
  widgets install dir for hot-register.
- **settings-rs changes**: §10 in full, plus the install pipeline.

## 4. Widget bundle and installation

**Bundle:** zip named `<id>-<version>.omxw` containing:

```
widget.json        manifest (required)
widget.wasm        compiled module (required; manifest `entry` names it)
icon.svg           picker-tile icon (required)
assets/…           optional static assets (images referenced by Scene)
SIGNATURE          ed25519 detached signature over the zip contents
```

**Signing:** registry bundles are signed with the pinned registry key.
Sideloaded bundles with unknown keys prompt with the signer fingerprint
("Install anyway?"); unsigned bundles prompt with a stronger warning. The
prompt is settings-rs UI; the overlay never installs.

**Install location:** `~/.config/oxidemx/widgets/<id>/`. Install = verify
signature → validate (manifest parses, `api_version` in supported range, wasm
instantiates, declared exports present) → unpack → **hot-register**: the
picker, the options card, and any live slices pick the widget up without
restarting the daemon or overlay (registry rescan on dir-watch event).
Uninstall = remove the directory; **settings bags are kept** (§6) and slices
referencing the widget fall back to a "missing widget" chip with a Reinstall
button (§9).

**Widget id:** slug `[a-z0-9.-]+`, unique per registry (e.g. `weather`,
`octowidgets.github`). Id collision on install prompts replace/cancel.

## 5. Manifest (`widget.json`) and option types

```jsonc
// ~/.config/oxidemx/widgets/weather/widget.json
{
  "id": "weather",
  "name": "Weather",
  "version": "1.4.0",
  "author": "JuhLabs",
  "homepage": "https://github.com/…",
  "api_version": 1,                      // host supports a range; checked at load
  "entry": "widget.wasm",                // sandboxed wasm module
  "icon": "icon.svg",
  "permissions": ["net:api.open-meteo.com",
                  "net:geocoding-api.open-meteo.com",
                  "open-url", "haptics"],      // also: "exec" (heavy prompt)
  "slice": {
    "refresh_ms": 900000,                // min enforced: 5000
    "fallback_icon": "weather-clear-symbolic"  // used by missing/error chip
  },
  "options": [                           // ← drives the auto-generated options card
    { "key": "location", "type": "location", "label": "Location",
      "hint": "city name or \"lat,lon\"", "required": true },
    { "key": "units",    "type": "enum",   "label": "Units",
      "values": ["c", "f"], "default": "c" },
    { "key": "display",  "type": "enum",   "label": "Slice display",
      "values": ["temp", "temp_condition", "full"], "default": "temp_condition" },
    { "key": "refresh",  "type": "select", "label": "Refresh",
      "values": [300, 900, 1800, 3600], "default": 900, "unit": "s" }
  ]
}
```

**Option types** — each `options[]` entry maps 1:1 to a settings-app control:

| type | Renders as | Value stored | Notes |
|---|---|---|---|
| `enum` | Segmented radio group | string | ≤ 4 values, else falls back to select |
| `select` | Dropdown | string \| number | `unit` formats labels (e.g. 900 → "Every 15 minutes") |
| `string` | Text input | string | `placeholder`, `maxlen` |
| `number` | Stepper / slider | number | `min`/`max`/`step`; slider when range ≤ 100 steps |
| `boolean` | Switch | bool | |
| `location` | Geocoding search → pinned chip | `{name, lat, lon}` | Built-in geocoder (Open-Meteo); widgets never see raw keystrokes |
| `color` | Curated swatch row | token name | Limited to the slice palette tokens — keeps the ring coherent |

Unknown `type` values render as a disabled row with "Update OxideMX to edit
this option" — forward-compatible with newer widgets on older hosts.

## 6. Scope & settings resolution

Settings exist at two levels: **global** (one bag per widget id, shared by
every instance) and **instance** (a partial override bag per slice). The
slice's widget pointer carries `scope`, which only chooses *where the options
card writes*; reads always resolve through the same merge:

```
fn resolve(widget_id, instance_key) -> Settings {
    defaults(manifest.options)              // 1. manifest defaults
      .merge(widgets.global[widget_id])     // 2. global user settings
      .merge(if slice.scope == "instance"   // 3. instance overrides
             { widgets.instances[instance_key][widget_id] })
}
// scope == "global" → step 3 skipped; the card writes to widgets.global
```

| Scenario | Behavior |
|---|---|
| Toggle slice → global | Card now edits `widgets.global`; banner lists every affected instance (this one highlighted). Instance bag is **kept** but ignored until toggled back. |
| Toggle global → slice | Instance bag is **seeded as a copy** of current resolved values, then diverges. |
| Same widget on 3 slices, global scope | One edit updates all 3 rings live — the banner makes the blast radius explicit before the user types. |
| Reset (per option) | Kebab on each control: "Reset to global" (instance scope) / "Reset to default" (global scope). |

The host computes resolution and delivers one flat map to the widget in `init`
and on `SettingsChanged`; the widget never knows which layer a value came from.
Per-option scope (location per-slice but units global) is deferred — the
two-bag merge already permits it; the card UI doesn't expose it yet.

## 7. Config schema (v3) and migration

Widget settings live in a dedicated `widgets` store — slices keep only a
pointer. `config.json` gains `schema_version: 3`:

```jsonc
{
  "schema_version": 3,
  "pages": [{
    "name": "Apps",
    "slices": [
      { "label": "Screenshot", "type": "exec", "command": "gnome-screenshot -i" },
      { "label": "Weather",                       // ← widget slice
        "type": "widget",
        "widget": { "source": { "custom": "weather" },   // or built-in source
                    "scope": "instance",
                    "instance_key": "apps.slot4" },
        "color": "yellow" }
    ]
  }],
  "widgets": {
    "global": {                                   // one bag per widget id
      "weather": { "location": { "name": "Oslo, Norway", "lat": 59.91, "lon": 10.75 },
                   "units": "c", "display": "temp_condition", "refresh": 900 }
    },
    "instances": {                                // keyed by instance_key
      "apps.slot4": {
        "weather": { "location": { "name": "San Francisco, CA, US",
                                   "lat": 37.77, "lon": -122.42 } }
      }                                           // partial — merges over global
    }
  }
}
```

`oxidemx-shared` changes:

```rust
// WidgetSource loses Copy, gains:
pub enum WidgetSource {
    Weather, Cpu, Memory, Network, Disk, TasksDue, MouseBattery,
    Custom(String),                       // installed widget id
}

pub struct WidgetConfig {
    pub source: WidgetSource,
    pub format: Option<String>,           // existing, built-ins only
    #[serde(default)]
    pub scope: WidgetScope,               // Instance (default) | Global
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_key: Option<String>,     // set when a custom widget is assigned
}

// AppConfig gains:
pub schema_version: u32,                                  // 3
pub widgets: WidgetStore,                                 // { global, instances }
```

**Instance keys** are human-readable `<page-slug>.slot<N>` (design-faithful;
the options-card breadcrumb shows the real path). settings-rs is the only
writer and **rewrites keys on every slice move/swap/page rename/page delete**
through a single `rekey_instance(old, new)` helper. Hand-edits of config.json
that reorder slices without rekeying are accepted as power-user breakage.

**Migration v2 → v3** (one-shot, on first load of a v2 config; a
`config.json.v2.bak` is written before rewriting):

- add `schema_version: 3` and empty `widgets` store;
- lift the global `weather_location`/`weather_place` overlay settings into
  `widgets.global.weather.location` (the canonical migration case — the old
  fields are kept for one release, read-preferred from the new location);
- existing built-in widget slices are untouched (their `WidgetConfig` gains
  defaults via serde).

## 8. Runtime API

### Guest-facing (via `oxidemx-widget-api`)

```rust
use oxidemx_widget_api::*;

struct Weather { temp: Option<f32>, cond: String, day: usize }

impl Widget for Weather {
    fn init(&mut self, ctx: &Ctx) {
        ctx.set_timer("refresh", ctx.setting_u64("refresh").max(5));
        self.fetch(ctx);
    }

    fn on_event(&mut self, ev: Event, ctx: &Ctx) -> bool {
        match ev {
            Event::Timer(id) if id == "refresh" => { self.fetch(ctx); false }
            Event::MenuOpened { .. } => ctx.data_stale(300), // redraw if stale
            Event::HttpResponse { id, body, .. } => { self.apply(body); true }
            Event::Scroll { delta, .. } => { self.day = …; true }
            Event::Click { .. } => { ctx.open_url("https://…"); false }
            Event::SettingsChanged => { self.fetch(ctx); true }
            _ => false,
        }
    }

    fn render(&self, geom: WedgeGeom) -> Scene {
        tile()                                    // template helper —
            .value(format!("{:.0}°", t))          // or free-form Scene::new()
            .sublabel(&self.cond)                 // with arcs/paths/text/images
            .label("WEATHER")
            .into_scene(geom)
    }
}
register_widget!(Weather);
```

### Boundary contract (raw ABI under the PDK)

- Exports: `omx_api_version() -> u32`, `omx_alloc(len) -> ptr`,
  `omx_init(cfg_ptr, len)`, `omx_event(ptr, len) -> u32` (bit0 = needs_render),
  `omx_render(geom_ptr, len) -> packed ptr/len of Scene`.
- Imports (host functions): `omx_cmd(ptr, len)` carrying a postcard `HostCmd`:
  `SetTimer`, `CancelTimer`, `HttpGet { id, url }`, `OpenUrl`, `Exec`,
  `HapticPulse(pattern)`, `Log(level, msg)`. HTTP responses come back
  asynchronously as `Event::HttpResponse`. Every command is checked against
  the manifest permission grants; denials return errors, not traps.
- Versioning: host refuses out-of-range `api_version` with a settings-rs
  warning (never a crash); non-exhaustive proto enums cover minor evolution
  inside a version; the host keeps decoding support for version N−1.

### Events (v1)

`Init`, `Timer(id)`, `MenuOpened { page }`, `MenuClosed`, `SliceVisible`,
`SliceHidden`, `Hover { entering }`, `Click`, `Scroll { delta }`,
`SettingsChanged`, `HttpResponse { id, status, body }`. No free per-frame
ticks.

### Scene (display list)

Coordinates are wedge-local: origin at the icon anchor point, +y down, with
`WedgeGeom { width, height, inner_radius, outer_radius, angle_start, angle_end,
hovered: f32 }`. Primitives (`Prim`): `Arc`, `Path` (move/line/quad/cubic),
`Fill`/`Stroke` styling with palette colors (`Palette(key)` resolves to the
user's theme) or raw RGBA, `Text { size, weight, align }`,
`Sparkline(Vec<f32>)`, `Image(asset_path)` (decoded + cached host-side),
`Group(transform)`. The host clips the scene to the wedge path, applies
hover/animation transforms from the track-based animation system, and renders
inside the existing canvas layer — widgets inherit the 3D shader framing for
free. Scene caps: 64 KB encoded, 2048 prims.

### Lifecycle, scheduling, resource caps

- **Frame path:** zero WASM calls. The painter replays the last `Scene` per
  instance; a scene revision counter busts the canvas cache only when a widget
  re-rendered (plays nice with the cache_epsilon mitigation).
- **Event path:** UI thread → channel → widget worker → `omx_event`; if
  needs_render, `omx_render`, decode, post `Message::WidgetScene(instance,
  scene)` back. One channel hop + one interpreter call — well under a frame.
- **Render budget:** 2 ms per `omx_render` (wasmi fuel metering); event calls
  get ~5 ms. Budget exhaustion abandons the call and counts a strike; three
  strikes disables the instance for the session (§9).
- **Refresh floor:** `refresh_ms` and `set_timer` clamp to ≥ 5000 ms. Timers
  fire only while the widget is placed on some page; while the menu is closed
  they relax (fire at most once per close period so data is warm on open).
  `MenuOpened` is the canonical "refresh if stale" hook.
- **Picker previews:** real renderer at 0.5× scale, capped at 1 fps, paused
  whenever the picker panel is not visible.
- **HTTP cache:** host caches responses keyed by URL for the refresh interval;
  identical fetches across instances/widgets share one request.

## 9. Error handling

| Failure | Behavior |
|---|---|
| Manifest invalid / api_version unsupported | Listed as "incompatible" in the picker's widget group (disabled tile); never crashes either app |
| Signature unknown/missing on install | Prompt with fingerprint / stronger unsigned warning; user decides |
| wasm trap, fuel exhaustion ×3, scene cap exceeded | Instance disabled for the session; wedge renders fallback: `slice.fallback_icon` dimmed + ⚠ badge; picker tile shows last error; "Restart widget" in the options-card kebab |
| Widget uninstalled while placed on slices | Slice keeps its config; chip becomes a **"missing widget" chip with a Reinstall button**; settings bags kept |
| HTTP denied / rate-limited | `Event::HttpResponse { status: 0 }` with error body; widget renders gracefully |

The overlay process is never taken down by widget code: all wasmi calls are
wrapped, the worker catches and logs, the painter only touches decoded data.

## 10. Settings app UX (per design handoff, draft 0.9)

### 10a. UX flow

1. **Expand a slice** in Settings › Menu. Its current behavior shows as a
   **chip** (icon · name · summary · `Change…`).
2. **Change…** opens the **picker panel** inline below the chip. Search field
   auto-focused; `Esc` cancels.
3. **One click** on any tile applies it immediately — no confirm step. The
   panel collapses back to the chip; the ring preview above updates live.
4. If a **widget** was chosen and it declares options, the **options card**
   expands under the chip, pre-filled from resolved settings (§6).
5. First control in the card is the **scope toggle**: *This slice only* vs
   *All [Widget] slices*. Edits write through on change (no Save button),
   debounced 400 ms.
6. **Get more widgets…** tile opens the downloader dialog (§11). Newly
   installed widgets appear in the picker without restarting.

Selecting a different behavior **never deletes** the previous configuration in
the same session — the old action/widget config is kept in memory so the user
can click back to it ("undo by reselect"). It is garbage-collected when the
editor row collapses.

### 10b. Behavior chip (collapsed state)

One row that always answers "what does this slice do?" — 40 px icon tile,
title, one-line summary, `Change…` button. Widget slices get an accent border,
soft accent wash and a `WIDGET` tag so they're scannable in a long slice list.
Collapsed slices in the list render as compact reorder rows (move handles,
icon, label + slot number, summary, kind tag).

### 10c. Picker panel

Two labelled groups in one scroll:

- **Built-in actions** — compact tiles (icon + name + one-line sub) for every
  `ActionKind` (hint: "Run once when the slice is clicked").
- **Widgets · n installed** — taller tiles with a **live mini-preview**
  rendered by the widget's own slice renderer (built-ins use the native
  sampler), author + version line, and a **gear badge** when the widget
  declares options. Hint: "Live data drawn inside the slice · single click
  applies."

Current choice gets an accent ring + check. Grid is 4-up at ≥ 920 px, 3-up
below. Last tile is always the dashed **Get more widgets…** stub.

| Interaction | Behavior |
|---|---|
| Single click / Enter / Space | Applies the tile, closes the panel, focus returns to the chip |
| Type-to-search | Filters both groups by name, summary, author; group headers hide when empty |
| Arrow keys | Roving focus across the grid, wraps within a group |
| Esc / Cancel | Closes without change |
| Widget mini-preview | Real renderer at 0.5×, 1 fps cap, paused when panel not visible |

Selecting a widget sets `kind = Widget`, `widget.source = Custom(id)` (or the
built-in source), assigns an `instance_key`, and auto-labels the slice with
the widget name (shown with an "auto label" tag; user can override).

### 10d. Widget options card

Generated entirely from the manifest `options[]` (§5) — widgets never ship
their own settings UI. **Header:** widget icon, "{Name} — widget options",
"Declared by the widget · rendered by OxideMX", version + author tags.
**Body, two columns:** left — scope toggle first (*This slice only / Stored
with {Page} › Slot {N}* vs *All {Name} slices / Shared by {n} instances*;
global scope shows a banner listing every affected instance with this one
highlighted), then one control per option; right — **live wedge preview**
(hover-state wedge rendered by the real renderer, re-renders on every option
change). **Footer:** scope tag (`THIS SLICE` / `GLOBAL`) + a mono breadcrumb
of the exact config path being written, e.g.
`config.json → widgets.instances["apps.slot4"].weather`. Per-option kebab:
"Reset to global" / "Reset to default".

For widget slices the Appearance section drops the icon input (the widget
draws its own slice content — noted inline) and keeps the color picker and
visibility selector.

### 10e. Missing widget chip

Orphaned widget slices (uninstalled id) render the chip with the manifest
`fallback_icon` (if cached) or a generic puzzle icon, a "missing widget"
summary, and a **Reinstall** button that opens the downloader.

## 11. Downloader

v1 ships the dialog shell + install pipeline; registry browsing UI lands
later. Dialog: header ("Get more widgets" / "Community widgets — installed
ones appear in the slice picker"), search field, list rows (icon, name, by
author, description, download count, Install button / "✓ Installed" tag), and
a footer: `STUB — for now drop a .omxw bundle into ~/.config/oxidemx/widgets/`
plus **Install from file…** and **Install from URL…** actions, which run the
§4 pipeline with the signature prompt.

Registry contract (a static JSON site initially, e.g. GitHub Pages):

```
GET  https://widgets.oxidemx.org/v1/index.json        // paged catalog
GET  https://widgets.oxidemx.org/v1/w/<id>.json       // manifest + versions
GET  https://widgets.oxidemx.org/v1/w/<id>-<ver>.omxw // the bundle
```

| Security control | Detail |
|---|---|
| Signature | ed25519 over the bundle; registry key pinned; sideloads prompt with fingerprint |
| Sandbox | wasm module — no FS, no spawn; network restricted to manifest `permissions[]` |
| Resource caps | Render budget 2 ms/frame, 1 fps in picker previews, `refresh_ms` floor 5 s |
| Uninstall | Removes the directory; orphaned slices → missing-widget chip; settings bags kept |

## 12. Example widget + developer workflow

`examples/widgets/weather/` in-repo — the canonical reference: network
permission with domain allowlist, `location` setting usable globally or
per-instance (home/office wedges), timer + menu-open refresh, scroll to cycle
forecast days, click to open the forecast page, haptic tick on scroll, tile
template for the main view + free-form drawing for the forecast strip.

```bash
cargo generate oxidemx/widget-template     # or copy examples/widgets/weather
cargo build --release --target wasm32-wasip1
oxidemx-widget pack --sign dev             # validates manifest, zips .omxw,
                                           # dev-key signs (unsigned prompt on install)
oxidemx-widget install ./weather-1.4.0.omxw   # or the settings-app file picker
```

`just dev-widget` rebuilds + reinstalls + lets the dir-watch hot-reload the
instance (state resets on reload — acceptable for v1).

## 13. Testing

- **proto crate:** round-trip serde tests incl. unknown-variant forward-compat.
- **widget-host:** unit tests with a fixture widget compiled in CI to
  wasm32-wasip1 — lifecycle order, fuel/budget exhaustion → three-strikes
  disable, timer clamping, settings resolution order (defaults ← global ←
  instance; seed-on-toggle), HTTP cache dedupe, scene caps, signature
  verification, hot-register on dir change.
- **shared config:** v2 → v3 migration round-trip incl. `config.json.v2.bak`
  and weather-location lift; `rekey_instance` on move/swap/rename.
- **example weather widget:** guest logic tests run natively (`Ctx` mockable).
- **settings-rs:** state-machine tests for picker messages, undo-by-reselect,
  scope-toggle write paths + seeding, debounced write-through, rekey on
  swap/move.
- **Manual/visual:** `just dev-widget` + overlay visual test flow; trap fixture
  widget verifies the fallback wedge and missing-widget chip.

## 14. Open questions (carried from the design handoff)

- Widget on the **center hub**? Schema supports it via instance key
  `"<page>.hub"`; layout work only.
- Per-option scope (location per-slice, units global) — two-bag merge permits
  it; card UI deferred.
- Registry governance: single org signing key vs. per-author keys + trust
  prompt, for 1.0.
- Long-press **quick-settings** popover on the ring itself vs. settings-app
  round-trip.

## 15. Build order (for the implementation plan)

1. `oxidemx-widget-proto` (types) → 2. shared-config v3 + migration →
3. `oxidemx-widget-host` (wasmi runtime, registry, signature, no UI) →
4. overlay integration (worker, scene replay, events) → 5. `oxidemx-widget-api`
PDK + weather example + pack CLI → 6. settings-rs chip + picker panel →
7. options card (scope, resolution writes, wedge preview) → 8. downloader
dialog + install pipeline + missing-widget chip → 9. docs
(`docs/widgets/authoring.md`).
