# Indicator + Popup + Settings — follow-ups

Tracked items deferred during the 25-commit `indicator-feature` integration (now merged into `rust-gtk4-overlay` at `040e3db`). Captured here so nothing falls off the radar. **2026-05-25 pass closed 7 of the 15 items; remaining items below are tagged with status.**

Sibling docs:
- [INDICATOR_DESIGN.md](../../INDICATOR_DESIGN.md) — architecture
- [docs/plans/indicator-implementation.md](indicator-implementation.md) — the phased plan that shipped
- [design/oxidemx-indicator/SPEC_ADDENDUM.md](../../design/oxidemx-indicator/SPEC_ADDENDUM.md) — deltas vs the original prompt

---

## P1 — Blocks `cargo test --workspace`

### 1. `daemon/src/hidpp/tests.rs` `page_change` field initializers ✅ DONE (resolved by merge)
The stash-pop-and-resolve cycle on `rust-gtk4-overlay` properly restored the user's in-flight `page_change` work — both `manager.rs` initializers and `tests.rs` fixtures have the field. Verified 2026-05-25 with `cargo test -p oxidemxd --lib` → **266 passed, 0 failed, 7 ignored**.

---

## P2 — Indicator Phase 3.5 (deferred quick-actions)

### 2. Wire the 7 unwired quick-toggle / quick-slider actions to real daemon D-Bus methods 🟡 PARTIAL

| Quick-action id | `Action::` variant | Status |
|---|---|---|
| `haptics` | `Haptics(bool)` | ✅ **DONE 2026-05-25.** Added `daemon::set_haptics_enabled(bool)` that mutates `config.haptics.enabled`, refreshes the live `HapticManager`, and persists to disk via `Config::save()`. `popup-rs/src/actions.rs` now dispatches via the new method. |
| `radial` | `Radial(bool)` | ⏳ PENDING. ON-side could call existing `EnsureOverlayRunning()`. OFF-side needs a new `ShutdownOverlay()` method backed by an `OverlaySpawner::shutdown()` that signals the overlay's `HideMenu`-loop + waits for the bus name to drop. |
| `scroll` | `Scroll(u8)` | ⏳ PENDING. No corresponding daemon-side concept yet. The popup design says "1–10 scroll sensitivity"; the daemon's `ScrollConfig` has `mode`/`smartshift`/`smartshift_threshold`/`natural` — none is a linear 1–10 knob. Needs a design pass on what "sensitivity" maps to (DPI multiplier? HID++ wheel-divisor?). |
| `haptic_i` | `HapticIntensity(u8)` | ⏳ PENDING. The MX4 haptic patterns are discrete strings (`DampStateChange`, `DampLight`, …) — there is no intensity knob on the device. Could map "intensity" → pattern selection (off / light / medium / strong). Needs design. |
| `accel` | `Accel(f32)` | ⏳ PENDING. `config.pointer.acceleration` is a `bool` (flat vs default profile). The popup's `-1.0..1.0` range implies a continuous curve. Either reinterpret as discrete (-1=off, +1=on) or add a `acceleration_curve: f32` field. |
| `flow` | `Flow(bool)` | ⏳ PENDING. JuhFlow daemon is a separate process; the indicator popup needs to call into its D-Bus or shell out. Out of scope until JuhFlow gets a D-Bus surface. |
| `highlight` | `Highlight(bool)` | ⏳ PENDING. Cursor-highlight on shake is a brand-new feature — would need a daemon-side motion detector + animator. Largest piece of new work. |

For `Flow` and `Highlight` specifically — consider removing from `QUICK_TOGGLE_CATALOG` in `oxidemx-shared/src/popup.rs` until the underlying features exist; both currently render as toggles that log "not yet wired" on click.

---

## P3 — Indicator Phase 3.5 (UX polish)

### 3. Popup view: pixel-fidelity pass against the design
`popup-rs/src/view.rs` is a functional first pass per the Phase 3 spec ("rough first pass acceptable"). Compare against `design/oxidemx-indicator/popup.jsx` and `design/oxidemx-indicator/index.html` for:
- Battery-ring stroke width + caps
- Device-pill header layout
- Section spacing
- Footer "Settings" link + version line

