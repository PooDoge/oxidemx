# SPEC ADDENDUM — OxideMX Indicator + Popup + Settings

**Date:** 2026-05-24
**Parent spec:** [`CLAUDE_CODE_PROMPT.md`](CLAUDE_CODE_PROMPT.md) (preserved verbatim as historical record)
**Architectural design:** [`../../INDICATOR_DESIGN.md`](../../INDICATOR_DESIGN.md)
**Implementation plan:** [`../../docs/plans/indicator-implementation.md`](../../docs/plans/indicator-implementation.md)

This addendum records the deltas between the original `CLAUDE_CODE_PROMPT.md` spec and the agreed implementation, as resolved in the 2026-05-24 clarification round and the subsequent live conversation. **Read this alongside the parent spec.** The parent spec is *not* edited — it is preserved as the original brief; this addendum is the authority where the two disagree.

---

## A. Decisions confirmed in the 2026-05-24 clarification round

| # | Topic | Parent spec assumed | Confirmed decision |
|---|---|---|---|
| 1 | Popup window ownership | "Daemon-owned popup, in-process Iced window inside the daemon" | **Separate `oxidemx-popup` binary** spawned by daemon's `ShowPopup` D-Bus handler. Keeps `oxidemxd` UI-free (~30MB iced/wgpu otherwise loaded into the supervisor). |
| 2 | D-Bus name | `org.oxidemx.Daemon` | **Confirmed `org.oxidemx.Daemon`** — but this requires a flag-day rename from the legacy `org.kde.oxidemx`. Rename scope: 5 daemon files, 4 overlay-rs files, settings-rs, packaging desktop file (rename), 3 install scripts, 2 root design docs. Done atomically in Phase 0. |
| 3 | Popup preferences storage | New `~/.config/oxidemx/config.toml` (TOML, serde) | **New `popup` field on existing `config.json`** (serde_json, reuses overlay's inotify watcher and `settings-rs/persist.rs` debounce). One file = one reload. |
| 4 | Crate extractions | "Extract `oxidemx-widgets` and `oxidemx-window`" | **Confirmed — extract both.** New workspace members `oxidemx-widgets/` (moves `widgets.rs + style.rs + palette.rs` out of settings-rs) and `oxidemx-window/` (moves `ext_positioner.rs` out of overlay-rs and adds `frameless_topmost()` helper from the inlined overlay window setup). Done in Phase A. |
| 5 | GNOME extension language | TypeScript for the new indicator | **TypeScript for *both* extensions.** Existing `oxidemx-cursor@dev.juhlabs.com` migrates from plain JS to TS at the same time. Shared `gnome-extension/tsconfig.json`. |
| 6 | Critical-battery notification | Not specified | **Emit one-shot `Gio.Notification`** when device first crosses into critical band (not charging). Re-armable on band exit or charging start. Body: `"<deviceName> at <pct>% — connect charging cable"`. Implemented in `lib/battery.ts`. |
| 7 | Unwired quick-action ids (`highlight`, `flow`) | Implicit — assumed all daemon methods exist | **Render the toggle, log "not yet wired" on click.** Catalog stays complete per the design. Wiring tracked as a Phase 3.5 follow-up. |
| 8 | `ShowPopup(panel_rect)` coordinate space | Implicit | **Stage-absolute logical pixels** (whatever `actor.get_transformed_extents()` returns natively). Popup forwards `monitor=-1` to `MoveOverlay`; cursor extension resolves the monitor. |

## B. Scope extension confirmed in the 2026-05-24 live conversation

### B.1 The indicator is also the stack supervisor / unified health surface

**Parent spec position:** The indicator's responsibilities are battery surface + popup launcher. Nothing about supervising daemons, restarting processes, or showing stack health.

**Confirmed scope extension:** The indicator is the single front-of-house surface for "is the OxideMX stack healthy" and the single click-to-remediate surface. This added a third co-equal responsibility documented as §3.4 in `INDICATOR_DESIGN.md`. Summary:

| Probe | Check | Remediation |
|---|---|---|
| Daemon process | `systemctl --user is-active oxidemx-daemon.service` + D-Bus name probe | Indicator turns critical-colored. Right-click menu adds `Start daemon`. Popup footer shows `Daemon down — Start`. |
| Device link | `GetActiveDeviceState` returns `connection != "off"` | Indicator shows `DisconnectedIndicator` lozenge. Popup hides quick toggles, links to Devices tab. |
| Radial overlay process | When `Radial Overlay` toggle goes ON: D-Bus probe for `org.oxidemx.overlay`. If missing: call new daemon method `EnsureOverlayRunning()` which wraps the spawn. | Daemon handles the actual spawn — extension never spawns long-lived processes itself. |
| Gaming-mode bridges | Daemon's `GetGamingState()` confirms active + gamepad-haptic bridge thread running inside daemon | Daemon-internal; no supervision needed. |
| Haptic feedback | Daemon's `haptic_supported()` returns true | Daemon-internal; no supervision needed. |
| Cursor-helper extension | `Main.extensionManager.lookup('oxidemx-cursor@dev.juhlabs.com')` enabled | Popup footer shows one-time warning if missing. |

**Architectural rules baked in:**
- Extension **never** spawns long-lived processes directly. Use `systemctl --user start <unit>` or new daemon-side methods (`EnsureOverlayRunning`).
- Remediation is **user-confirmed** by default. The exception is auto-start when the user toggles `Radial Overlay` ON — that toggle is itself consent.
- Health is observable on three consistent surfaces: panel button color, popup header line, right-click menu item labels.

**New D-Bus method introduced for this responsibility:**

```rust
/// Idempotent. Returns immediately if overlay is on the bus; otherwise
/// spawns `oxidemx-overlay` and waits up to 3s for the bus
/// registration before returning.
async fn ensure_overlay_running(&self) -> fdo::Result<()>;
```

---

## C. Items the parent spec leaves *open*, defaulted by this addendum

| Topic | Default | Rationale |
|---|---|---|
| Dash-to-Panel detection | `Main.extensionManager.lookup('dash-to-panel@jderose9.github.com')`; fall back to schema probe if `lookup` returns null on older DTP builds | Modern API is the supported path; schema probe is the documented fallback. |
| Tab nav glyph for `IndicatorPopup` | `applications-system-symbolic`, glyph `I` | Closest match in the freedesktop symbolic set; no special icon needed. |
| Daemon-spawn timeout for the popup | 1.5s before extension logs warning | Popup binary is small; cold start should complete within that window. |

---

## D. Items the parent spec is *correct about* — no change

These are spelled out so a future reader can tell the original spec held on these points:

- Section ordering on the Indicator Popup tab (Mode → Easy-Switch → Quick toggles → Quick sliders → Interactions).
- Default toggle set (Simple: gaming/haptics/radial; Power: + flow).
- Default slider set (Power: dpi/scroll).
- Battery thresholds (15% critical, 30% low) and the four band colors.
- Three click-behavior options (`popup` / `settings` / `none`).
- Up/down arrow reorder UX (no drag gestures).
- libadwaita single-page prefs layout (no sidebar).
- Inserting the new tab between Point & Scroll and Haptic Feedback.
- Acceptance criteria visual targets matching `index.html` artboards.

---

## E. NON-GOALS — added in addendum

In addition to the NON-GOALS listed in the parent spec (no GJS-rendered popup, no drag-reorder, no Devices-page redesign, no new battery bands, no WM-drawn popup, no overlay-window fork):

- ❌ The extension does **not** run any subprocess of its own. All process spawns go through `systemctl --user` or new daemon D-Bus methods.
- ❌ The supervisor checks are **never** invasive — no daemon restarts without user consent, no killing of overlay processes the user is actively using.
- ❌ The popup does **not** ship a "Settings" deep-link inside every toggle. One Settings link in the popup footer, that's it.
- ❌ The Phase 0 rename does **not** bundle any other refactor (no module reorganisation, no API change beyond the three new D-Bus members). String substitution + tests, that's the entire diff.
