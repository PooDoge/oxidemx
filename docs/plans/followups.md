# Indicator + Popup + Settings — follow-ups

Tracked items deferred during the 25-commit `indicator-feature` integration (now merged into `rust-gtk4-overlay` at `040e3db`). Captured here so nothing falls off the radar.

Sibling docs:
- [INDICATOR_DESIGN.md](../../INDICATOR_DESIGN.md) — architecture
- [docs/plans/indicator-implementation.md](indicator-implementation.md) — the phased plan that shipped
- [design/juhradial-indicator/SPEC_ADDENDUM.md](../../design/juhradial-indicator/SPEC_ADDENDUM.md) — deltas vs the original prompt

---

## P1 — Blocks `cargo test --workspace`

### 1. `daemon/src/hidpp/tests.rs` `page_change` field initializers
**Status:** pre-existing on `rust-gtk4-overlay` before the indicator work began. The merge did not introduce or change this; library code builds clean, only the daemon test target fails to compile.

**Symptoms:**
```
error[E0063]: missing field `page_change` in initializer of `PerEventPattern`
error[E0063]: missing field `page_change` in initializer of `HapticEventConfig` (x2)
```

**Fix:** the `page_change` variant / field was added to `HapticEvent` / `PerEventPattern` / `HapticEventConfig` somewhere in the haptic-patterns work but the three test fixtures in `tests.rs` weren't updated. Walk through the three initializers and add `page_change: PerEventPattern::default()` (or the equivalent value the production code uses).

---

## P2 — Indicator Phase 3.5 (deferred quick-actions)

### 2. Wire the 7 unwired quick-toggle / quick-slider actions to real daemon D-Bus methods

`popup-rs/src/actions.rs` currently logs `"action not yet wired to daemon"` for these `Action::*` variants. Each needs a corresponding daemon D-Bus method:

| Quick-action id | `Action::` variant | Daemon work needed |
|---|---|---|
| `haptics` | `Haptics(bool)` | Add `set_haptics_enabled(bool)` → write `config.haptics.enabled` + signal `ReloadConfig`. |
| `radial` | `Radial(bool)` | Add `set_radial_enabled(bool)` → on/off should map to `EnsureOverlayRunning()` vs daemon kill-signal to overlay process. |
| `scroll` | `Scroll(u8)` | Add `set_scroll_sensitivity(u8)` → write `config.scroll.scroll_speed` (range 1–10 per design). |
| `haptic_i` | `HapticIntensity(u8)` | Add `set_haptic_intensity(u8)` → write `config.haptics.intensity_pct` or equivalent. |
| `accel` | `Accel(f32)` | Add `set_pointer_accel(f32)` → write `config.pointer.acceleration_curve` (range -1.0..1.0). |
| `flow` | `Flow(bool)` | Cross-device flow toggle — currently no daemon method exists at all. May be juhflow-scope. |
| `highlight` | `Highlight(bool)` | Cursor-highlight on shake — needs a new daemon-side animator. May be biggest piece of net-new work. |

Mechanically:
1. Add the method to `daemon/src/dbus/interface.rs` (mirror `set_dpi` / `set_smart_shift` shape).
2. Add the proxy method to `popup-rs/src/actions.rs` `Daemon` trait declaration.
3. In `popup-rs/src/actions.rs::apply`, change the warn-only arm to the real `proxy.set_*` call.
4. Smoke-test from the popup binary against a running daemon.

For `Flow` and `Highlight` specifically — check whether these belong here at all or should be removed from `QUICK_TOGGLE_CATALOG` in `juhradial-shared/src/popup.rs` until the underlying features exist.

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

### 7. `popup-rs` opens fresh `Connection::session()` per action
`popup-rs/src/actions.rs::apply()` calls `Connection::session()` on every D-Bus dispatch. zbus pools internally so this isn't catastrophic, but a single connection stored at popup startup (`Arc<Connection>` shared across actions) would be cleaner. Mirrors what `overlay-rs` does.

### 8. `overlay_spawner.rs` `Mutex::unwrap()` on Child
`daemon/src/overlay_spawner.rs:62` does `*self.child.lock().unwrap() = Some(child)`. The `.unwrap()` panics if the mutex is poisoned — which kills the daemon. Replace with `.unwrap_or_else(|poisoned| { warn!(...); poisoned.into_inner() })` or propagate the error.

### 9. `normalise_connection_kind` → `normalize_connection_kind`
`daemon/src/dbus/interface.rs` uses UK spelling for the helper. Codebase norm elsewhere uses US (`normalized` in `bundled_themes.rs`). Single rename + call-site update; one call site only.

### 10. `org.juhlabs.juhradial.Settings` vs `org.juhradial.Settings` naming inconsistency
Pre-existing inconsistency surfaced during the D-Bus rename (Task 0.0 review):
- `settings-rs/src/singleton.rs` registers as `org.juhlabs.juhradial.Settings`.
- `overlay/juhradial-overlay.py:445` and `overlay/settings_page_settings.py:116` now use `org.juhradial.Settings` (post-Task-0.0 rename).
- Neither pointed at the same name before the rename either (different inconsistency).

Pick one name, update all three sites.

### 11. Stale Phase comment in `lib/placement.ts`
`gnome-extension/juhradial-indicator@dev.juhlabs.com/lib/placement.ts` says "A dual-button design is deferred to Phase 2". Phase 2 (the settings tab) has shipped. Should say "not yet implemented" or reference a follow-up issue.

### 12. `juhradial-cursor` extension `ListMonitors` tuple-arity bug
Pre-existing JS bug intentionally preserved during the Phase 1A TS migration:
- D-Bus XML declares `a(iiii)` (4-tuple)
- Code pushes 5-tuples `[idx, x, y, w, h]`

The `@ts-expect-error` annotation in `extension.ts:300` flags it. Fix: either update the XML to `a(iiiii)` and add `monitor_index` to the documented contract, or drop the `idx` field from the pushed array.

---

## P4 — Documentation

### 13. ListMonitors API contract clarification
Per #12 — if we fix the tuple-arity bug by adding `idx`, document it in the cursor-helper extension's README + `INDICATOR_DESIGN.md` §4 D-Bus table.

### 14. Update `INDICATOR_DESIGN.md` status header
Currently says "Status: Pre-implementation. Spec locked 2026-05-24." Update to reflect "Status: Phase 0–3 shipped on rust-gtk4-overlay at 040e3db".

### 15. CHANGELOG cleanup pass once a real version cuts
The `[Unreleased]` block enumerates everything that landed. When `0.3.3` (or whatever) cuts, rename to `## [0.3.3] - YYYY-MM-DD` and start a new empty `[Unreleased]` block.

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
