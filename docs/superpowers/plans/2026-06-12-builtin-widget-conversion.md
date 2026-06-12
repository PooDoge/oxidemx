# Built-in Widget Conversion Implementation Plan (Plan 4)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Spec: `docs/superpowers/specs/2026-06-12-widget-plugin-system-design.md` **§16** (authoritative for this plan) + §8.

**Goal:** Convert weather/cpu/memory/network/disk/tasks-due built-ins to bundled plugin widgets fed by a host-pushed `SystemStats` event; seed them at overlay startup; swap the picker to prefer them; keep native rendering for legacy configs. MouseBattery stays native.

**Worktree:** `/run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-builtins-wt`, branch `widget-builtins`. After `git worktree add`, symlink vendored dirs (per docs/notes/multi-session-coordination.md): `ln -s ../juhradial-mx/pop_os_iced pop_os_iced; ln -s ../juhradial-mx/libcosmic libcosmic` (remove the empty gitlink dirs first if present; do NOT commit the symlinks — verify `git status` stays clean).

### Task 1: proto — SystemStats event (APPEND-ONLY)
`oxidemx-widget-proto/src/event.rs`: append `Event::SystemStats(SystemStatsSnapshot)` as the LAST variant; new `#[non_exhaustive] #[derive(Default…)] pub struct SystemStatsSnapshot` with `pub cpu_pct: Option<f32>, cpu_cores: Option<u32>, cpu_temp_c: Option<f32>, mem_used_gb: Option<f32>, mem_total_gb: Option<f32>, net_down_mbps: Option<f32>, net_up_mbps: Option<f32>, disk_free_gb: Option<f32>, tasks_due: Option<u32>, battery_pct: Option<u8>, battery_charging: Option<bool>` (struct fields are append-only too — comment it). Round-trip test incl. default-None snapshot. Do NOT bump API_VERSION (host-first rollout per the wire-rule comments). Commit: `feat(widget-proto): SystemStats event (append-only)`

