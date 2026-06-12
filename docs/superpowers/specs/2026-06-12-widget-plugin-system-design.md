# Widget Plugin System — Design Spec

**Date:** 2026-06-12
**Status:** Approved direction (all open decisions deferred to recommendations)
**Branch:** rust-gtk4-overlay

## 1. Goal

Make widgets first-class citizens of radial-menu slices. Users install third-party
widgets as single files, assign any widget to any slice on any page, and configure
each widget's options — globally (all instances of that widget) or per slice
instance — from the settings app. Widget developers write normal Rust against a
small published API, compile once, and the artifact runs on every architecture.

### Non-goals (v1)

- Migrating the seven built-in `WidgetSource`s to the plugin runtime (they stay
  native; migration is a follow-up once the API proves out).
- A hosted widget store backend. v1 ships install-from-file and install-from-URL;
  the "Get more widgets" tile links to a static gallery page.
- Widgets outside slice wedges (center puck, full pages, panel applets).
- Non-Rust widget authoring (the WASM boundary permits it later; we only publish
  a Rust PDK in v1).

## 2. Decisions and rationale

| Decision | Choice | Why |
|---|---|---|
| Plugin format | **WebAssembly** (`wasm32-wasip1` module, no component model in v1) | One artifact for x86_64 + aarch64; sandboxed (a faulty widget cannot crash an always-on overlay); install-by-file/URL. Native dylibs rejected (per-arch builds, `abi_stable` dormant, no isolation); scripting rejected (weaker typing/isolation, slower). Precedent: Zed, Zellij. |
| Engine | **wasmi 1.x** (interpreter) | Instant startup, tiny host footprint, no JIT compile/cache step. Widget logic runs off the frame path so interpreter throughput is irrelevant. Zellij moved wasmtime → wasmi for exactly this profile. |
| Wire format across the boundary | **Protobuf-style versioned messages via postcard/serde over byte buffers** — concretely: `oxidemx-widget-proto` crate with `#[non_exhaustive]` serde enums, postcard encoding, and an explicit `api_version` handshake | Forwards-compatible by construction (Zellij's protobuf lesson) without WIT/component-model toolchain weight on hobbyist devs. wasmi has no component-model support, so WIT is out anyway. |
| Rendering | **Retained display list** ("Scene"): widget returns draw primitives only when its state changes; host bakes them into the iced canvas layer and replays per frame | No production system lets plugins draw per-frame. Keeps the WASM boundary entirely off the 120fps frame path. Maps onto the existing `draw_widget_wedge` canvas path. |
| Interactivity | **Fully interactive**: widgets receive click / scroll / hover events on their wedge and can trigger permission-gated host actions | This is what "first-class" means — a media widget can play/pause, a weather widget can cycle forecast days with the scroll wheel. |
| Drawing freedom | **Free-form primitives within the wedge clip**, plus a `tile()` template helper (big value / sublabel / sparkline / label) in the PDK | Simple widgets stay ~10 lines; ambitious ones can draw anything. |
| Settings | **Manifest-declared typed settings schema**; settings app renders the options form generically. Each option resolves per-instance → global → manifest default | No custom settings-rs code per widget. Exceeds Zed/Zellij practice (neither has a formal schema). |
| Data fetching | **Host-mediated**: `http_get` host function with permission gate, host-side response cache and rate limiting; refresh driven by widget-requested timers + lifecycle events | Sandbox stays closed; a greedy widget cannot spam APIs; host can dedupe identical fetches across instances. |
| Store stub | "Get more widgets…" tile opens the widget manager page, which has **Install from file** and **Install from URL** | URL install is nearly free once file install exists (Zellij model). |

## 3. Architecture overview

```
┌────────────────────────────────────────────────────────────────────┐
│ settings-rs                                                        │
│  • slice editor: visual action/widget selector grid                │
│  • schema-driven widget options form (global / per-instance)       │
│  • widget manager page (install/uninstall/permissions)             │
└──────────────┬─────────────────────────────────────────────────────┘
               │ writes ~/.config/oxidemx/config.json (atomic, watched)
               │ installs to ~/.local/share/oxidemx/widgets/<id>/
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
  oxidemx-widget-host   — wasmi runtime, registry, lifecycle (host only)
  oxidemx-widget-api    — guest PDK: Widget trait, register_widget!, helpers
```

### Units and boundaries

- **`oxidemx-widget-proto`** (no_std-friendly, serde + postcard): `Event`,
  `HostCmd`, `Scene`/`Prim`, `WedgeGeom`, `SettingValue`, `ApiVersion`. The only
  crate both sides depend on. All enums `#[non_exhaustive]`; unknown variants
  decode to `Unknown` so old widgets survive new hosts and vice versa.
- **`oxidemx-widget-host`**: owns `WidgetRegistry` (scans install dir, parses
  manifests, validates `api_version`), one wasmi `Store` + instance per *widget
  instance* (a widget placed on two slices = two instances with separate state),
  the timer wheel, the HTTP fetch pool (reqwest, shared cache keyed by URL,
  per-widget rate limit from `min_refresh_interval`), and the event router.
  Runs on a dedicated tokio task; communicates with the iced loop via
  `async_channel` (same pattern as `ai_client.rs` / the sampler).
- **`oxidemx-widget-api`** (published for developers): wraps the raw export/import
  ABI in `trait Widget { init, on_event, render }` + `register_widget!`, plus
  `Ctx` (set_timer, http_get, open_url, exec, haptic, log, get_setting) and the
  `tile()` scene builder. Developers never see postcard or extern "C".
- **overlay-rs changes**: `actions.rs` routes click/scroll/hover on a custom-widget
  slice to the host; `ring.rs` replays the instance's cached `Scene` (translated +
  clipped to the wedge) instead of `draw_widget_wedge`; config watcher additionally
  watches the widgets install dir.
