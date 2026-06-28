# Menu Open/Close Animations — Design (Slice B of the UI-improvements program)

**Date:** 2026-06-27
**Branch / worktree:** new branch off `2b-collapsible-panels`
**Goal:** Animate our popup menus open AND closed like the Projects `Select` dropdown — by adopting the
Select animation pattern in our two menu primitives (`Popover` + `OxideContextMenuViewer`), so every
menu inherits it. Frontend-only, all in `oxide-ui`.

## Approach (pre-approved: enhance, don't migrate to Freya `Menu`)

Both primitives adopt the **Freya `Select` animation pattern** (`repos/freya/.../select.rs:140-172`):
a *persistent* `use_animation` (`OnChange::Rerun` + `OnCreation::Finish`) driving `scale`, `opacity`,
and a small `offset_y` slide; on close it `into_reversed()` **in place**, and the node stays mounted
while `opacity > 0` so the exit tween plays before unmounting. The Freya `Select` (project switcher in
`sidebar_header.rs`) already animates — no change.

**Animation params (match Select exactly):** for each of scale / opacity / slide,
`AnimNum::new(a, b).time(125).ease(Ease::Out).function(Function::Quart)`:
- `scale = AnimNum::new(0.9, 1.)`
- `opacity = AnimNum::new(0., 1.)`
- `slide` (offset_y) = placement-aware: `AnimNum::new(SLIDE_FROM, 0.)` where `SLIDE_FROM = -8.` when the
  menu opens **Below** the anchor (reveals downward) and `+8.` when **Above** (reveals upward). The
  context menu (cursor-anchored, always opens downward) uses `-8.`.
- On `open` → `(scale, opacity, slide)`; else `(scale.into_reversed(), opacity.into_reversed(),
  slide.into_reversed())`. Read once: `let (scale, opacity, slide) = animation.read().value();`.

## Task 1 — `Popover` (`oxide-ui/src/components/menu/popover.rs`)

Today: a per-mount entrance *fade* (`use_animation(OnCreation::Run)`, opacity-only, 120ms) inside the
`PopoverOverlay` subcomponent, which is mounted only while `open=true` and **unmounts instantly on
close** (no exit animation). Migrate to the persistent pattern:

1. **Move the animation hook into `Popover::render`** (always mounted): the persistent
   `use_animation(OnChange::Rerun + OnCreation::Finish)` reading the builder's `open` bool, returning the
   forward/reversed `(scale, opacity, slide)` tuple. `slide`'s `SLIDE_FROM` derives from the
   **effective** placement (after the existing auto-flip), so the slide direction matches where the menu
   actually opens.
2. **Keep the overlay mounted while fading out:** change the overlay-mount gate from `open` to
   `open || opacity > 0.0`.
3. **`PopoverOverlay`**: drop its own `use_animation`; add `scale: f32`, `opacity: f32`, `slide: f32`
   fields passed from the parent. In render apply `.scale(scale).offset_y(slide)` and
   `.opacity(if positioned { opacity } else { 0.0 })` to the positioned overlay rect (the slide is added
   on top of the absolute `off_top` via `.offset_y`, NOT by mutating `off_top`).
