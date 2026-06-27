# Shell on Freya 0.4 `ResizableContainer` — Design

**Date:** 2026-06-27
**Branch / worktree:** `rightpanel-shell-tabs` / `oxidemx-rightpanel` (added to the current slice before merge)
**Why:** A live bug — when the right panel is expanded (348px) and the left sidebar is then
collapsed, the right panel squeezes to a vertical sliver while still rendering expanded content.

## Root cause

Each side panel hand-rolls its width with a `use_animation` driven by a plain `collapsed: bool`
prop + `OnChange::Rerun`. When one panel animates (340ms of continuous shell re-renders), the
OTHER panel's animation re-fires and never settles → its width collapses to a sliver. Two
hand-rolled width animations on one flex row destabilize each other; the manual `Content::Flex`
row is fragile besides. (A deeper instance of `[[feedback_freya_use_animation_scoping]]` +
`[[feedback_freya_flex_row_layout]]`.)

## Fix — adopt Freya 0.4 `ResizableContainer`

Replace the hand-rolled `CollapsiblePanel` + per-panel width `use_animation` + the manual
`Content::Flex` row with Freya 0.4's resizable-panel primitives, which own sizing/layout
internally (nothing to cross-fire) and add drag-to-resize. This matches the design —
`freya2-right.jsx` is annotated `data-freya="ResizableContainer"`. (We use the `ResizableContainer`
primitives, NOT the full `DockingArea` drag-rearrange tree, which is overkill for a fixed
3-region shell.)

### API (verified in `freya-blog04/crates/freya-components/src/resizable_container.rs`)
- `ResizableContainer::new().direction(Direction::Horizontal).panel(p0).panel(p1).panel(p2)` —
  renders a `rect().direction(...).content(Content::flex()).expanded()` and **auto-inserts a
  `ResizableHandle` between panels** (do NOT add handles manually).
- `ResizablePanel::new(PanelSize)` where `PanelSize::px(v)` / `PanelSize::percent(v)`; builders
  `.min_size(f32)`, `.child(elem)`, `.key(DiffKey)` (impls `KeyExt`/`ChildrenExt`).
- A panel registers its `initial_size` via `use_hook` (once); the live size lives in the
  container's internal `ResizableContext`. To change a panel's size from state, **re-key the panel**
  (`.key(...)`) so it remounts with a new `initial_size` — no custom `controller` needed (the
  container provides a default `ResizableContext` that the handles drive for drag-resize).

## Architecture

### Shell (`oxide-freya/src/app.rs`)
```
ResizableContainer::new().direction(Direction::Horizontal)
  .panel(ResizablePanel::new(PanelSize::px(if sidebar_collapsed {60} else {274}))
           .min_size(60.).key(sidebar_collapsed).child(Sidebar { state, collapsed: sidebar_collapsed }))
  .panel(ResizablePanel::new(PanelSize::percent(100.)).min_size(320.)
           .child(MainRegion { state }))
  .panel(ResizablePanel::new(PanelSize::px(if context_collapsed {60} else {348}))
           .min_size(60.).key(context_collapsed).child(ContextRegion { state, collapsed: context_collapsed }))
```
- `sidebar_collapsed` / `context_collapsed` are the EFFECTIVE values (`user signal || size_class.is_compact_or_narrower()`), computed in `shell()` exactly as today (the size-class probe stays).
- Re-keying a panel on its collapsed flag forces a remount with the new `initial_size` → the
  container redistributes width. Width is owned by the container; **no hand-rolled width animation**.
- The size-class probe (full-window logical-width rect) stays as a sibling/overlay — it is global
  (out of flow), so it does not participate in the `ResizableContainer`'s panel set. Keep
  `OxideContextMenuViewer` + the connection banner mounted outside/around the container as today
  (the container `.expanded()`s, so wrap it in the shell root rect; the menu viewer + probe are
  overlay/global and layout-neutral).

### Side panels (`sidebar.rs`, `context/mod.rs`)
- **Remove** the width `use_animation` + the `anim_w`-driven `rect().width(Size::px(anim_w))`.
  The panel now fills its `ResizablePanel` (`.width(Size::fill()).height(Size::fill())`); the
  CONTAINER sets the width. Keep the `collapsed: bool` prop ONLY to switch CONTENT (full vs rail).
- `Sidebar`: render the full column when `!collapsed`, the icon rail when `collapsed`, filling the
  panel. `«`/`»`/rail buttons still write the USER signal (`sidebar_collapsed`).
