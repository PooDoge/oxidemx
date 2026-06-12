# Slice Editor Redesign Implementation Plan (Plan 5)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development.

**Goal:** Restructure the settings Menu tab's slice editor to the design handoff's layout: a vertical list of compact **reorder rows** (one per slot) with the selected slice expanding **inline** as the full slice card — replacing today's "radial preview + single pinned editor below" arrangement — plus the design's card chrome and remaining picker polish.

**Design truth:** `docs/design-system/menu-page.jsx` (`SlotRow`, `SliceCard`, `MenuSlicesPage` layout) + `docs/design-system/Widget Selector Spec (Standalone).html` §3 — the repo copy of the mockups; spec §10 in `docs/superpowers/specs/2026-06-12-widget-plugin-system-design.md`. Visual vocabulary (sizes, tags, ordering) comes from the JSX; reuse `oxidemx-widgets` style helpers for the app's existing look.

**Scope guardrails:** view-layer restructure of `settings-rs/src/tabs/buttons/` — existing Messages/handlers are reused wherever possible (SelectSlice = expand, MoveSliceUp/Down, DeleteSlice, AddSlice, TestSliceAction…). The radial preview card stays (above the list, as in the design's MenuSlicesPage). Page picker card and Easy-Switch panel stay as today. Submenu editor stays inside the expanded card unchanged. NO new persistence, NO keyboard nav (still deferred), NO drag-reorder (arrows only, like the design's reorder handles).

### Task 1: reorder-row list + inline expansion
`buttons/mod.rs` `view()`: after the preview/ES cards, render a "Slice editor" section head (with **+ Add slice** button on the right, design's `JRSectionHead`) followed by one element per slot index 0..slot_count:
- **Collapsed row** (new `slot_row()` in a new `buttons/rows.rs`): up/down chevrons (stacked, design's reorder handle → `MoveSliceUp/Down`, disabled at ends), 15px icon tinted slice color, label (or "(empty)" dim for empty slots) + small `Slot {n}` tag, one-line summary (REUSE `picker::chip_summary` — export it), kind tag (ACTION/WIDGET/SUBMENU/DIAL/… per design's `jr-reorder-row-kind`), and the row is clickable → `SelectSlice(idx)`.
- **Expanded**: when `state.selected_slice == Some(idx)` render the full slice card (Task 2) in that list position instead of the row. Clicking another row moves the expansion (existing SelectSlice semantics; `DismissSliceSelection` on the card's collapse control).
- Delete the old `selected_slice_editor`-below-preview placement; the radial preview's click-to-select keeps working (it now scrolls… no scroll API — fine, the expansion just appears in-list).
- Empty slots: collapsed row shows "(empty)" + an **Assign…** affordance that selects the slot (the card on an empty slot starts with the picker open — set `picker_open` in the SelectSlice handler ONLY when the slot's kind is None/empty; do it in update, not view).

### Task 2: slice card chrome per design
Extract the expanded editor into `buttons/card.rs` wrapping the existing pieces in the design's order/chrome (`SliceCard`): header row `SLOT {n}` mono tag + `WIDGET` accent tag when applicable + spacer + `Test` button + move up/down + delete (reuse existing messages; remove these controls from wherever they currently live to avoid duplicates); then label input (with dim "auto label" tag when the label equals the widget's auto-label) + description input; then "Slice behavior" field label + behavior chip/picker/options-card stack (existing `picker::behavior_section` + options card — unchanged calls); then **Appearance** field (color swatch select + icon input/browse buttons for non-widget slices, the widget explanatory line for widget slices — this content exists, just regroup under the one labeled section) with the Visibility selector right-aligned in the same row (existing visibility_editor's simple variant — if it doesn't fit one row, stack it; match the design's intent, note what you did).

### Task 3: polish
- Picker grid 4-up when the window is wide: settings-rs tracks window size? If a size is already in State use it; else wrap the grid in `iced::widget::responsive` (check availability in the vendored iced 0.14) — if neither is workable in <30 min, keep 3-up and note it.
- "Get more widgets…" tile: bordered tile in the grid flow (it currently renders as a full-width row below — move it into the grid as the last tile, quiet outline style).
- Breadcrumb context line above the card when expanded: `Menu › Slice editor › Slot {n} · {label}` (design's SliceCardFocus header), small/dim.
- Sub-item editors keep their compact form but get the same summary helper for consistency if trivial.

### Task 4: gates
`cargo test -p oxidemx-settings` (existing 44+ stay green — view restructure shouldn't break message tests; fix any that asserted old view structure), `cargo check -p oxidemx-overlay` (untouched), clippy settings `-D warnings`, `bash scripts/settings-widget-demo.sh`, `bash scripts/widget-smoke.sh`. Update `docs/plans/followups.md` (drag-reorder, keyboard nav, scroll-to-expanded) if not present. Single review pass after.

Commits: T1 `feat(settings): slice editor reorder-row list with inline expansion`; T2 `feat(settings): slice card chrome per design handoff`; T3 `feat(settings): picker grid + card polish`; T4 `chore(settings): editor redesign gates + followups`.