4. **Cleanup:** when `!open && opacity == 0.0 && content_size().is_some()`, clear `content_size` so the
   next open measures fresh (mirrors Select's cleanup).
5. **Don't re-measure during close:** the `on_sized` handler that reports `content_size` must only fire
   while opening/positioned-unknown — guard it so a mounted-but-fading overlay doesn't overwrite the
   measurement (which feeds positioning). 

**Edge-aware positioning math is untouched** (`EDGE_MARGIN`, clamp, auto-flip, `off_top`/`off_left`):
the animation only multiplies `.scale()`/`.opacity()`/`.offset_y()` on the already-positioned rect.
Inheritors: ProviderMenu + AttachMenu (`composer/toolbar.rs`), collapsed project dropdown + icon picker
(`regions/sidebar.rs`).

## Task 2 — `OxideContextMenuViewer` (`oxide-ui/src/components/menu/context_menu.rs`)

Today: no animation; opacity flips 0→1 once measured, instant unmount on `close_context_menu`. Add the
persistent animation:

1. **Persistent hook** in `OxideContextMenuViewer::render`, open-state = `ctx.menu.read().is_some()`;
   returns `(scale, opacity)` forward/reversed (context menu slide uses `-8.`; include it too if cheap).
2. **Persist the last menu so it survives the fade:** a root-scoped `last_menu: State<Option<(CursorPoint, Menu)>>`
   updated (via `use_side_effect`) whenever `ctx.menu` becomes `Some`; render from `ctx.menu.read().or(last_menu)`.
   Keep the menu node mounted while `ctx.menu.is_some() || opacity > 0.0`.
3. **Apply** `.scale(scale)` + `final_opacity = measure_gate * anim_opacity` (measure_gate = 0 until
   measured, then 1) to the menu rect.
4. **Dismissal during fade:** because the menu node stays mounted (rendering `last_menu`) while fading,
   the Freya `Menu`'s `on_close` (click-away) + the `CloseReq` debounce keep working — verify in live
   test. Clear `measured` once `opacity == 0` so the next open re-measures at the new cursor.

Inheritors: right-click clipboard menu (`text_input.rs`/`bubble.rs`), paste-attachment menu
(`composer/editor.rs`), conversation-row Rename/Set-icon menu (`regions/sidebar.rs`).

## Data flow
```
open=true  ──▶ persistent anim plays forward (scale 0.9→1, opacity 0→1, slide ±8→0) ──▶ menu reveals
open=false ──▶ anim reverses in place; node stays mounted while opacity>0 ──▶ exit tween ──▶ unmount + clear cache
```

## Error / edge handling
- Animation is the LAST visual layer — if `use_animation` ever yields odd values, the menu still renders
  (positioning is independent). Worst case is a visual glitch, never a crash.
- Center-origin `.scale()` may shift a menu slightly sideways; if live test shows drift, switch that
  rect to a `transform`/scale-from-top-anchor (note as a live-tunable, not a blocker).
- Cached measurement (`content_size` / `measured`) cleared on close so a window resize during the close
  tween can't leave a stale position on the next open.
- Context menu: the existing CloseReq "ignore-first-close" debounce is preserved.

## Testing
- **Snapshots** (render + read-back): poll the animation to a MID-OPEN frame (~60ms, scale≈0.95,
  opacity≈0.6) and assert the menu renders partially-scaled — for one Popover menu (e.g. the icon picker
  or a toolbar menu) and the context menu. Snapshots can't show motion, so they prove the animated values
  are applied at a mid-frame, not the full curve.
- **Unit** (if a pure helper falls out, e.g. `slide_from(placement) -> f32`): assert Below→-8, Above→+8.
- **Live**: open + close each menu (model picker, attach, project dropdown, icon picker, right-click
  clipboard, conversation Rename/Set-icon) — confirm both directions animate, click-away still dismisses,
  positioning/edge-flip unchanged, no sideways scale drift.
- Headless can't show the motion or drive dismissal timing — those are live-verified.

## File structure
| File | Responsibility |
|------|----------------|
| `oxide-ui/src/components/menu/popover.rs` | persistent Select-style anim (scale/opacity/placement-slide) + keep-mounted-while-fading + measurement cleanup |
| `oxide-ui/src/components/menu/context_menu.rs` | same anim + persist-last-menu-while-fading + dismissal-during-fade + measured cleanup |

## Decomposition / sequencing (2 tasks)
1. `Popover` migration → all Popover menus animate open+close.
2. `OxideContextMenuViewer` animation → all right-click menus animate open+close. + final verify.

## Out of scope
- Freya `Select` (already animates).
- New menu types / restructuring the menu system.
- Slice C (menu interrupt-closure: toggles/radio/submenu keep the menu open) — separate.
- Per-item hover/press animations.