### 4. `panel-target='both'` full implementation
Currently degrades to top-bar-only with a label hint (`Both panels (currently top-bar only)`). The GObject double-parent error is real — needs either a separate widget instance per panel (with shared state subscription) or removing `'both'` from the schema choices entirely. UX call.

### 5. Mouse-glyph tinting in prefs `Preview` group
`prefs.ts` `_buildPreviewGroup` uses `Gtk.CssProvider` for the mouse-glyph color but the approach is approximate. Verify it renders correctly in the live prefs dialog; if not, switch to setting the icon's symbolic color directly via `pixel-size` + accent CSS class.

### 6. DeviceStateChanged empty-string race window
`popup-rs` correctly merges empty `deviceName`/`connection`/`deviceId` fields from `_latest` cache so the tooltip doesn't blank between signal emit and the next poll. But when the popup first starts, `_latest` is null until the first `GetActiveDeviceState` call returns. If a `DeviceStateChanged` signal arrives BEFORE that initial call completes, the tooltip blanks for one refresh cycle. Two fixes:
- (a) Force a synchronous initial `GetActiveDeviceState` before subscribing to the signal stream.
- (b) Daemon emits the full payload on `DeviceStateChanged` (Task 0.2c that never landed — would require wiring `OxideMXService` info into `BatteryHandler`).

Pick whichever is the smaller diff.

---

## P3 — Code-quality cleanup

### 6. DeviceStateChanged empty-string race window ✅ DONE (already implemented at merge)
`popup-rs::boot()` returns `(State, Task::perform(actions::fetch_device_state(), Message::DeviceFetched))` — the synchronous initial fetch runs as the boot task so `_latest` is populated before any signal can arrive. Verified 2026-05-25.

### 7. `popup-rs` opens fresh `Connection::session()` per action ✅ DONE 2026-05-25
`popup-rs/src/actions.rs` now caches the session connection via `tokio::sync::OnceCell` (`session_conn()` helper). Both `apply()` and `fetch_device_state()` reuse the same connection.

### 8. `overlay_spawner.rs` `Mutex::unwrap()` on Child ✅ DONE 2026-05-25
`daemon/src/overlay_spawner.rs::ensure_running` now recovers from a poisoned mutex via `match self.child.lock() { Ok(g) => …, Err(poisoned) => { warn!(…); *poisoned.into_inner() = … } }` instead of crashing the daemon.

### 9. `normalise_connection_kind` → `normalize_connection_kind` ✅ DONE 2026-05-25
Renamed in `daemon/src/dbus/interface.rs`. Single call site updated. US spelling now matches codebase norm.

### 10. `org.juhlabs.oxidemx.Settings` vs `org.oxidemx.Settings` naming inconsistency ✅ DONE 2026-05-25
Canonical name picked: `org.oxidemx.Settings` (matches `org.oxidemx.Daemon` pattern from Task 0.0). Rust side renamed in `singleton.rs` (`BUS_NAME` + `OBJECT_PATH` + `INTERFACE` + `#[interface]`), `raise.rs` (APP_ID), `cursor_helper.rs` (SETTINGS_APP_ID), `main.rs` (application_id). Python files were already using the new name post-Task-0.0. Aligned.

### 11. Stale Phase comment in `lib/placement.ts` ✅ DONE 2026-05-25
Replaced "deferred to Phase 2 if there is demand" with a reference to this very followups file (P3.4).

### 12. `oxidemx-cursor` extension `ListMonitors` tuple-arity bug ✅ DONE 2026-05-25
Resolution: corrected the XML declaration to `a(iiiii)` (5 ints) to match the actual runtime contract — the existing Rust client at `oxidemx-window/src/cursor_helper.rs:36` already expected 5-tuples. Updated XML, doc-comment, and dropped the `@ts-expect-error` annotation.

---

## P3 — Indicator Phase 3.5 (UX polish) — DEFERRED

These need live-session testing or substantial design work; deferred until next pass.

### P3.3 Popup view pixel-fidelity pass — DEFERRED
Needs a running Wayland session + visual diff against `design/oxidemx-indicator/index.html`. Tackle in a focused visual-polish session.

