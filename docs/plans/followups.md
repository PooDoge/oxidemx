# Indicator + Popup + Settings — follow-ups

Tracked items deferred during the 25-commit `indicator-feature` integration (now merged into `rust-gtk4-overlay` at `040e3db`). Captured here so nothing falls off the radar. **2026-05-25 pass closed 7 of the 15 items; remaining items below are tagged with status.**

Sibling docs:
- [INDICATOR_DESIGN.md](../../INDICATOR_DESIGN.md) — architecture
- [docs/plans/indicator-implementation.md](indicator-implementation.md) — the phased plan that shipped
- [design/juhradial-indicator/SPEC_ADDENDUM.md](../../design/juhradial-indicator/SPEC_ADDENDUM.md) — deltas vs the original prompt

---

## P1 — Blocks `cargo test --workspace`

### 1. `daemon/src/hidpp/tests.rs` `page_change` field initializers ✅ DONE (resolved by merge)
The stash-pop-and-resolve cycle on `rust-gtk4-overlay` properly restored the user's in-flight `page_change` work — both `manager.rs` initializers and `tests.rs` fixtures have the field. Verified 2026-05-25 with `cargo test -p juhradiald --lib` → **266 passed, 0 failed, 7 ignored**.

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

For `Flow` and `Highlight` specifically — consider removing from `QUICK_TOGGLE_CATALOG` in `juhradial-shared/src/popup.rs` until the underlying features exist; both currently render as toggles that log "not yet wired" on click.

---

## P3 — Indicator Phase 3.5 (UX polish)

### 3. Popup view: pixel-fidelity pass against the design
`popup-rs/src/view.rs` is a functional first pass per the Phase 3 spec ("rough first pass acceptable"). Compare against `design/juhradial-indicator/popup.jsx` and `design/juhradial-indicator/index.html` for:
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
- (b) Daemon emits the full payload on `DeviceStateChanged` (Task 0.2c that never landed — would require wiring `JuhRadialService` info into `BatteryHandler`).

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

### 10. `org.juhlabs.juhradial.Settings` vs `org.juhradial.Settings` naming inconsistency ✅ DONE 2026-05-25
Canonical name picked: `org.juhradial.Settings` (matches `org.juhradial.Daemon` pattern from Task 0.0). Rust side renamed in `singleton.rs` (`BUS_NAME` + `OBJECT_PATH` + `INTERFACE` + `#[interface]`), `raise.rs` (APP_ID), `cursor_helper.rs` (SETTINGS_APP_ID), `main.rs` (application_id). Python files were already using the new name post-Task-0.0. Aligned.

### 11. Stale Phase comment in `lib/placement.ts` ✅ DONE 2026-05-25
Replaced "deferred to Phase 2 if there is demand" with a reference to this very followups file (P3.4).

### 12. `juhradial-cursor` extension `ListMonitors` tuple-arity bug ✅ DONE 2026-05-25
Resolution: corrected the XML declaration to `a(iiiii)` (5 ints) to match the actual runtime contract — the existing Rust client at `juhradial-window/src/cursor_helper.rs:36` already expected 5-tuples. Updated XML, doc-comment, and dropped the `@ts-expect-error` annotation.

---

## P3 — Indicator Phase 3.5 (UX polish) — DEFERRED

These need live-session testing or substantial design work; deferred until next pass.

### P3.3 Popup view pixel-fidelity pass — DEFERRED
Needs a running Wayland session + visual diff against `design/juhradial-indicator/index.html`. Tackle in a focused visual-polish session.

### P3.4 `panel-target='both'` full implementation — DEFERRED
Requires architectural design: shared-state subscription split across two `PanelMenu.Button` instances on different panels. The current schema lists `'both'` as an option; the user-facing dropdown label already flags "(currently top-bar only)" via `gnome-extension/juhradial-indicator@dev.juhlabs.com/prefs.ts`.

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

## Reference: what landed on this branch

25 commits from indicator-feature + 1 merge commit:

```
040e3db merge(indicator-feature): resolve conflicts + restore in-flight work
90e1517 fix: align popup-rs D-Bus proxy with real daemon interface
38d563b fix(popup): build in install.sh + wire scroll/esc + tidy ownership
c63c73f feat(popup): juhradial-popup binary - daemon-spawned indicator popup
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
7de09b6 refactor: extract juhradial-window sibling crate
0efc2c6 refactor: extract juhradial-widgets sibling crate
5f01939 feat(indicator): GSettings schema for juhradial-indicator extension
6b920a5 feat(daemon): wire device info + emit DeviceStateChanged
d937cf2 fix(daemon): reap popup zombie + cache DBusProxy in overlay_spawner
e23a72a feat(daemon): add GetActiveDeviceState + ShowPopup + EnsureOverlayRunning + DeviceStateChanged
7666d6c refactor(shared): consolidate PopupConfig default helpers
74ddc9b feat(shared): add PopupConfig + quick-action catalogs
bb62f85 rename(dbus): org.kde.juhradialmx -> org.juhradial.Daemon
deb2c0e docs: spec lock + implementation plan for indicator + popup + settings
```