- **settings-rs changes**: selector grid, options form, manager page (§7).

## 4. Widget bundle and installation

**Bundle:** gzip tarball named `<id>-<version>.omxwidget` containing:

```
widget.json        manifest (required)
widget.wasm        compiled module (required)
icon.svg           selector-grid icon (required)
assets/…           optional static assets (images referenced by Scene)
```

**Install location:** `~/.local/share/oxidemx/widgets/<id>/` (XDG data dir).
Install = validate (manifest parses, api_version in supported range, wasm
instantiates, declared exports present) → extract → registry rescan. Uninstall =
remove dir. The overlay watches this directory (same debounced notify mechanism
as the config watcher) so installs apply without restart.

**Widget id:** reverse-slug `author.name` (e.g. `poodoge.weather`), `[a-z0-9.-]+`.
Id collisions on install prompt replace/cancel in settings-rs.

## 5. Manifest (`widget.json`)

```jsonc
{
  "id": "poodoge.weather",
  "name": "Weather",
  "version": "1.2.0",
  "api_version": 1,                  // single integer, host supports a range
  "description": "Current conditions + 3-day forecast",
  "author": "PooDoge",
  "homepage": "https://github.com/…",
  "icon": "icon.svg",
  "permissions": {
    "network": ["api.open-meteo.com", "geocoding-api.open-meteo.com"],
    "exec": false,                   // may run shell commands via host
    "open_url": true,                // may open URLs in browser
    "haptics": true                  // may trigger MX4 haptic pulses
  },
  "min_refresh_interval_secs": 300,  // host clamps timers/fetches to this
  "settings": [
    {
      "key": "location",
      "type": "location",            // host renders geocoder search UI
      "label": "Location",
      "default": null,
      "required": true
    },
    {
      "key": "units",
      "type": "enum",
      "label": "Units",
      "options": [["metric", "°C"], ["imperial", "°F"]],
      "default": "metric"
    },
    { "key": "show_forecast", "type": "bool", "label": "Scrollable forecast", "default": true }
  ]
}
```

**Setting types (v1):** `string`, `number` (min/max/step), `bool`, `enum`
(value/label pairs), `color`, `location` (host-provided Open-Meteo geocoder
search, stores `{name, lat, lon}`). Every type has a generic iced renderer in
settings-rs; widgets cannot inject custom UI code into the settings app — the
schema *is* the options panel. (Custom option widgets are a possible v2 via
scene-rendered forms; explicitly out of scope now.)

**Permissions** are shown on the install confirmation screen and on the manager
page. Host functions check grants at call time; denied calls return errors to
the widget rather than trapping it.

## 6. Runtime API

### Guest-facing (via `oxidemx-widget-api`)