### Task 2: host — stats sampler + menu-open push loop
- New `oxidemx-widget-host/src/stats.rs`: `trait StatsSource { fn sample(&mut self) -> SystemStatsSnapshot }` + `ProcStatsSource` factored from `overlay-rs/src/sampler.rs` logic (read it): /proc/stat delta (keep prev counters in the source), /proc/meminfo MemAvailable, /proc/net/dev deltas→Mb/s, disk free via `statvfs` on /home (use `nix` or parse `df` like sampler does — copy sampler's approach), tasks-due via the same systemd command sampler uses (spawned, cached 60s). battery_* stays None (native-only).
- `worker.rs`: hold `Box<dyn StatsSource>` (prod Proc, test double via a `spawn_with`-style injection — extend the existing test constructor); while menu open (existing latch state) a 1 s tokio interval samples once and pushes `Event::SystemStats` to every instance whose manifest permissions contain `"system-stats"`; also push one immediately on MenuOpened. No push while closed. needs_render handling identical to other events.
- Tests (fixture pdk-widget gains a `stats` mode echoing cpu_pct into its scene + the permission in its test manifest — keep existing modes intact): stats_pushed_while_menu_open (paused time), no_stats_while_closed, no_stats_without_permission.
- Commit: `feat(widget-host): system-stats sampler + menu-open push feed`

### Task 3: bundled guest widgets
New dir `widgets/builtin/` (workspace-EXCLUDED crates like examples/): `git mv examples/widgets/weather widgets/builtin/weather` (update its README + docs/widgets/authoring.md + scripts paths referencing examples/widgets/weather), then five new PDK crates `cpu`, `memory`, `network`, `disk`, `tasks`:
- Manifests: ids `cpu`/`memory`/`network`/`disk`/`tasks`, author "OxideMX", api_version 1, permission `["system-stats"]`, refresh irrelevant (push-fed) — omit slice.refresh_ms (default fine), fallback_icon per current built-ins. Options (small, per §5): cpu → `show_sparkline: boolean default true`; network → same; disk → `path: enum ["/home","/","/var"] default "/home"`… NO — disk path is host-sampled globally; give disk NO options v1; memory none; tasks none. Keep options minimal: only the two sparkline booleans.
- Behavior: on `SystemStats` update state (cpu/network keep `VecDeque<f32>` 30-sample history) → `true`; render via `tile()` reproducing TODAY'S typography exactly (read `draw_widget_wedge` in overlay-rs/src/render/slices/widgets.rs + oxidemx-scene-render: big value "23%"/"11.2"/"84↓"/"412"/"3", sublabels "8 cores · 52°C"/"of 32 GB"/"12↑ Mb/s"/"GB free"/"due in 24h", uppercase labels, sparkline when enabled). Colors: tile() default palette accent (slice color) — matches native behavior.
- Native unit tests per crate (value formatting, history cap). Shared formatting helpers may live in a tiny `widgets/builtin/common` path-dep crate IF it stays <100 lines, else duplicate.
- Build all: extend `tests/common/mod.rs` consumers? No — host integration test `builtin_widgets_load` (build cpu via build_wasm_at, load with stats perm manifest, push a SystemStats, assert scene contains "%" text).
- Commit: `feat(widgets): bundled builtin widgets — cpu/memory/network/disk/tasks + weather move`

### Task 4: packaging + startup seeding
- `scripts/build-builtin-widgets.sh`: builds each `widgets/builtin/*` to wasm + `oxidemx-widget pack` (dev key) → `target/builtin-widgets/<id>-<ver>.omxw`.
- Seeding in `oxidemx-widget-host` (new `seed.rs`, called by overlay at startup before first registry scan AND by settings-rs): search dirs in order: `$OXIDEMX_BUILTIN_WIDGETS_DIR`, `<exe>/../share/oxidemx/widgets`, each `$XDG_DATA_DIRS/oxidemx/widgets`, `~/.local/share/oxidemx/widgets`; for each `*.omxw`: parse manifest, install (reusing CLI lib's extraction — move/share the safe-extract fn if needed; force-overwrite ONLY when bundled version > installed version per semver-ish compare (split on '.', numeric compare), never touching ids not present in the seed dir). Unsigned/dev-signed bundles seed WITHOUT consent (they came from the install media, same trust as the binary — document this in seed.rs and authoring.md security section).
- Wire: overlay main + settings startup call `seed_builtin_widgets()` (log result). `packaging/` (read what's there — Makefile/install.sh): install `target/builtin-widgets/*.omxw` to `/usr/local/share/oxidemx/widgets/` (Bazzite: /usr/local, per repo conventions — check install.sh) — add to install script + Makefile target.
- Tests: seed_installs_new, seed_upgrades_older, seed_skips_newer_or_equal, seed_never_touches_foreign_ids (temp dirs).
- Commit: `feat(widget-host): builtin widget seeding + build/packaging scripts`

### Task 5: picker swap + convert-to-plugin
- settings-rs picker: builtin-tile mapping `WidgetSource::{Cpu→"cpu", Memory→"memory", Network→"network", Disk→"disk", TasksDue→"tasks", Weather→"weather"}`; when the mapped plugin id IS installed+ready, hide the canned native tile (the registry tile covers it); MouseBattery always shows its native tile. New picks therefore produce Custom widgets.
- Legacy slice affordance: when the selected slice is a NATIVE widget source whose plugin is installed, show a one-line hint row under the chip: "A plugin version of this widget is installed — Convert" → `Message::ConvertSliceToPlugin(idx)`: rewrites source→Custom(id), assigns instance_key, seeds instance bag from old native settings where they map (weather: lift overlay.weather_location/place/celsius → location/units; others: none), keeps label/color. Test the conversion fn (esp. weather lift).
- settings-rs startup also calls seeding (so the picker is populated even before the overlay ran).
- Commit: `feat(settings): picker prefers bundled plugins + convert-to-plugin for legacy slices`

### Task 6: in-app verification + gates
- Extend `scripts/widget-smoke.sh`: after weather, also seed-install cpu (via OXIDEMX_BUILTIN_WIDGETS_DIR pointing at target/builtin-widgets), place it on slot 2, assert TWO scene lines and that cpu's scene arrives after the harness's MenuOpened (the --widget-smoke flag already sends one? read it — extend to send MenuOpened and wait for the stats-driven render).
- `scripts/settings-widget-demo.sh`: assert registry lists 6 ready widgets after seeding.
- Full gates: workspace tests, clippy on touched crates, both scripts. Update authoring.md (system-stats permission + bundled widgets section) + followups.md (MouseBattery conversion, battery feed via daemon D-Bus).
- Commit: `test(widgets): builtin conversion smoke + docs`

**Self-review notes:** §16 coverage — push feed (T2), bundled set minus battery (T3), seeding+packaging (T4), picker/back-compat/convert (T5). Append-only wire discipline respected (T1 comments). Weather move keeps docs coherent (T3 greps for stale paths).