- `ContextRegion`: same — render the 348 tab panel when `!collapsed`, the 60 icon rail when
  `collapsed`, filling the panel. Tab header + rail nav + `»` unchanged; drop `anim_w` (use
  `Size::fill()`).

### Retire
- `oxide-ui/src/components/collapsible_panel.rs` (`CollapsiblePanel`) — no longer used; delete it +
  its `mod`/`pub use`. (Confirm no other consumers first; if any remain, leave it.)

### Collapse animation (scope note)
Re-keying gives an **instant** collapse (no tween). The previous hand-rolled tween is what caused
the bug, so instant is the safe baseline. An animated collapse (tweening a single panel size via a
provided `controller: Writable<ResizableContext>`) is a possible follow-up polish — NOT in this
slice unless the user wants it. (One animated value written to the container's controller would not
cross-fire the way two competing per-panel animations did, but it adds complexity; defer.)

## Data flow
```
collapse/expand button ──writes user signal (sidebar_collapsed / context_collapsed)
size-class probe ──writes size_class
shell() ──effective collapsed = user || compact──▶ ResizablePanel initial_size + .key()
ResizableContainer ──owns width distribution + drag-resize (handles)──▶ panels fill their width
```

## Error / edge handling
- `min_size(60)` on side panels prevents drag-collapsing below the rail; `min_size(320)` keeps the
  center usable.
- Re-key on collapse resets that panel's drag-resize to the keyed `initial_size` (acceptable — a
  discrete collapse is a deliberate reset).
- No panic paths added; content-switch is a simple `if collapsed`.

## Testing
- **Spike first (Task 1):** stand up the 3-region `ResizableContainer` in the shell with collapse
  re-key; snapshot Wide + Compact + the regression case (right expanded + left collapsed) — the
  bug must be gone (right panel stays 348, not a sliver). Controller reads the PNGs.
- Unit: nothing new (layout is visual).
- Snapshots (dark, read-back): Wide (full sidebar + chat + right rail), Compact (both rails), the
  **regression case** (context expanded + sidebar collapsed → both correct), and a drag-resized
  state if feasible.
- Full suite (85+) stays green; clippy clean.

## File structure
| File | Change |
|------|--------|
| `app.rs` | shell → `ResizableContainer` + 3 `ResizablePanel`s (re-key on collapse); keep probe + effective-collapsed; update snapshot harness(es) |
| `regions/sidebar.rs` | drop width animation; fill the panel; keep collapsed content-switch + buttons |
| `regions/context/mod.rs` | drop `anim_w`/width animation; fill the panel; keep tabs/rail/collapse content |
| `oxide-ui/.../collapsible_panel.rs` | delete (retire) if no other consumers |

## Out of scope
- Animated collapse (instant via re-key this slice; controller-tween is a possible follow-up).
- Full `DockingArea` drag-rearrange/tabs-between-panels.
- The later right-panel slices (rail agent-status + popovers, real Run/Worktree/.oxide data, editor,
  tablet/phone) — unchanged.

## Spike findings (2026-06-27)

Validated via `spike_resizable` test (see `app.rs`) at 1200×800. All PNGs written to `/tmp`.

### Exact API used
```rust
ResizablePanel::new(PanelSize::px(274.))
    .min_size(60.)
    .key((0u8, sidebar_collapsed))   // ← tuple to disambiguate left vs right panel keys
    .child(Sidebar { ... })
```
- `PanelSize::px(v)` / `PanelSize::percent(v)` — pixel or flex weight
- `.min_size(f32)` — minimum size in logical pixels
- `.key(impl Hash)` — `KeyExt` accepts any hashable value (bool, usize, tuple, …); creates a `DiffKey::U64` by hashing

### Key disambiguation (critical)
`.key(bool)` alone is **not safe** when two sibling panels both use a plain `bool`. If both
collapse flags are `false` (both expanded), both panels get the same `U64` hash → Freya panics
with `"duplicate sibling key"`. **Always include a panel-position discriminant** in the key tuple:
`.key((0u8, sidebar_collapsed))` for the left panel, `.key((2u8, context_collapsed))` for the right.

### Re-key behaviour
`.key(...)` change → Freya unmounts the old panel and mounts a fresh one with the new
`initial_size` registered via `use_hook`. The container's `ResizableContext` redistributes the
remaining flex space immediately (instant, no tween). The right panel correctly stayed at 348 px
when the left panel collapsed from 274 px to 60 px — the regression is fixed.

### Available from `freya::prelude::*`
All types — `ResizableContainer`, `ResizablePanel`, `PanelSize`, plus the `KeyExt`/`ChildrenExt`
traits — are re-exported via `freya_components::resizable_container::*` which is re-exported
through `freya::prelude::*` (line 214 of `freya/src/lib.rs`). No extra imports needed.

