# OxideMX Widget Authoring Guide

Widgets are WebAssembly modules that draw live data into radial-menu
slices.  They run in a wasmi sandbox, receive events over a postcard
wire, and return a retained display list (Scene) that the overlay
replays every frame without calling guest code.

---

## Quickstart

Copy the canonical example and adapt it:

```bash
cp -r examples/widgets/weather my-widget
cd my-widget
# edit src/lib.rs, widget.json
cargo build --release --target wasm32-wasip1
oxidemx-widget pack              # validates + zips + dev-key-signs
oxidemx-widget install ./my-widget-0.1.0.oxw
# the overlay hot-registers it; place it from Settings › Menu
```

The weather example lives at `examples/widgets/weather/` and is the
authoritative reference for every feature described in this guide.

---

## Widget trait lifecycle

Implement `oxidemx_widget_api::Widget` and call
`register_widget!(YourType)` at the crate root.  The host calls the
three methods below in order; all run on the worker thread, never on
the frame path.

| Method | When called | Typical use |
|---|---|---|
| `init(&mut self, ctx: &Ctx)` | Once, after the wasm module loads. Settings are already resolved. | Read settings, call `ctx.set_timer`, issue the first `ctx.http_get`. |
| `on_event(&mut self, ev: Event, ctx: &Ctx) -> bool` | For every inbound event. Return `true` if the visual state changed and `render` should be called. | Match on the event variant; update fields; issue follow-up commands. |
| `render(&self, geom: WedgeGeom) -> Scene` | Only when `on_event` returned `true`. | Build and return a `Scene`; `tile()` covers most cases. |

### Events that fire (v1)

All variants are `#[non_exhaustive]`; unknown variants arrive as
`Event::Unknown` — always include a `_ => false` arm.

| Event | When |
|---|---|
| `Event::Timer(id)` | A timer you registered with `ctx.set_timer` fired. |
| `Event::MenuOpened { page }` | The radial menu opened on the given page. Use this to re-fetch stale data. |
| `Event::MenuClosed` | The menu closed. |
| `Event::Hover { entering }` | The user's cursor entered (`true`) or left (`false`) this slice. |
| `Event::Click` | The user clicked this slice. |
| `Event::Scroll { delta }` | The user scrolled the wheel over this slice. Positive = scroll down. |
| `Event::SettingsChanged` | The user edited an option in the Settings app. Re-read `ctx.setting_*` and re-fetch if needed. |
| `Event::HttpResponse { id, status, body }` | Response to a `ctx.http_get` call. `id` matches what you passed; `status == 0` means the request was denied or failed (network or permission). |

---

## Ctx method reference

`Ctx` is the handle passed to every `Widget` call.

### Settings accessors

Settings are resolved from manifest defaults → global user bag →
instance bag (see §6 of the spec).  The widget never knows which layer
a value came from.

| Method | Returns | Notes |
|---|---|---|
| `setting_str(key)` | `Option<&str>` | |
| `setting_f64(key)` | `Option<f64>` | |
| `setting_u64(key)` | `Option<u64>` | Truncates via `as u64` |
| `setting_bool(key)` | `Option<bool>` | |
| `setting_location(key)` | `Option<(String, f64, f64)>` | Returns `(name, lat, lon)` |

### Host commands (fire-and-forget)

| Method | Permission required | Notes |
|---|---|---|
| `set_timer(id, secs)` | none | Period is clamped to max(manifest `refresh_ms` / 1000, 1) by the host. The 5 s floor applies globally (spec §8). |
| `cancel_timer(id)` | none | |
| `http_get(id, url)` | `net:<host>` | HTTPS only. Host deduplicates concurrent identical requests and caches for the refresh interval. |
| `open_url(url)` | `open-url` | Calls `xdg-open`. |
| `exec(command)` | `exec` | Runs via `sh -c`. |
| `haptic(pattern)` | `haptics` | Stub in v1; routed to daemon in Plan 3. |
| `log(msg)` | none | Appears in the overlay log at the `info` level. |

---

## `tile()` vs free-form Scene

### `tile()` — the template helper

`tile()` reproduces the built-in wedge typography from the overlay's
`draw_widget_wedge`: big value (18pt bold), optional sparkline, 8.5pt
regular sublabel, 8pt semibold UPPERCASE label.  The y-offsets match
exactly, so plugin wedges look identical to built-in ones.

```rust
fn render(&self, geom: WedgeGeom) -> Scene {
    tile()
        .value("14°")           // main reading — 18pt bold
        .sparkline(&[0.2, 0.5]) // normalised 0.0..=1.0; omit if unused
        .sublabel("Partly cloudy")
        .label("WEATHER")       // auto-uppercased
        .into_scene(geom)
}
```

`into_scene` returns a `Scene` with `prims` in order:
`[Text(value), Sparkline?, Text(sublabel)?, Text(label)?]`.

### Free-form Scene

Build `Scene { prims: vec![…] }` directly for custom layouts.  All
coordinates are **wedge-local**: origin at the icon anchor point, +y
down, matching the `WedgeGeom` dimensions the host passes to `render`.

Available primitives (`oxidemx_widget_proto::Prim`):

