# Widget Plugin Settings UI Implementation Plan (Plan 3 of 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The settings-app side of the widget system per the design handoff: behavior chip → inline picker panel (search, action + widget tiles, "Get more widgets…"), the schema-driven widget options card with Global/This-slice scope, and the downloader dialog with file/URL install.

**Architecture:** All UI lives in settings-rs (iced, Elm-style — see `settings-rs/src/main.rs` Message/State/update and `tabs/buttons.rs` for the slice editor being modified). Widget metadata comes from `oxidemx_widget_host::registry::WidgetRegistry` scanned directly (no worker for the list); install/uninstall reuse `oxidemx-widget-cli`'s lib functions. Persistence goes through the existing `AppConfig` + `persist::save` flow — the overlay picks changes up via the file watcher. UI reference (visual truth): the design bundle mockups in `/tmp/omx-design/oxidemx-design-system/project/menu-page.jsx` + spec §10 of `docs/superpowers/specs/2026-06-12-widget-plugin-system-design.md`.

**Scoped deviations (approved):** (a) write-through is immediate via the existing save path — the design's 400 ms debounce is dropped (settings-rs already saves per keystroke elsewhere; consistency wins). (b) Picker widget-tile previews are static (canned value/sublabel styled like the design) — live 1 fps wasm previews and the options-card live wedge preview are LAST (Task 5) and may land as a follow-up if iced-embedding friction explodes; everything else must not depend on them.

**New files** (keep `buttons.rs` from growing — it is already 1579 lines):
- `settings-rs/src/tabs/buttons/mod.rs` — move existing buttons.rs here unchanged (mechanical move first, separate commit)
- `settings-rs/src/tabs/buttons/picker.rs` — chip + picker panel + tiles
- `settings-rs/src/tabs/buttons/widget_options.rs` — options card
- `settings-rs/src/widget_store.rs` — downloader dialog + install pipeline + registry cache

---

### Task 0: Mechanical module split

- [ ] `git mv settings-rs/src/tabs/buttons.rs settings-rs/src/tabs/buttons/mod.rs`; fix `mod` declarations; `cargo check -p oxidemx-settings`; commit `refactor(settings): buttons.rs → buttons/mod.rs (no changes)`

### Task 1: Registry cache + behavior chip + picker panel

**Files:** create `picker.rs`; modify `buttons/mod.rs` (replace the kind `pick_list` row inside `selected_slice_editor` / `slice_editor_row`), `main.rs` (State/Message/update), `Cargo.toml` (+ oxidemx-widget-host path dep).

State additions (`main.rs`):
```rust
pub struct State {
    // …
    pub widget_registry: Vec<WidgetSummaryLite>,   // scanned at startup + on store actions
    pub picker_open: Option<usize>,                 // slice idx with picker expanded
    pub picker_search: String,
    pub picker_undo: Option<(usize, Slice)>,        // undo-by-reselect cache, GC'd on picker close/slice deselect
}
/// settings-side mirror of host WidgetSummary (id, name, version, author,
/// ready: bool, reason: Option<String>, has_options, icon_path, signature: String)
```
Build `WidgetSummaryLite` from `WidgetRegistry::scan(widgets_dir)` + `iter()` (read the actual registry API in `oxidemx-widget-host/src/registry.rs` — `InstalledWidget.state`, `SignatureState`).

Behavior chip (design `ActionChip`): 40px icon tile (IconCache equivalent in settings-rs — see how the radial preview canvas resolves icons; for the chip use `iced::widget::svg`/icon name text fallback, match existing settings widgets style from `oxidemx-widgets` crate), title, one-line summary (kind-specific: command for Exec, macro name, widget "Live widget · <name>" etc.), `Change…` button → `Message::OpenPicker(idx)`. Widget slices: accent-tinted container style + small "WIDGET" tag (see `jr-tag` in mockup; use the `oxidemx_widgets::style` helpers).