### No manual handles
`ResizableContainer` auto-inserts `ResizableHandle` between panels. Do NOT add `ResizableHandle`
manually — it will create duplicate handles and mis-count panel indices.

### Instant collapse
Re-keying gives an instant collapse with no tween. The previous hand-rolled `use_animation`
tween was the root cause of the cross-fire bug; instant collapse is the correct baseline.

### Live-run fix (2026-06-27): percent-panel `min_size` units
RUNTIME panic (snapshots did NOT catch it — only triggers when available space < the min, e.g.
a narrow window or a drag): `clamp(min=320, max=100): min > max` from Freya's resize logic. Cause:
the CENTER panel is `PanelSize::percent(100)` but was given `.min_size(320.)` — for a **percent**
panel `min_size` is in PERCENT units, so 320(%) > 100(%) panics. **Fix: drop the center's
`min_size`** (Freya's default for a percent panel = `initial_value * 0.25` = 25%, which is correct).
Rule: `min_size` on a `PanelSize::percent(v)` panel must be a percent ≤ `v`; `min_size` on a
`PanelSize::px` panel is pixels. (Left/right px panels keep `min_size(60.)`.)

## Resize-semantics refinement (live feedback 2, 2026-06-27)

Two issues from live drag-testing of the flat 3-panel `ResizableContainer`:
1. **Collapsed side panels were still resizable** (a handle next to a 60px rail).
2. **Dragging one side panel moved the OPPOSITE side**, and the center "retained" its width.

**Root cause (traced in `resizable_container.rs::apply_resize`):** for a drag, the algorithm grows
the panel immediately *behind* the handle and shrinks the *forward* panels **in positional order
starting from index 0** (breaking at the first panel above its min). In a flat `[left|center|right]`,
dragging the RIGHT handle inward (negative) sets `forward=[left, center]` and shrinks **left first**
— so the right panel grows at the left panel's expense, bypassing the center. The center only absorbs
in one drag direction. This generic cascade does not match "each side resizes against the center."

**Fix — NESTED containers + collapse-aware structure** (the center always flexes; a collapsed side is
a plain fixed rail with NO handle):
```
right_group =
  if context_collapsed: rect(Horizontal, Content::Flex)[ MainRegion(flex(1.0)) | ContextRail(px 60) ]   // no handle
  else:                 ResizableContainer(Horizontal)[ MainRegion(percent 100) | ContextRegion(px 348, min 60) ]
shell_body =
  if sidebar_collapsed: rect(Horizontal, Content::Flex)[ SidebarRail(px 60) | right_group(flex(1.0)) ]   // no handle
  else:                 ResizableContainer(Horizontal)[ Sidebar(px 274, min 60) | right_group(percent 100) ]
root = rect(Vertical).expanded()[ probe(overlay) | OxideContextMenuViewer | banner? | shell_body ]
```
- Outer container (when sidebar expanded) resizes left ↔ right_group; right_group is a flex unit, so
  the center inside it absorbs and the RIGHT panel (px) stays. Inner container (when context expanded)
  resizes center ↔ right; the left (outer) is untouched. → each side resizes ONLY against the center.
- A collapsed side is a plain fixed `px(60)` rect OUTSIDE any container → no `ResizableHandle` →
  not resizable. The structure SWAPS (ResizableContainer ↔ rect) on collapse toggle, which remounts
  cleanly (no re-key needed; collapse resets drag state, which is acceptable).
- Center `min_size` stays UNSET (percent default 25%); side px panels keep `min_size(60)`.

## Resize-handle styling (live feedback 3, 2026-06-27)

The auto-inserted `ResizableHandle` paints a 4px bar with its theme `background` at REST (a visible
colored bar the user dislikes) and `hover_background` on hover/drag. Fix: register a
`resizable_handle` theme preference so the handle is **transparent at rest** and only highlights on
hover:
- `background` → the PANEL colour `th.panel()` (NOT transparent — transparent shows the darker shell-root through the 4px gap as a black line; panel-colour blends since center bg()=18,20,24 ≈ panel()=15,17,23).
- `hover_background` → a subtle highlight (`Theme::with_alpha(accent, ~0x40)` or `hairline_strong()`).
- `HANDLE_SIZE` is 4px (fine; the gap itself is thin) — only the COLOR changes.
Set via the app theme (customize the `Theme` passed to `use_init_theme` to include the
`resizable_handle` `ThemePreference`, mirroring how Freya component theme preferences are registered).
