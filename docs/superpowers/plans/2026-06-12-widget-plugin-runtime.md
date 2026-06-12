# Widget Plugin Runtime Implementation Plan (Plan 2 of 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The live widget runtime — wasmi host, developer PDK, the weather example widget, pack/install CLI, and overlay integration so a `Custom` widget actually renders and reacts inside the radial menu.

**Architecture:** `oxidemx-widget-host` owns wasmi instances (one per placed widget instance), a registry over `~/.config/oxidemx/widgets/`, timers, and a permission-gated HTTP fetcher, all on a dedicated tokio worker that exchanges `WidgetHostMsg`/`WidgetUiMsg` with the iced loop over `async_channel` (same pattern as `overlay-rs/src/ai_client.rs`). The frame path never calls wasm: the painter replays the last decoded `Scene` per instance. `oxidemx-widget-api` is the guest PDK wrapping the raw ABI. Spec: `docs/superpowers/specs/2026-06-12-widget-plugin-system-design.md` §3, §8 (ABI + events + scene), §9 (errors), and resource caps in §8.

**Tech Stack:** wasmi (latest 1.x / newest on crates.io, `fuel` support) + wasmi_wasi for WASI-p1 shims, postcard via `oxidemx-widget-proto`, tokio + async_channel + reqwest (already overlay deps), ed25519-dalek 2 for bundle signatures, zip 2.x for `.omxw`.

**Worktree:** implement in `/run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-widgets-wt` (branch `widget-plugins`). Run all commands from that root. `rustup target add wasm32-wasip1` once (Task 2).

**Plan style note:** Tasks 1–5 are TDD with concrete test specifications (test code given; adapt names/imports as the compiler requires). Task 6 (overlay) is integration work verified by build + a scripted smoke run; its inner steps are behavioral requirements, not line-by-line code — read the named existing files before writing. Commit after every task with the message given.

---

### Task 1: Host crate skeleton + manifest registry

**Files:**
- Create: `oxidemx-widget-host/Cargo.toml`, `oxidemx-widget-host/src/lib.rs`, `oxidemx-widget-host/src/registry.rs`
- Modify: root `Cargo.toml` members

**Cargo.toml** (start minimal; later tasks add deps):

```toml
[package]
name = "oxidemx-widget-host"
version = "0.1.0"
edition = "2021"

[dependencies]
oxidemx-widget-proto = { path = "../oxidemx-widget-proto" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
log = "0.4"
```

**registry.rs** — filesystem scanner, no wasm yet:

```rust
pub struct InstalledWidget {
    pub manifest: WidgetManifest,          // from oxidemx-widget-proto
    pub dir: PathBuf,                      // ~/.config/oxidemx/widgets/<id>/
    pub state: WidgetState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WidgetState {
    Ready,
    Incompatible { reason: String },       // bad manifest / api_version range
}

pub struct WidgetRegistry { widgets: BTreeMap<String, InstalledWidget> }

impl WidgetRegistry {
    /// Scan `dir` for `<id>/widget.json`. Bad manifests yield
    /// `Incompatible`, never errors; dir id must match manifest.id.
    pub fn scan(dir: &Path) -> Self;
    pub fn get(&self, id: &str) -> Option<&InstalledWidget>;
    pub fn iter_ready(&self) -> impl Iterator<Item = &InstalledWidget>;
    pub fn widgets_dir() -> Option<PathBuf>;   // ~/.config/oxidemx/widgets
}
```

Validation per spec §4/§5: manifest parses, `manifest.validate()` passes, `api_version == oxidemx_widget_proto::API_VERSION`, `entry`/`icon` files exist in the dir. Each failure → `Incompatible { reason }` with a human-readable reason (the settings UI shows it verbatim in Plan 3).

- [ ] Tests first (`registry.rs` bottom; build fixture dirs under `std::env::temp_dir()` like `oxidemx-shared/src/migrate.rs` tests do):
  - `scan_finds_valid_widget` — write widget.json + empty widget.wasm + icon.svg, scan, assert Ready + fields
  - `bad_api_version_is_incompatible` — api_version 99 → Incompatible, reason mentions version
  - `missing_entry_file_is_incompatible`
  - `id_mismatch_is_incompatible` — dir `foo/` with manifest id `bar`
  - `unparseable_manifest_is_incompatible_not_fatal` — garbage JSON in one dir doesn't poison scanning the others