Picker panel (replaces chip's row while `picker_open == Some(idx)`; chip stays visible above with button label "Cancel"):
- Search `text_input` (`Message::PickerSearch`), filters both groups by name/summary/author, hides empty group headers. (No Esc-key handling unless trivial — note if skipped.)
- Group 1 "Built-in actions": tiles for every `ActionKind` variant currently in `KIND_OPTIONS` EXCEPT Widget (icon + label + one-line sub; reuse each kind's existing display name; single click = existing `Message::SetSliceKind(idx, kind)` + close picker).
- Group 2 "Widgets · n installed": tiles for the 7 built-in `WidgetSource`s (canned preview values copied from the design mockup: 14°/23%/11.2/84↓/412/3/78%) AND each registry widget (icon from icon_path via svg handle, name, author · version, gear badge when has_options, "incompatible" dimmed state with reason tooltip). Click → `Message::PickWidget(idx, WidgetSource)`: sets kind=Widget, widget = WidgetConfig { source, format: None, scope: Instance, instance_key: Some(instance_key(page, slot)) } (custom only; built-ins keep instance_key None), auto-label (slice.label = widget name) only if label is empty or equals the previous auto-label.
- Current selection: accent ring + ✓ on the tile matching the slice's current kind/source.
- Last tile: dashed "Get more widgets…" → `Message::OpenWidgetStore`.
- Grid: `iced::widget::row/column` wrap at 3-4 per row (fixed 3-up is acceptable; note it).
- Undo-by-reselect: `OpenPicker` stores the slice clone in `picker_undo`; re-picking the tile matching the stored slice's behavior restores the full stored slice (so a widget's instance settings survive a round-trip); cache cleared on picker close or slice deselect.

Tests (settings-rs has message-dispatch tests? check existing test style in main.rs/tabs — if none, add a `#[cfg(test)]` module in picker.rs testing the pure helpers): search filtering, auto-label rule, undo-restore, instance_key assignment on custom pick. Commit: `feat(settings): slice behavior chip + action/widget picker panel`

### Task 2: Widget options card

**Files:** create `widget_options.rs`; modify `buttons/mod.rs` (render under the chip when the slice is a Custom widget with options), `main.rs`.

Rendered from the manifest (`registry.get(id).manifest.options`) when slice.widget.source is Custom and the widget is Ready:
- Header: icon, "{Name} — widget options", "Declared by the widget · rendered by OxideMX", version + author tags.
- Scope toggle FIRST (two buttons per design `jr-scope`): "This slice only / Stored with {page} › Slot {n}" vs "All {Name} slices / Shared by {count} instances" (count = how many slices across all pages reference this widget id). `Message::SetWidgetScope(idx, WidgetScope)`:
  - → Global: just set scope (instance bag KEPT in store, ignored).
  - → Instance: set scope + `cfg.widgets.seed_instance(id, key, &manifest_defaults_bag)` (see `oxidemx-shared/src/widgets.rs`).
  - Global scope shows the banner row listing affected instances as chips ("{page} · Slot {n}", this one highlighted).
- One control per `OptionSpec` mapped per spec §5 table: enum ≤4 → segmented buttons (styled like design `jr-radio-group`; use small `button`s), enum >4 + select → `pick_list` (with `unit`-formatted labels: `"s"` + 900 → "Every 15 minutes" — implement humanize for s only, else "{v}{unit}"), string → text_input (placeholder/maxlen), number → slider when (max-min)/step ≤ 100 else text_input-parsed stepper, boolean → toggler, color → swatch row over the slice palette keys (reuse the color pick_list palette source in buttons/mod.rs), location → search text_input + results list + pinned chip, REUSING the existing Open-Meteo geocoder machinery from `settings-rs/src/tabs/settings_page.rs` (read how weather location search works there — extract/share its lookup task + message shape rather than duplicating; factor into a small `geocode.rs` helper module if needed). Unknown type → disabled row "Update OxideMX to edit this option".
- Values read through `cfg.widgets.resolve(id, key, scope, defaults)`; edits write to the scoped bag: Global → `cfg.widgets.global[id][key]`, Instance → `cfg.widgets.instances[key][id][key]`, then the normal save path (`Message` → update → persist). One generic message: `Message::SetWidgetOption { slice: usize, key: String, value: serde_json::Value }` (+ the scope's current value decides the bag).
- Per-option reset: small "↺" button per row — Instance scope: remove the key from the instance bag ("Reset to global"); Global: remove from global bag ("Reset to default"). Tooltip text accordingly.
- Footer: tag THIS SLICE/GLOBAL + mono breadcrumb `config.json → widgets.instances["apps.slot4"].weather` / `widgets.global["weather"]`.
- Missing/uninstalled widget (Custom id not in registry): instead of the card, the chip shows "missing widget" summary + a Reinstall button → opens the store dialog (Task 3). Incompatible: show the reason line.
- For widget slices, the Appearance row hides the icon input (keep color + visibility) and shows the design's explanatory line.

Tests: option-control mapping helper (OptionSpec → control kind enum), humanize_unit, write-target selection (scope → bag path), reset behavior, affected-instances count. Commit: `feat(settings): schema-driven widget options card with scope toggle`

### Task 3: Downloader dialog + install pipeline

**Files:** create `widget_store.rs`; modify `main.rs` (modal state + messages), `Cargo.toml` (+ oxidemx-widget-cli lib dep, rfd if the app already uses it for file dialogs — check existing "From file…" icon picking in buttons/mod.rs and reuse that mechanism).

- Modal overlay (check how settings-rs does modals/dialogs today — theme editor? confirm dialogs? follow that pattern; if none, a stacked container swap on a `State.store_open: bool`).
- Content per design `WidgetStoreDialog`: header, search input (filters the list), list of INSTALLED widgets for v1 (id/name/author/version/signature fingerprint chip/enabled state — there is no enabled toggle in config; show Uninstall + "Settings" jump instead; note deviation: design showed a registry catalog — v1 lists installed + the stub footer), footer: STUB tag + "drop a .omxw into ~/.config/oxidemx/widgets/" + **Install from file…** + **Install from URL…** row.
- Install from file: file dialog (reuse existing mechanism) → `oxidemx_widget_cli::install(path, force: false)`-equivalent lib call; on UnknownKey/Unsigned error surface a confirm sub-dialog showing the fingerprint → retry with force. On success: rescan registry, refresh `widget_registry`, show toast/status line (follow existing settings-rs status conventions).
- Install from URL: text_input + button → async download (reqwest — check settings-rs deps; the geocoder already uses HTTP, reuse its client pattern) to a temp file → same install path.
- Uninstall: confirm → remove `widgets_dir()/<id>` → rescan. Settings bags are kept (spec §9). Slices referencing it now show the missing-widget chip (Task 2 already renders it).
- Reinstall button on missing-widget chip opens this dialog.

Tests: pure helpers only (URL filename derivation, list filtering). Commit: `feat(settings): widget store dialog — install from file/URL, uninstall`

### Task 4: Manual verification pass + polish

- [ ] Build + run settings app against a temp XDG_CONFIG_HOME with the weather widget installed (script `scripts/settings-widget-demo.sh` mirroring widget-smoke.sh: installs weather via CLI, seeds a config, launches `oxidemx-settings`). Walk: picker open → pick weather → options card appears → set location via geocoder → flip scope → check config.json contents (print them in the script after a sleep, or just document the manual steps and verify config writes via a follow-up cat).
- [ ] Fix what the walk reveals (this is where iced layout panics/jank get caught).
- [ ] Commit: `fix(settings): widget UI polish from manual pass`

### Task 5: Live previews (best-effort)

Embed a preview host in settings-rs: spawn `oxidemx_widget_host::worker` with a synthetic one-slice config for the selected widget; render incoming Scenes in (a) the options-card wedge preview (canvas, reuse `overlay-rs`'s replay? — it lives in overlay-rs; either factor `draw_custom_widget`'s prim-walk into `oxidemx-widget-host` behind a `render-iced` feature OR copy the ~120-line replay into settings-rs with a comment; prefer the factor-out if it doesn't drag overlay deps) and (b) picker tiles at 1fps... 

**Scope ruling:** implement (a) only — options-card live wedge preview re-rendering on option change (send ConfigChanged with the edited bag to the preview worker). Picker-tile live previews stay static in v1 (deviation noted; design allows pausing them when hidden anyway). If factoring the replay turns into an overlay-rs refactor bigger than ~an hour, STOP, leave the static wedge preview (canned values like the design mockup), report DONE_WITH_CONCERNS, and we ship without it.

- [ ] Commit: `feat(settings): live wedge preview in widget options card` (or the concerns report)

### Task 6: Hygiene

- [ ] `cargo test --workspace`, `cargo clippy -p oxidemx-settings -- -D warnings` (fix new warnings only), smoke scripts still pass, update `docs/widgets/authoring.md` if any user-facing flow changed. Commit: `chore(settings): widget UI hygiene sweep`

## Self-review notes
- Spec §10 coverage: chip 10b ✓ T1; picker 10c ✓ T1 (live minis → static, deviation); options card 10d ✓ T2 (debounce → immediate, deviation); missing chip 10e ✓ T2/T3; downloader §11 ✓ T3 (catalog browsing explicitly out of v1 scope per spec non-goals).
- Keyboard nav (arrows/Esc/Enter roving focus) from the design's interaction table is NOT planned — iced 0.14 focus APIs make this a sizeable chunk; explicitly deferred as a follow-up, record it in docs/plans/followups.md during Task 6.
- Type consistency: `WidgetSummaryLite` mirrors host `WidgetSummary` + `SignatureState`; instance keys always via `oxidemx_shared::widgets::instance_key`; option writes use the same `JsonBag` types as Plan 1.