### P3.4 `panel-target='both'` full implementation — DEFERRED
Requires architectural design: shared-state subscription split across two `PanelMenu.Button` instances on different panels. The current schema lists `'both'` as an option; the user-facing dropdown label already flags "(currently top-bar only)" via `gnome-extension/oxidemx-indicator@dev.juhlabs.com/prefs.ts`.

### P3.5 Mouse-glyph tinting in prefs Preview group — DEFERRED
Approximate `Gtk.CssProvider` rendering needs live verification in the prefs dialog. Defer to a sit-down session with the live extension.

---

## P4 — Documentation

### 13. ListMonitors API contract clarification ✅ DONE (closed with #12)
`INDICATOR_DESIGN.md` §4 didn't include ListMonitors in its D-Bus table; the cursor-helper extension's inline doc comment is the canonical source and is now corrected.

### 14. Update `INDICATOR_DESIGN.md` status header ✅ DONE 2026-05-25
Status now reads: "Phases 0–3 shipped on `rust-gtk4-overlay` (merge commit `040e3db`, 2026-05-25). Post-merge follow-ups tracked in `docs/plans/followups.md`."

### 15. CHANGELOG cleanup pass once a real version cuts ⏳ PENDING
The `[Unreleased]` block enumerates everything that landed. When `0.3.3` (or whatever) cuts, rename to `## [0.3.3] - YYYY-MM-DD` and start a new empty `[Unreleased]` block. Only relevant at release-cut time.

---

## P5 — Distribution (CI + pre-built binaries)

### 16. GitHub Releases pipeline for pre-built binaries ⏳ PENDING

`install.sh` today supports three build paths (host cargo, distrobox cargo, or `OXIDEMX_SKIP_BUILD=1` with pre-built `target/release/*`). The third path is the right one for "I'm a user, not a developer" install on atomic Fedora — but today the user has to produce those binaries themselves.

Wire up a CI pipeline that produces a signed, downloadable artifact per tag:

**Workflow:**

```yaml
# .github/workflows/release.yml
on:
  push:
    tags: ['v*']
jobs:
  build:
    runs-on: ubuntu-latest
    container: fedora:latest   # or rockylinux:9 for older glibc compat
    steps:
      - uses: actions/checkout@v4
      - run: |
          dnf install -y rust cargo dbus-devel systemd-devel \
                         libevdev-devel hidapi-devel git make
      - run: |
          cargo build --release \
            -p oxidemxd -p oxidemx-overlay \
            -p oxidemx-popup -p oxidemx-settings
      - run: |
          tar -czf oxidemx-${{ github.ref_name }}-x86_64-linux.tar.gz \
              -C target/release \
              oxidemxd oxidemx-popup oxidemx-overlay oxidemx-settings
          sha256sum oxidemx-*.tar.gz > oxidemx-${{ github.ref_name }}-x86_64-linux.tar.gz.sha256
      - uses: softprops/action-gh-release@v2
        with:
          files: |
            oxidemx-${{ github.ref_name }}-x86_64-linux.tar.gz
            oxidemx-${{ github.ref_name }}-x86_64-linux.tar.gz.sha256
```

**`install.sh --from-release [vX.Y.Z]` flag:**

```bash
# In install.sh:
download_release_binaries() {
    local tag="${1:-latest}"
    local arch
    arch="$(uname -m)"  # x86_64 or aarch64
    local url
    if [ "$tag" = "latest" ]; then
        url="https://api.github.com/repos/JuhLabs/oxidemx/releases/latest"
        tag=$(curl -s "$url" | grep -oP '"tag_name":\s*"\K[^"]+')
    fi
    local archive="oxidemx-${tag}-${arch}-linux.tar.gz"
    local base="https://github.com/PooDoge/oxidemx/releases/download/${tag}"

    log_info "Downloading $archive..."
    curl -fL --progress-bar -o "/tmp/$archive"        "$base/$archive"
    curl -fL --progress-bar -o "/tmp/$archive.sha256" "$base/$archive.sha256"
    ( cd /tmp && sha256sum -c "$archive.sha256" ) || {
        log_error "Checksum mismatch — refusing to install."; exit 1;
    }
    mkdir -p "$INSTALL_DIR/target/release"
    tar -xzf "/tmp/$archive" -C "$INSTALL_DIR/target/release"
    rm "/tmp/$archive" "/tmp/$archive.sha256"
    log_success "Release $tag binaries downloaded + verified"
}
```