- [ ] Red run → implement → `cargo test -p oxidemx-widget-host` green
- [ ] Commit: `feat(widget-host): manifest registry over the widgets install dir`

---

### Task 2: Fixture guest widget + wasmi instance lifecycle

**Files:**
- Create: `oxidemx-widget-host/fixtures/strike-widget/` (tiny Rust crate, `crate-type = ["cdylib"]`, target wasm32-wasip1, NOT a workspace member — add `fixtures` to workspace `exclude`)
- Create: `oxidemx-widget-host/src/instance.rs`
- Create: `oxidemx-widget-host/tests/instance.rs` (integration tests; build fixture via `std::process::Command` cargo invocation, cached)
- Modify: `oxidemx-widget-host/Cargo.toml` (+ `wasmi` with fuel, `wasmi_wasi` if needed for WASI imports)

**Fixture widget** implements the raw ABI directly (NO PDK yet — the PDK comes in Task 3 and gets its own fixture): exports `omx_api_version() -> u32`, `omx_alloc(len: u32) -> u32`, `omx_init(ptr: u32, len: u32)`, `omx_event(ptr: u32, len: u32) -> u32`, `omx_render(ptr: u32, len: u32) -> u64` (packed `ptr << 32 | len`), imports `omx_cmd(ptr: u32, len: u32)` from module `"oxidemx"`. Behavior, switchable via the settings bag passed to init (`{"mode": "ok" | "spin" | "huge_scene" | "trap"}`):
- `ok`: event returns 1 (needs render); render returns a 2-prim Scene
- `spin`: event loops forever (fuel exhaustion test)
- `huge_scene`: render returns > MAX_SCENE_PRIMS prims
- `trap`: event executes `unreachable!()`
- every `init` also issues one `omx_cmd` carrying `HostCmd::Log` (proves import wiring)

**instance.rs**:

```rust
pub struct WidgetInstance { /* wasmi Store/Instance/Memory, strikes: u8, disabled: bool */ }

pub const EVENT_FUEL: u64 = 5_000_000;    // ~ms-scale; tune once measured
pub const RENDER_FUEL: u64 = 2_000_000;
pub const MAX_STRIKES: u8 = 3;

pub enum CallOutcome {
    NeedsRender(bool),
    Scene(Scene),
    Disabled,                              // strikes exhausted
    Skipped,                               // instance already disabled
}

impl WidgetInstance {
    /// Instantiate `widget.wasm`, check omx_api_version, link omx_cmd
    /// (pushes decoded HostCmds into an out-queue the caller drains),
    /// call omx_init with the postcard-encoded settings.
    pub fn load(wasm_path: &Path, settings: &Settings) -> Result<Self, InstanceError>;
    pub fn on_event(&mut self, ev: &Event) -> CallOutcome;       // refuels per call
    pub fn render(&mut self, geom: &WedgeGeom) -> CallOutcome;   // decode + Scene::validate
    pub fn drain_cmds(&mut self) -> Vec<HostCmd>;
    pub fn is_disabled(&self) -> bool;
    pub fn last_error(&self) -> Option<&str>;
}
```

Strike rules (spec §8/§9): trap, fuel exhaustion, scene decode failure, or `Scene::validate` failure each = 1 strike; at `MAX_STRIKES` the instance flips `disabled` permanently for the session; every later call returns `Skipped`. A successful call does NOT reset the count. Encoded-scene byte cap: reject render results whose returned `len > MAX_SCENE_BYTES` (counts a strike) — this is where the byte cap from proto is enforced.

wasmi notes for the implementer: use `Engine::new(Config::default().consume_fuel(true))`, `store.set_fuel(...)` before each call (API name may be `set_fuel`/`add_fuel` depending on version — check docs.rs for the resolved version); guest memory access through the exported `memory`; host-side writes go through `omx_alloc`. If the wasip1 build demands WASI imports, link `wasmi_wasi::add_to_linker` with an empty-inherit ctx; if the cdylib fixture links clean without it, skip the dep and note it.