```rust
use oxidemx_widget_api::*;

struct Weather { temp: Option<f32>, cond: String, day: usize }

impl Widget for Weather {
    fn init(&mut self, ctx: &Ctx) {
        ctx.set_timer("refresh", 900);            // clamped to manifest min
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

- Exports: `omx_alloc(len) -> ptr`, `omx_init(cfg_ptr, len)`,
  `omx_event(ptr, len) -> u32` (bit0 = needs_render),
  `omx_render(geom_ptr, len) -> packed ptr/len of Scene`.
- Imports (host functions): `omx_cmd(ptr, len)` carrying a postcard `HostCmd`:
  `SetTimer`, `CancelTimer`, `HttpGet { id, url }`, `OpenUrl`, `Exec`,
  `HapticPulse(pattern)`, `Log(level, msg)`. HTTP responses come back
  asynchronously as `Event::HttpResponse`.
- Versioning: host calls `omx_api_version() -> u32` before init and refuses
  (with a settings-rs warning, not a crash) anything outside its supported
  range. The proto crate's non-exhaustive enums cover minor evolution inside a
  version; the integer bumps only on breaking changes, and the host keeps
  decoding support for version N−1.

### Events (v1)

`Init`, `Timer(id)`, `MenuOpened { page }`, `MenuClosed`, `SliceVisible`,
`SliceHidden`, `Hover { entering }`, `Click`, `Scroll { delta }`,
`SettingsChanged`, `HttpResponse { id, status, body }`, `Tick` *(none by
default — widgets get no free per-frame ticks)*.

### Scene (display list)

Coordinates are wedge-local: origin at the icon anchor point, +y down, with
`WedgeGeom { width, height, inner_radius, outer_radius, angle_start, angle_end,
hovered: f32 }` describing the slot. Primitives (`Prim`):

`Arc`, `Path` (move/line/quad cubic ops), `Fill`/`Stroke` styling with palette
colors (`Palette(key)` resolves to the user's theme) or raw RGBA, `Text { size,
weight, align }`, `Sparkline(Vec<f32>)`, `Image(asset_path)` (from the bundle's
`assets/`, decoded + cached host-side), `Group(transform)`. The host clips the
whole scene to the wedge path, applies hover/animation transforms from the
track-based animation system, and renders inside the existing canvas layer —
widgets inherit the 3D shader framing for free. Scene size is capped (64 KB
encoded, 2048 prims) to bound a misbehaving widget.

## 7. Lifecycle, scheduling, performance

- **Frame path:** zero WASM calls. The painter replays the last `Scene` per
  instance from a `HashMap<InstanceId, CachedScene>` shared via the existing
  state. A scene revision counter busts the canvas cache only when a widget
  actually re-rendered (plays nice with the cache_epsilon mitigation).
- **Event path:** UI thread sends events over a channel to the widget worker;
  worker calls `omx_event`; if needs_render, calls `omx_render`, decodes, posts
  `Message::WidgetScene(instance, scene)` back. Hover/scroll latency budget:
  one channel hop + one interpreter call (µs–low-ms) — well under a frame.
- **Fuel limit:** each `omx_event`/`omx_render` call runs with wasmi fuel
  metering (~5 ms worth). Exhaustion = the call is abandoned and counted; three
  strikes disables the instance for the session (placeholder wedge, §9).
- **Timers** only fire while the widget could matter: suspended when no
  instance of the widget is on any page (and optionally relaxed-rate while the
  menu is closed — fires at most once per close period so data is warm on open).
  `MenuOpened` is the canonical "refresh if stale" hook; `ctx.data_stale(secs)`
  is sugar the PDK provides over a host-tracked last-fetch timestamp.
- **HTTP cache:** host caches responses keyed by URL for
  `min_refresh_interval_secs`; two instances of the same widget (or two widgets
  hitting the same endpoint) share one fetch.

## 8. Config schema and settings storage

`oxidemx-shared` changes:

```rust
// WidgetSource loses Copy, gains:
pub enum WidgetSource {
    Weather, Cpu, Memory, Network, Disk, TasksDue, MouseBattery,
    Custom(String),                       // widget id, e.g. "poodoge.weather"
}

pub struct WidgetConfig {
    pub source: WidgetSource,
    pub format: Option<String>,           // existing, built-ins only
    /// Per-instance overrides; absent keys fall through to global → default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Map<String, serde_json::Value>>,
}