Then user flow becomes:

```bash
# On any Bazzite / Silverblue / Kinoite box, zero rpm-ostree / distrobox required:
curl -fsSL https://raw.githubusercontent.com/JuhLabs/oxidemx/master/install.sh -o /tmp/install.sh
chmod +x /tmp/install.sh
/tmp/install.sh --from-release             # latest tag
/tmp/install.sh --from-release v0.3.3      # pinned tag
```

**Scope:**
- One `release.yml` workflow file.
- ~30 lines added to `install.sh` (the `download_release_binaries` helper + a `--from-release` arg parser + a check that's wired into `build_project()` before the cargo path).
- Update `docs/live-test.md` to mention Path C: "from-release zero-touch install".

**Considerations:**
- glibc compat: building on `fedora:latest` may require glibc ≥ 2.38 at the user's runtime. Use `rockylinux:9` if you need to support older base images.
- Code signing: optional but worth doing. `cosign` is the modern tool; GitHub provides keyless signing via OIDC.
- ARM64: Bazzite has an aarch64 spin (Fedora-Asahi). Build matrix should include `aarch64-unknown-linux-gnu`.
- Provenance: SBOM via `cargo-cyclonedx` or `syft` if you want OCI-style attestation.
- The download URL needs to handle `JuhLabs/oxidemx` vs forks — accept `OXIDEMX_RELEASE_REPO` env override.

**Estimated effort:** half a day for the basic pipeline, full day with signing + ARM64 + glibc-compat docs.

---

## Reference: what landed on this branch

25 commits from indicator-feature + 1 merge commit:

```
040e3db merge(indicator-feature): resolve conflicts + restore in-flight work
90e1517 fix: align popup-rs D-Bus proxy with real daemon interface
38d563b fix(popup): build in install.sh + wire scroll/esc + tidy ownership
c63c73f feat(popup): oxidemx-popup binary - daemon-spawned indicator popup
626fc39 fix(settings): correct misleading copy in Indicator Popup tab
ed0cb4a feat(settings): Indicator Popup tab
10f3d9f feat(indicator): stylesheet + four symbolic mouse-shape SVGs
c22e5da fix(indicator): clean up prefs.ts subscriptions + dead import
0e1ecfe feat(indicator): libadwaita preferences dialog (prefs.ts)
9dc6d4b fix(indicator): surface systemctl failure + preserve device info across signals
8d0609e feat(indicator): scaffold + lib + extension.ts
bb0d050 fix(gnome-ext): silence two tsc errors with @ts-expect-error notes
1b1b77e fix(gnome-ext): tsconfig moduleResolution + @girs/* version specs
83b3cba feat(gnome-ext): TypeScript build pipeline + migrate cursor extension
7de09b6 refactor: extract oxidemx-window sibling crate
0efc2c6 refactor: extract oxidemx-widgets sibling crate
5f01939 feat(indicator): GSettings schema for oxidemx-indicator extension
6b920a5 feat(daemon): wire device info + emit DeviceStateChanged
d937cf2 fix(daemon): reap popup zombie + cache DBusProxy in overlay_spawner
e23a72a feat(daemon): add GetActiveDeviceState + ShowPopup + EnsureOverlayRunning + DeviceStateChanged
7666d6c refactor(shared): consolidate PopupConfig default helpers
74ddc9b feat(shared): add PopupConfig + quick-action catalogs
bb62f85 rename(dbus): org.kde.oxidemx -> org.oxidemx.Daemon
deb2c0e docs: spec lock + implementation plan for indicator + popup + settings
```

## Widget system follow-ups

Deferred from the widget-plugin plans (runtime + CLI + settings UI,
2026-06-12). Ordered roughly by user impact:

- **Keyboard nav for the behavior picker** — roving focus across the
  tile grid, Esc closes the panel, Enter applies the focused tile.
  iced 0.14's focus APIs make this a sizeable chunk; explicitly
  deferred from Plan 3 Task 1. The slice editor's reorder rows
  (plan 5) inherit the same gap: rows are mouse-only today.
- **Drag-reorder for the slice editor rows** — the reorder-row list
  (plan 5) moves slices with up/down chevrons only, like the
  design's reorder handles. True drag-and-drop between rows needs a
  drag overlay + drop-target hit-testing that iced doesn't give us
  for free (the radial preview's canvas drag-to-swap already covers
  the common case).
- **Scroll-to-expanded slice card** — clicking a wedge in the radial
  preview expands that slot's card inline in the list below, but
  the page doesn't scroll to it (iced's `scrollable` has no
  scroll-to-child API; `scroll_to` needs a hand-computed offset).
  Revisit if users report losing the expansion off-screen.