- [ ] Integration tests (`tests/instance.rs`; helper builds the fixture once via `cargo build --release --target wasm32-wasip1 --manifest-path fixtures/strike-widget/Cargo.toml`, skip-with-message if the target isn't installed):
  - `lifecycle_ok` — load(ok) → init drains one Log cmd → on_event(Click) = NeedsRender(true) → render = Scene with 2 prims
  - `fuel_exhaustion_strikes_and_disables` — load(spin); 3 × on_event → first two are strikes (outcome Disabled only on the third), 4th call = Skipped
  - `trap_strikes` — load(trap), same 3-strike pattern
  - `oversized_scene_strikes` — load(huge_scene), render ×3 → disabled
  - `api_version_mismatch_fails_load` — fixture variant compiled with version 99 (make the fixture read a env/feature… simplest: second tiny fixture crate `wrong-version-widget` hardcoding 99)
- [ ] Red → implement → green (`cargo test -p oxidemx-widget-host`)
- [ ] Commit: `feat(widget-host): wasmi instance lifecycle — fuel, strikes, scene caps`

---

### Task 3: `oxidemx-widget-api` PDK

**Files:**
- Create: `oxidemx-widget-api/Cargo.toml` (workspace member; deps: `oxidemx-widget-proto` default-features=false, postcard)
- Create: `oxidemx-widget-api/src/lib.rs` (trait + Ctx + registry glue), `src/tile.rs` (template builder), `src/macros.rs` (`register_widget!`)

Public surface (spec §8 — keep exactly this; Plan 3's docs and the example depend on it):

```rust
pub trait Widget: Default {
    fn init(&mut self, ctx: &Ctx);
    fn on_event(&mut self, ev: Event, ctx: &Ctx) -> bool;   // true = needs render
    fn render(&self, geom: WedgeGeom) -> Scene;
}

pub struct Ctx { /* settings: Settings, cmd sink */ }
impl Ctx {
    pub fn setting_str(&self, key: &str) -> Option<&str>;
    pub fn setting_f64(&self, key: &str) -> Option<f64>;
    pub fn setting_u64(&self, key: &str) -> Option<u64>;
    pub fn setting_bool(&self, key: &str) -> Option<bool>;
    pub fn setting_location(&self, key: &str) -> Option<(String, f64, f64)>;
    pub fn set_timer(&self, id: &str, secs: u64);
    pub fn cancel_timer(&self, id: &str);
    pub fn http_get(&self, id: &str, url: &str);
    pub fn open_url(&self, url: &str);
    pub fn exec(&self, command: &str);
    pub fn haptic(&self, pattern: &str);
    pub fn log(&self, msg: &str);
}

pub fn tile() -> TileBuilder;      // .value(str) .sublabel(str) .label(str)
                                   // .sparkline(&[f32]) .color(palette_key)
                                   // .into_scene(geom) -> Scene
```

`register_widget!(MyWidget);` expands to the `omx_*` extern fns: a `static mut` (or `OnceCell`) instance, postcard decode of init settings/events via `decode_event` (unknown → return 0, no render), encode render output into a guest-owned buffer whose ptr/len pack into the u64. `Ctx` queues `HostCmd`s and flushes each through the `omx_cmd` import. The crate must compile for `wasm32-wasip1` AND natively (tests run native; the extern glue is `#[cfg(target_arch = "wasm32")]`).

`tile()` reproduces the built-in wedge typography (see `overlay-rs/src/render/slices/widgets.rs` draw_widget_wedge): 18pt bold value at the icon anchor, optional sparkline below, 8.5pt sublabel, 8pt semibold uppercase label — as `Prim::Text`/`Prim::Sparkline` with `Color::Palette` defaulting to the slice color key `"accent"` unless `.color()` given.

- [ ] Tests (native, in-crate): `tile_builder_produces_expected_prims` (order + sizes + uppercase label), `ctx_setting_accessors` (build Ctx from a fixture Settings vec incl. Location), `event_decode_unknown_returns_no_render` (feed an Envelope with bogus tag through the same decode path the macro uses)
- [ ] Red → implement → green; also `cargo check -p oxidemx-widget-api --target wasm32-wasip1`
- [ ] Replace Task 2's raw-ABI fixture internals with PDK usage? **No** — keep the raw fixture (it pins the ABI independently of the PDK). Instead add `fixtures/pdk-widget/` (uses the PDK, mode-switchable ok-behavior only) and one host integration test `pdk_widget_round_trips` proving PDK-built widgets load in the host.
- [ ] Commit: `feat(widget-api): guest PDK — Widget trait, Ctx, register_widget!, tile builder`

---

### Task 4: Weather example widget

**Files:**
- Create: `examples/widgets/weather/Cargo.toml` (NOT workspace member; path-dep on oxidemx-widget-api), `src/lib.rs`, `widget.json`, `icon.svg` (any simple sun svg), `README.md` (build+pack+install in ~15 lines)

Manifest: exactly the spec §5 example (id `weather`, api_version 1, permissions `net:api.open-meteo.com` + `net:geocoding-api.open-meteo.com` + `open-url` + `haptics`, refresh_ms 900000, options location/units/display/refresh as spec'd).

Behavior (spec §12): init → set refresh timer from `refresh` setting + fire `http_get("wx", open-meteo URL from location lat/lon)`; Timer→refetch; HttpResponse→parse `current_weather` JSON (hand-rolled with serde_json? NO — guest has no serde_json: parse the two needed numbers with a tiny string scan, or add `serde-json-core`; pick the lighter and note it); MenuOpened→refetch if stale (track last-fetch via a counter the host's Timer events advance — simplest: refetch unconditionally on MenuOpened if no data); Scroll→cycle `day` 0..=2 + `haptic("tick")`; Click→`open_url("https://open-meteo.com/")`; SettingsChanged→refetch. Render: `tile().value("{t:.0}°").sublabel(condition).label("WEATHER")` with WMO code→label table copied from `overlay-rs/src/sampler.rs` (lines ~292-305).

- [ ] Native unit tests in the crate: WMO mapping, response parsing from a canned JSON string, day-cycling wraps, °F conversion when `units == "f"`
- [ ] `cargo build --release --target wasm32-wasip1` from the example dir produces `weather.wasm`; host integration test `weather_widget_loads` (load with a settings bag incl. location, assert init drains an HttpGet cmd whose url contains the lat)
- [ ] Commit: `feat(examples): weather widget — canonical PDK example`

---

### Task 5: Host worker — timers, HTTP, settings resolution, event routing

**Files:**
- Create: `oxidemx-widget-host/src/worker.rs`, `oxidemx-widget-host/src/http.rs`
- Modify: `oxidemx-widget-host/Cargo.toml` (+ tokio rt, async_channel, reqwest default-tls — match overlay-rs versions)

```rust
/// UI → worker
pub enum HostCtl {
    ConfigChanged(Arc<AppConfig>),           // re-derive desired instances
    RescanWidgets,                            // install dir changed
    Slice { instance: InstanceId, ev: SliceEvent },  // Click/Scroll/Hover
    MenuOpened { page: String },
    MenuClosed,
}
pub enum SliceEvent { Click, Scroll(f32), Hover(bool) }

/// worker → UI
pub enum HostEvent {
    Scene { instance: InstanceId, scene: Scene, revision: u64 },
    InstanceFailed { instance: InstanceId, error: String },
    RegistryChanged(Vec<WidgetSummary>),      // for Plan 3's picker
}

/// `<page-slug>.slot<N>` ↔ widget id pair
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct InstanceId { pub instance_key: String, pub widget_id: String }

pub fn spawn(ctl: async_channel::Receiver<HostCtl>,
             events: async_channel::Sender<HostEvent>) -> tokio::task::JoinHandle<()>;
```

Worker responsibilities (all spec §8):
- **Desired-state reconcile** on ConfigChanged: walk `cfg.radial_menu` pages/slices for `WidgetSource::Custom(id)` + `instance_key`; load/drop `WidgetInstance`s to match; resolve settings via `cfg.widgets.resolve(id, key, scope, manifest.defaults())` → postcard Settings (JSON→SettingValue conversion lives here: string/number/bool direct; `{name,lat,lon}` object → `Location`; everything else → skip + log); changed settings → `Event::SettingsChanged` to live instances.
- **Timers**: SetTimer cmds clamp to `manifest.effective_refresh_ms()/1000` min; tokio interval per (instance, timer-id); suspended while the widget's page set is empty; while menu closed, a timer fires at most once (latch reset on MenuOpened).
- **HTTP**: HttpGet cmd → permission check (`net:<host>` allowlist from the manifest — exact host match on the URL's host_str; scheme must be https) → shared `HttpCache` (`http.rs`: `get(url) -> CacheResult`, caches body+status keyed by url for the widget's effective refresh interval; in-flight dedup via a pending map) → `Event::HttpResponse` back to the instance. Denied → immediate `HttpResponse { status: 0, body: reason }`.
- **Other cmds**: OpenUrl → `xdg-open` spawn if manifest has `open-url`; Exec → spawn via `sh -c` if manifest has `exec`; HapticPulse → no-op stub with a `// TODO Plan 3: route to daemon haptic_client` comment + log (the overlay wires it in Task 6 only if trivial); Log → `log::info!(target: "widget", ...)`.
- **Render loop**: after any event where on_event returned true, call render with the instance's last-known WedgeGeom (UI sends geom inside Slice events; default 200×160 wedge before first hover), validate, bump revision, emit `HostEvent::Scene`.

- [ ] Tests (tokio multi-thread, fixture widgets from Tasks 2-4):
  - `reconcile_spawns_and_drops_instances` — feed a synthetic AppConfig with one Custom slice; expect one Scene event; remove slice; expect instance gone (probe via a follow-up Slice event producing no Scene)
  - `http_permission_denied` — fixture pdk widget requests a non-allowlisted host → HttpResponse status 0 (observable: fixture re-renders the error into its scene text; assert on scene)
  - `http_cache_dedupes` — two instances of the same widget; count actual upstream hits via a `HttpFetcher` trait test-double (worker takes `Box<dyn HttpFetcher>`; prod impl = reqwest)
  - `timer_clamps_to_refresh_floor` — fixture sets a 1s timer, manifest floor 5s → no Timer event within ~2s (use tokio::time::pause/advance)
  - `settings_resolution_reaches_widget` — instance bag overrides global; fixture echoes a setting into its scene
- [ ] Red → implement → green
- [ ] Commit: `feat(widget-host): worker — reconcile, timers, permission-gated http cache`

---### Task 6: Overlay integration

**Files (read each before editing):**
- Modify: `overlay-rs/Cargo.toml` (+ oxidemx-widget-host path dep)
- Modify: `overlay-rs/src/app/subscriptions.rs` (host event stream — copy the `ai_stream_stream` pattern at lines ~66-79)
- Modify: `overlay-rs/src/app/update.rs` (new `Message::WidgetHost(HostEvent)` arm; forward MenuOpened/MenuClosed from existing show/hide transitions; forward config reloads — the watcher already delivers `AppConfig`)
- Modify: `overlay-rs/src/radial/mod.rs` or state container (scene store: `HashMap<InstanceId, (Scene, u64)>`)
- Modify: `overlay-rs/src/render/slices/widgets.rs` (replay path), `render/slices/ring.rs` (dispatch `Custom` to it)
- Modify: `overlay-rs/src/actions.rs` + the painter's pointer handling (route Click/Scroll/Hover on Custom-widget slices to `HostCtl::Slice` with current `WedgeGeom`)
- Modify: `overlay-rs/src/config.rs` watcher (also watch the widgets dir → `HostCtl::RescanWidgets`)

Behavioral requirements:
1. Worker spawned once at app start; channels stored in app state; subscription yields `Message::WidgetHost`.
2. `HostEvent::Scene` updates the scene store and requests redraw (existing cache_epsilon mechanism — bump the same per-frame epsilon source the canvas cache uses; see memory note: do NOT remove cache_epsilon).
3. **Scene replay** (`widgets.rs`): new `fn draw_custom_widget(frame, scene, geom, palette, hover)` — translate to the icon anchor, scale by geom, walk prims: Path/Arc via canvas Path builder, Text via `draw_centered_text` conventions, Sparkline like the CPU sparkline, Image: look up pre-decoded `iced::widget::image::Handle`-equivalent — if image rendering inside canvas Frame isn't already available, render a placeholder rect + log once (note it as a Plan 3 follow-up; do NOT build an image pipeline now). `Color::Palette(key)` resolves through the same theme lookup `draw_slice` uses; unknown key → slice color. Unknown `Prim` variant (non_exhaustive wildcard) → log once per session.
4. Disabled/missing instance → fallback wedge: manifest `fallback_icon` via IconCache, dimmed, ⚠ char badge (reuse the icon+caption layout from `draw_slice`'s placeholder path).
5. Click on a Custom widget slice: `actions.rs dispatch` gains a `WidgetSource::Custom` arm sending `HostCtl::Slice{Click}` (non-blocking try_send). Scroll over the slice (where Dial slices hook `adjust_dial`) sends `Scroll(delta)`. Hover enter/leave from the painter's existing hover tracking sends `Hover(bool)` with the live `WedgeGeom{hovered}`.
6. Menu open/close transitions (wherever the overlay decides show/hide — find the state machine in `radial/mod.rs`/`update.rs`) send MenuOpened{page}/MenuClosed.

- [x] `cargo check -p oxidemx-overlay` then `cargo build -p oxidemx-overlay`
- [x] Smoke script `scripts/widget-smoke.sh`: builds the weather example, installs it into a temp XDG_CONFIG_HOME with a config.json placing it on slot 4 (set `weather.location` global bag inline), runs the overlay binary headless-checked — if the overlay can't run headless, the script just asserts the worker boots and a Scene event arrives via a `--widget-smoke` debug flag that prints scene revision to stdout and exits (add the flag behind `cfg(debug_assertions)`).
- [x] Commit: `feat(overlay): custom widget slices — host worker, scene replay, event routing`

---

### Task 7: `oxidemx-widget` pack/install CLI + signatures

**Files:**
- Create: `tools/oxidemx-widget-cli/` (workspace member; bin name `oxidemx-widget`; deps: clap 4 derive, zip 2, ed25519-dalek 2 + rand_core, oxidemx-widget-proto, serde_json)
- Modify: `oxidemx-widget-host/src/registry.rs` (+ signature check on scan)

Commands:
- `pack <dir> [--sign dev|--key <path>] [-o out.omxw]` — validate manifest (`validate()` + api_version + entry/icon exist), zip dir contents deterministically (sorted paths), sign: ed25519 over the zip bytes, detached `SIGNATURE` file appended INSIDE the zip as last entry (sign-then-embed: signature covers all other entries' bytes via signing the concatenation of (path, bytes) pairs sorted — implement `bundle_digest(zip) -> Vec<u8>` shared with verify). `--sign dev` generates/reuses `~/.config/oxidemx/dev-signing.key`.
- `verify <bundle.omxw> [--pubkey <path>]` — recompute digest, check SIGNATURE against the pinned registry pubkey (compiled-in const, placeholder key for now) or `--pubkey`; report signer fingerprint (hex of pubkey first 8 bytes).
- `install <bundle.omxw> [--force]` — verify (unknown key → print fingerprint + require `--force`, matching spec §4's "prompt" semantics for CLI), unpack to `widgets_dir()/<id>/`, refuse on id collision without `--force`.

Registry change: `scan` verifies `<dir>/SIGNATURE` if present → `signed: SignatureState` field on `InstalledWidget` (`Registry { Pinned | Unknown(String fingerprint) | Unsigned }`) — informational only (Plan 3 displays it; unsigned widgets still Ready since the user consented at install time).

- [ ] Tests: round-trip `pack` → `verify` OK with dev key; tampered byte → verify fails; `install` places files + registry scan shows `Unknown` fingerprint state; id collision refused
- [ ] Commit: `feat(widget-cli): pack/verify/install .omxw bundles with ed25519 signatures`

---

### Task 8: Hygiene + authoring doc

- [ ] `cargo test --workspace` (worktree) — note pre-existing iced_gtk_themer doctest failures, everything else green incl. new crates
- [ ] `cargo clippy -p oxidemx-widget-host -p oxidemx-widget-api -p oxidemx-widget-cli -- -D warnings`
- [ ] Write `docs/widgets/authoring.md` (~120 lines): quickstart (copy weather example), the Widget trait lifecycle table, Ctx reference, tile() vs free-form Scene, settings/options schema, permissions list, pack/install commands, the append-only wire rule, resource caps (2ms render / 5s refresh floor / 64KB scene). Source every statement from the spec + the actual PDK code — no aspirational features.
- [ ] Commit: `docs(widgets): authoring guide; chore: clippy sweep`

---

## Self-review notes

- Spec §15 coverage check: step 3 (host) = Tasks 1/2/5, step 4 (overlay) = Task 6, step 5 (PDK + example + CLI) = Tasks 3/4/7, docs = Task 8. Signature verify (spec §4) = Task 7. Haptic routing left stubbed in worker (logged), wired later — acceptable: spec lists haptics as permission, daemon D-Bus client lives in overlay, full route needs Plan 3's testing pass.
- Image prims render as placeholder in Task 6 (canvas image support uncertain in iced 0.14 Frame) — explicitly carried to Plan 3 follow-ups rather than silently dropped.
- Type consistency: `InstanceId{instance_key, widget_id}` matches Plan 1's `instance_key()` format; fuel constants/EVENT vs RENDER match spec's 5ms/2ms intent; `SignatureState` naming consistent between CLI and registry tasks.