// AppConfig gains:
/// Global (all-instances) settings per installed widget id.
#[serde(default, skip_serializing_if = "HashMap::is_empty")]
pub widget_settings: HashMap<String, serde_json::Map<String, serde_json::Value>>,
```

**Resolution order** (computed by the host, delivered to the widget as one flat
map in `init` and on `SettingsChanged`): instance `settings` → `widget_settings
[id]` → manifest default. The widget never knows which layer a value came from.

Existing configs are untouched (all additions are optional fields with serde
defaults) — no schema migration needed. `Custom` ids referencing uninstalled
widgets render the placeholder wedge and a warning in settings-rs, never an
error.

## 9. Error handling

| Failure | Behavior |
|---|---|
| Manifest invalid / api_version unsupported | Widget listed as "incompatible" in manager page; not loadable; never crashes either app |
| wasm trap or fuel exhaustion ×3 | Instance disabled for the session; wedge renders placeholder (widget icon, dimmed, ⚠ badge, "widget crashed" caption); manager page shows last error + "restart widget" |
| Scene too large / undecodable | Treated as a trap (counts a strike); previous scene kept on screen |
| HTTP denied by permission / rate limit | `Event::HttpResponse { status: 0 }` with error body; widget's problem to render gracefully |
| Widget uninstalled while placed on slices | Placeholder wedge; slice config preserved so reinstall restores it |

The overlay process is never taken down by widget code: all wasmi calls are
wrapped, the worker task catches and logs, and the painter only ever touches
decoded `Scene` data.

## 10. Settings app UX

### 10a. Slice action selector (replaces the kind `pick_list`)

In `selected_slice_editor`, the **Kind picker row is replaced by a selector
grid**: a wrapped grid of square tiles (icon + short label), single click
selects and updates `slice.kind` immediately (same `Message::SetSliceKind`
flow).

```
┌ Action ────────────────────────────────────────────────┐
│ [Run] [Submenu] [Macro] [Shortcut] [Easy-Switch] [Dial]│
│ [Power] [Night Light] [Mouse] [Emoji] [Settings] [None]│
│ ── Widgets ──────────────────────────────────────────  │
│ [CPU] [Memory] [Network] [Disk] [Weather*] [Tasks]     │
│ [Battery] [⛅ Weather+] [♪ Media] [＋ Get more widgets…]│
└────────────────────────────────────────────────────────┘
```

Built-in actions first; a "Widgets" divider; built-in widget sources; installed
custom widgets (icon from bundle); a final stub tile that opens the widget
manager page. Selecting a custom widget sets `kind = Widget`,
`widget.source = Custom(id)`.

### 10b. Widget options form (below the grid when a custom widget is selected)

Rendered generically from the manifest settings schema. Each option row:
label + typed editor (text input / slider / toggler / pick_list / color /
location-search) + a **scope toggle** `[Global | This slice]`. "Global" edits
`widget_settings[id][key]`; "This slice" writes the instance override and shows
a "↩ reset to global" affordance. Permissions summary + version shown in a
collapsed footer.

### 10c. Widget manager page (new entry in the settings nav)

- List of installed widgets: icon, name, version, author, permission chips,
  enabled toggle, "Settings" (global form), "Uninstall", last-error line if any.
- **Install from file…** (file picker, accepts `.omxwidget`) and **Install from
  URL…** (text field; downloads, then same validate/confirm path).
- Install confirmation dialog shows manifest metadata + requested permissions.
- "Get more widgets" links to a gallery URL (static page in the repo's GitHub
  Pages for now).

## 11. Example widget + developer workflow

`examples/widgets/weather/` in-repo — the canonical, fully-featured reference:
network permission with domain allowlist, `location` setting usable globally or
per-instance (home/office wedges), timer + menu-open refresh, scroll to cycle
forecast days, click to open the forecast page, haptic tick on scroll, tile
template for the main view + free-form drawing for the forecast strip.

Developer workflow:

```bash
cargo generate oxidemx/widget-template     # or copy examples/widgets/weather
cargo build --release --target wasm32-wasip1
oxidemx-widget pack                        # tiny cargo xtask/cli: validates
                                           # manifest, bundles .omxwidget
oxidemx-widget install ./weather-1.0.0.omxwidget   # or drag into settings app
```

A `just dev-widget` loop rebuilds + reinstalls + lets the overlay's dir-watch
hot-reload the instance (state is reset on reload — acceptable for v1).

## 12. Testing

- **proto crate:** round-trip serde tests incl. unknown-variant forward-compat
  (encode with a "future" enum, decode with current).
- **widget-host:** unit tests with a fixture widget compiled in CI to
  wasm32-wasip1 — lifecycle order, fuel exhaustion → three-strikes disable,
  timer clamping, settings resolution order, HTTP cache dedupe, scene size cap.
- **example weather widget:** guest-side logic tests run natively (the `Widget`
  trait is plain Rust; `Ctx` is mockable).
- **settings-rs:** state-machine tests for selector-grid messages and
  scope-toggle write paths (existing message-dispatch test style).
- **Manual/visual:** `just dev-widget` + the overlay's existing visual test
  flow; placeholder-wedge rendering verified by installing a deliberately
  trapping fixture widget.

## 13. Build order (for the implementation plan)

1. `oxidemx-widget-proto` (types) → 2. `oxidemx-widget-host` (wasmi runtime,
registry, no UI) → 3. shared-config additions → 4. overlay integration (worker,
scene replay, events) → 5. `oxidemx-widget-api` PDK + weather example + pack
CLI → 6. settings-rs selector grid → 7. options form + scope toggle →
8. manager page + install flows → 9. docs (`docs/widgets/authoring.md`).