| Variant | Purpose |
|---|---|
| `Text { x, y, content, size, color, weight, align }` | Canvas text. Width approximated at 0.55 em/char by the renderer. |
| `Arc { cx, cy, radius, start_angle, end_angle, stroke, fill }` | Circular arc. |
| `Path { ops, stroke, fill }` | Arbitrary path (MoveTo/LineTo/QuadTo/CubicTo/Close). |
| `Sparkline { x, y, w, h, points, color }` | Polyline chart; `points` are normalised 0.0..=1.0 samples. |
| `Image { x, y, w, h, asset }` | **Placeholder only in v1** — renders as a tinted rect with a one-time log warning. Plan 3 follow-up. |
| `Group { dx, dy, scale, children }` | Translate + uniform scale a subtree. |

Colors are `oxidemx_widget_proto::Color::Rgba(r, g, b, a)` (u8 each) or
`Color::Palette(key)` where key is a Catppuccin token (`"text"`,
`"subtext0"`, `"accent"`, `"red"`, etc.).  `"accent"` resolves to the
slice's configured colour — the default for `tile()`.

**Append-only wire rule:** `Prim`, `Color`, `PathOp`, `TextWeight`, and
`TextAlign` are postcard-encoded by declaration index.  They are
strictly append-only within an `api_version`.  Never reorder variants.
Adding a new variant requires bumping `api_version`.

---

## Manifest (`widget.json`)

```jsonc
{
  "id": "weather",               // slug [a-z0-9.-]+, unique per registry
  "name": "Weather",
  "version": "1.4.0",
  "author": "JuhLabs",
  "api_version": 1,              // must match oxidemx_widget_proto::API_VERSION
  "entry": "widget.wasm",
  "icon": "icon.svg",
  "permissions": [
    "net:api.open-meteo.com",    // HTTPS-only, exact host (case-insensitive)
    "open-url",
    "haptics"
  ],
  "slice": {
    "refresh_ms": 900000,        // floor: 5000 ms
    "fallback_icon": "weather-clear-symbolic"
  },
  "options": [
    { "key": "location", "type": "location", "label": "Location", "required": true },
    { "key": "units", "type": "enum", "values": ["c", "f"], "default": "c",
      "label": "Units" },
    { "key": "refresh", "type": "select", "values": [300, 900, 1800],
      "default": 900, "label": "Refresh", "unit": "s" }
  ]
}
```

### Permissions list

| Permission string | What it allows |
|---|---|
| `net:<host>` | HTTPS GET to exactly `<host>` (case-insensitive, no subdomains). |
| `open-url` | `xdg-open <url>` from `ctx.open_url`. |
| `exec` | `sh -c <cmd>` from `ctx.exec`.  Shows a strong install-time prompt. |
| `haptics` | `ctx.haptic` pattern delivery (stub in v1). |

### Settings resolution order

```
manifest defaults  →  global bag (widgets.global[id])
                   →  instance bag (widgets.instances[key][id])  // scope = Instance only
```

The global bag is shared by every instance of the widget.  The instance
bag is only written and read when the slice's scope is `"instance"`;
when scope is `"global"` the third step is skipped and the options card
writes to the global bag.  `ctx.setting_*` always sees the fully merged
result — the widget is unaware of the layer.

---

## Pack, verify, install

All three commands are provided by `oxidemx-widget` (alias `oxidemx-widget-cli`).

```bash
# Pack: validates manifest, zips all files, signs with your dev key.
# Dev key lives at ~/.config/oxidemx/dev-signing.key (generated on first use).
# Output: <dir_name>.oxw  (override with --out path)
oxidemx-widget pack [--out my-widget-0.1.0.oxw] <widget-dir>

# Verify a .oxw bundle's SIGNATURE without installing.
# Prints: Signed (pinned|unknown fingerprint) or Unsigned.
oxidemx-widget verify my-widget-0.1.0.oxw

# Install to ~/.config/oxidemx/widgets/<id>/.
# Verifies signature first.  Unknown (dev) key: proceeds with a log line.
# Use --force to replace an already-installed id.
oxidemx-widget install [--force] my-widget-0.1.0.oxw
```

After install the dir-watcher triggers a hot-register; the overlay
picks the widget up without a restart.

---

## Resource caps and the 3-strikes rule

The host enforces these limits on every call.  Exceeding them counts
as one strike.  Three strikes disable the instance for the session; the
slice shows a fallback wedge (dimmed `fallback_icon` + ⚠ badge).

| Resource | Cap |
|---|---|
| `omx_event` fuel | `EVENT_FUEL = 5_000_000` wasmi units (~5 ms intent) |
| `omx_render` fuel | `RENDER_FUEL = 2_000_000` wasmi units (~2 ms intent) |
| Scene byte size | `MAX_SCENE_BYTES = 64 * 1024` (64 KB encoded postcard) |
| Scene prim count | `MAX_SCENE_PRIMS = 2048` (counted recursively through Groups) |
| Timer floor | `refresh_ms` from manifest, minimum 5 000 ms.  Also the HTTP cache TTL. |
| Closed-menu timer fires | At most once per close period per timer (latch resets on `MenuOpened`). |

A `SettingsChanged` event triggers an instance reload (settings diff
→ drop + re-init).  State resets on reload; this is acceptable in v1.

---

## Developing locally

```bash
# Hot-reload loop: rebuild wasm, replace the installed file, overlay
# dir-watch fires → reconcile → wasm reload.
just dev-widget        # or the equivalent cargo + cp sequence

# Run widget unit tests natively (Ctx is mockable; no wasm target needed).
cargo test --manifest-path examples/widgets/weather/Cargo.toml
```

The weather example has full native unit tests for JSON parsing,
settings accessors, event handling, render output shape, and °F
conversion — use it as a template for your own test suite.