- **Live picker-tile mini-previews (1 fps)** — picker widget tiles
  currently show canned design-mockup values; the options-card live
  preview (Task 5) proved the worker-embedding pattern, the picker
  just needs a multi-instance variant with a 1 fps cap + pause when
  the panel is hidden.
- **`Prim::Image` real rendering + wedge-path clipping** — the scene
  replay (`oxidemx-scene-render`) draws a placeholder rect for Image
  prims (needs a decode + handle cache) and does not clip prims to
  the wedge path; both halves of spec §8's drawing model.
- **Per-frame `InstanceId` derivation caching in ring.rs** — the
  overlay derives `instance_key` (string alloc) per widget slice per
  frame; cache per (page, slot) and invalidate on config reload.
- **Slot-number display vs 0-based key cosmetic mismatch** — the UI
  says "Slot 5" (1-based) while instance keys read `apps.slot4`
  (0-based). Settle on one public numbering or annotate the
  breadcrumb so hand-editors aren't surprised.
- **Haptic routing to the daemon** — `HostCmd::HapticPulse` is gated
  on the `haptics` permission but stubs to a log line; route through
  the daemon's haptic client like the overlay's own pulses.
- **tracing-log bridge in the overlay** — the widget host logs via
  `log` (incl. guests' `Log` cmds); the overlay only installs a
  `tracing` subscriber, so widget logs vanish. Add `tracing-log`'s
  LogTracer (settings-rs has the same gap).
- **Registry browsing UI (1.0)** — the widget store dialog lists
  installed widgets + file/URL install only; spec §11's catalog
  browse/search against widgets.oxidemx.org is post-v1.
- **exec/open-url consent UX deepening** — install-time consent
  lists permissions, but `exec` in particular deserves a louder,
  per-permission explanation (and possibly a first-use prompt)
  rather than one bullet in the sideload dialog.
- **MouseBattery → bundled plugin conversion** — the only built-in
  widget left native after the spec §16 conversion (plan 4). Blocked
  on a host-side battery data feed: `SystemStatsSnapshot` already
  reserves `battery_pct`/`battery_charging` (append-only, always
  `None` today), but the widget host has no daemon D-Bus client to
  fill them. Add one (subscribe to the daemon's battery signal like
  the indicator does), populate the snapshot fields, ship a
  `widgets/builtin/battery` crate, and extend the picker mapping in
  `settings-rs/src/tabs/buttons/picker.rs::builtin_plugin_id`.
- **Native widget render-path removal (future MAJOR)** — the legacy
  `WidgetSource::{Weather,Cpu,Memory,Network,Disk,TasksDue}` render
  paths in `overlay-rs/src/render/slices/widgets.rs` +
  `overlay-rs/src/sampler.rs` stay for back-compat with existing
  configs (spec §16: existing slices keep rendering exactly as
  today; the picker offers one-click conversion). Remove them only
  in a major cleanup once conversion has been the default for a
  release — at that point also drop the canned native picker tiles
  and auto-convert remaining configs.

## Gap-sweep triage (2026-06-12 evening)

Full-workspace stub sweep (session 89b684a1). DONE this pass:
heartbeat tick (+systemd timer armed, 30 min), SmartShift button
action (HID++ toggle via existing 0x2110 path). Remaining, in
priority order, with deferral reasons verified in-code:

1. Popup quick-actions (radial/flow/highlight/scroll/haptic_i/
   accel) — blocked on missing daemon D-Bus surfaces + three open
   DESIGN decisions (scroll 1-10 knob vs mode/threshold; haptic
   intensity vs discrete patterns; accel bool vs curve). Consider
   hiding Flow/Highlight toggles until their features exist.
2. DeviceStateChanged empty device fields (battery.rs TODO) —
   blocked on a device-cache module; indicator falls back to a
   second poll today.
3. ButtonAction::Custom — needs a config payload field (what
   command/keys?) before it can mean anything; stub now says so.
4. tracing-log bridge for widget host logs — S effort, unblocked.
5. Editor-crate TODOs (icon_picker, editor window, tray SNI) —
   superseded in practice by settings-rs; decide deprecate-vs-
   finish before investing.

### From Plan 4/5 final reviews (2026-06-12)
- **Atomic seeding**: `install()` does remove_dir_all + incremental extract; two
  processes seeding concurrently (overlay autostart + settings) can interleave on
  an upgrade and a registry scan can observe a half-extracted widget. Fix:
  extract-to-tempdir-then-rename, or a flock around seeding.
- **Stats sampling skip for dead instances**: 3-strike-disabled instances keep
  their `system-stats` permission entry, so push_stats keeps sampling (df spawns)
  for widgets that will never render. Filter disabled instances in push_stats.
- Preview worker MenuOpened one-shot: DONE inline (widget_preview.rs) — stats
  widgets now render live in the options-card preview.

## SP2a → SP2c deferred (oxidemx-ledger, 2026-06-19)

- **oxidemx-ledger: add `record_tool_call`** — bump `Step.tool_calls` + append a `ToolCall` event; also emit `TaskCreated` on `create`. [SP2a→SP2c]
- **oxidemx-ledger: add per-step budget field + StepGraph validate()/cycle-check** for the Planner. [SP2a→SP2b/c]

## SP2b final-review fixes (2026-06-19)

- **SP2c: before trusting GENERATE-TIME constraint on PlanOutput, smoke-test mistralrs 0.8.1 `Constraint::JsonSchema` acceptance of schemars' draft-07 `$ref`/`definitions` schema (nested PlanStep).** If the llguidance grammar backend chokes on `$ref`, inline PlanStep (flat schema) or convert to draft-2020-12 `$defs`. Validate-time jsonschema is the backstop, so a generate-time failure degrades to unconstrained-decode+validator-catch, not a correctness hole. [SP2b final review]

## T3 review fixes (2026-06-19)

- **agentd compose_flow validates via subprocess** — agentd links the conductor crate in-process, but compose_flow shells out to `oxidemx-conductor validate`. Expose `oxidemx_conductor::validate(id)` as a lib fn and call in-process (systemd unit may lack the conductor binary on PATH). [SP1c T3]
- **agentd memory/persona tools use global core paths** — both tools are backed by global `~/.local/share/oxidemx/memories.json` and `~/.config/oxidemx/{soul,user}.md`. Wire per-project memory/persona under the project's `.oxidemx/` store. [SP1c T3]
- oxidemx-agent-core hardcodes `/home/jim` HOME fallback in persona.rs, memory_semantic.rs, heartbeat.rs, api_key.rs, tasks.rs, memory.rs — sweep to a shared home_dir() helper that errors/uses XDG instead. [SP1c T8a]
- NOTE: 'StepGraph validate()/cycle-check' + 'grammar-constrained decoding' are now scoped INTO SP2b, and 'record_tool_call/TaskCreated + per-step budget' INTO SP2c (see SP2 spec §7). [2026-06-19]
- SP2c final-review minors (address in SP2d): RunReport.blocked conflates dep-failed vs dep-needs-approval; extract_path_arg covers file_path/path/source but not destination/new_path (document mutation tools must expose the WRITTEN path); reversible.rs ancestor-directory-symlink not resolved (leaf-only); git2 default-features=false disables transports (fine for HEAD-tree read). [SP2c final review]
