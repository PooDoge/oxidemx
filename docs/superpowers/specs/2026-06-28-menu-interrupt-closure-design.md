# Menu Interrupt-Closure (light-dismiss + per-item auto-dismiss) — Design (Slice C)

**Date:** 2026-06-28
**Branch / worktree:** new branch off `2b-collapsible-panels`
**Goal:** Inside a menu, clicking a non-terminal control (reasoning radio, a toggle, the
Composer-settings submenu/expander) must KEEP the menu open; only a *terminal* item (picking a
model), an outside click, or Escape dismisses. Frontend-only, all in `oxide-ui`. Scoped to the
composer `ProviderMenu`/model-picker (the only rich menu); other menus are untouched.

## Root cause (confirmed via live test + devtools session)

Freya `Menu` dismisses through a **no-hit-test** `on_global_pointer_press` → `on_close` that
fires on *every* pointer press, inside or outside; the `Menu`'s `on_press(stop_propagation)` does
NOT stop a global handler. So every inside click closes the menu (the Composer-settings expander
"flashes then dismisses" because the same click swaps the view for one frame, then the global
press unmounts the whole menu). This is fine for Freya's simple menus (click item → close) but
wrong for a rich menu with persistent controls.

## Approach (pre-approved)

Two independent axes, both opt-in, leaving the other menus on Freya's current any-click-close:

1. **Menu-level — light-dismiss.** The menu stops auto-closing on inside clicks; it closes only on
   an **outside press** (bounds-checked) or **Escape**. (Term: HTML-popover `auto` / WinUI "light
   dismiss".)
2. **Item-level — auto-dismiss.** Each interactive row declares whether activating it *also* closes
   the menu. Regular items close; toggle/submenu/nav rows stay open. Overridable per row.

`auto_dismiss` is meaningful only when `light_dismiss` is on (otherwise Freya's any-click-close
already dismisses everything). The two ship together and are wired only on `ProviderMenu`.

## Components

### 1. `MenuDismiss` context (new) — `oxide-ui/src/components/menu/mod.rs` (or `surface.rs`)

A `#[derive(Clone, Copy)] struct MenuDismiss(EventHandler<()>)` provided via `use_provide_context`
by `MenuSurface` when `light_dismiss` is on. Any descendant row can `use_try_consume::<MenuDismiss>()`
and call it to ask the menu to close. `EventHandler<()>` is `Copy`, so the context is `Copy`.

### 2. `MenuSurface::light_dismiss(bool)` — `oxide-ui/src/components/menu/surface.rs`

New builder field `light_dismiss: bool` (default `false`). In `render`, when `true`:
- **Do NOT** thread `on_close` into the inner Freya `Menu` (so the Menu's naive global-press +
  Escape dismissal never fires). When `false`, behavior is exactly as today (on_close threaded).
- Provide `MenuDismiss(on_close)` via context (only when an `on_close` is set).
- On the outer surface `rect`: measure its area with `.on_sized(|e| area.set(Some(e.area)))`, and add:
  - `.on_global_pointer_press(move |e| { if let Some(a) = area.peek().as_ref() { if !a.contains(e.global_location()) { on_close.call(()); } } })`
  - `.on_global_key_down(move |e| if e.key == Key::Named(NamedKey::Escape) { on_close.call(()); })`
- The outer rect already carries the deep shadow + `Content::fit` + min/max width; the measured
  `area` is the menu's window-space rectangle (`on_sized` area and `global_location()` are both
  window-space — **verify during implementation**; if they differ, translate before `contains`).

Because `MenuSurface` is mounted only while the popover is open, the *opening* click (on the
trigger pill) happens before mount, so the global handler never sees it — no self-close, no
opening-click guard needed (same reason today's Freya `on_close` doesn't self-close on open).

### 3. `MenuRow::auto_dismiss(bool)` — `oxide-ui/src/components/menu/row.rs`

New builder field `auto_dismiss: bool` (**default `true`** — a regular item). In `render`, the
`MenuButton::on_press` closure, after calling the row's `on_press`, also closes the menu when the
row auto-dismisses:

```rust
let auto_dismiss = self.auto_dismiss;
let dismiss = use_try_consume::<MenuDismiss>();   // None when not under a light_dismiss surface
// ...
MenuButton::new()
    .theme(item_theme)
    .on_press(move |_: Event<PressEventData>| {
        if let Some(h) = &on_press { h.call(()); }
        if auto_dismiss { if let Some(d) = dismiss { d.0.call(()); } }
    })
    .child(inner)
```

When there is no `MenuDismiss` context (menu not in light-dismiss mode), `auto_dismiss` is a no-op
and the row behaves as today.

### 4. `ProviderMenu` wiring — `oxide-ui/src/components/composer/provider_menu.rs`

- `MenuSurface::new(th)...light_dismiss(true)` on the surface in `ProviderMenu::render`.
- **Model rows** (`model_row`, a raw `MenuButton`): treat as regular item → on press, after
  `on_select_model`, call the `MenuDismiss` context to close. (Mirror the MenuRow logic inline,
  since `model_row` builds the `MenuButton` directly.)
- **Composer-settings expander** + **Back row** (both `MenuRow` with `on_press` that swaps `View`):
  `.auto_dismiss(false)` → stay open, swap view in place.
- **Reasoning segmented control** + **Prompt-optimizer / Send-on-Enter toggle rows**: unchanged —
  they mutate via `SegmentedButton`/`Switch` with no row-level `on_press`, so nothing dismisses.
- **`composer/mod.rs`**: remove the now-redundant `provider_open.set(false)` from the
  `on_select_model` handler (closing is declarative via the model row's auto-dismiss). `on_close`
  (→ `provider_open.set(false)`) stays — it is the `MenuDismiss` target and the outside/Escape sink.

## Data flow

```
outside press / Escape ─▶ MenuSurface (light_dismiss) bounds-check / key ─▶ on_close ─▶ provider_open=false
model row press        ─▶ on_select_model (select)  +  MenuDismiss ─▶ on_close ─▶ provider_open=false
toggle / radio / expander press ─▶ own handler only (no MenuDismiss) ─▶ menu stays open
```

## Error / edge handling

- No `MenuDismiss` context (menu not light-dismiss) → `auto_dismiss` is inert; existing menus
  keep Freya's any-click-close. Zero behavior change off the opt-in path.
- Coordinate-space mismatch between `on_sized` area and `global_location()` is the one
  implementation risk: verify in the live build; if they differ, translate the point into the
  area's space before `contains`. Worst case is a mis-aimed outside-click, never a crash.
- Light-dismiss replaces Escape handling that previously lived on the Freya `Menu`; the
  `on_global_key_down` Escape on the surface preserves it.

## Testing

- **Unit:** (a) a pure `area_contains(area, point) -> bool` helper (Below/edge/inside cases);
  (b) mount a `MenuSurface::light_dismiss(true)` with a `MenuRow::auto_dismiss(false)` and a
  spy `on_close`, assert pressing that row does NOT call `on_close`; and a `auto_dismiss(true)`
  row DOES. (`freya_testing` can drive a press; it cannot drive a real global outside-press.)
- **Live (controller-verified):** in the model picker — model pick closes; reasoning Low/Med/High
  stays open; Prompt-optimizer + Send-on-Enter toggles stay open; Composer-settings expander swaps
  to settings IN PLACE (no dismiss) and Back returns; outside-click closes; Escape closes. Confirm
  the other menus (attach, project dropdown, icon picker) are unchanged (still close on pick).

## File structure

| File | Responsibility |
|------|----------------|
| `oxide-ui/src/components/menu/mod.rs` (or `surface.rs`) | `MenuDismiss` context type + re-export |
| `oxide-ui/src/components/menu/surface.rs` | `light_dismiss` field; bounds-checked global-press + Escape; provide `MenuDismiss`; gate Freya `on_close` |
| `oxide-ui/src/components/menu/row.rs` | `MenuRow::auto_dismiss` field; call `MenuDismiss` on press when set |
| `oxide-ui/src/components/composer/provider_menu.rs` | `light_dismiss(true)`; model-row dismiss; `auto_dismiss(false)` on expander + Back |
| `oxide-ui/src/components/composer/mod.rs` | drop redundant `provider_open.set(false)` in `on_select_model` |

## Decomposition / sequencing

1. `MenuDismiss` context + `MenuSurface::light_dismiss` (bounds-checked dismissal + context provision).
2. `MenuRow::auto_dismiss` (consume context, dismiss on press when set) + unit tests.
3. `ProviderMenu` wiring (light_dismiss + model-row dismiss + expander/Back opt-out) + composer/mod.rs
   cleanup; final live verify.

## Out of scope

- The other Popover menus (attach, project dropdown, icon picker) — they keep Freya's any-click-close.
- Freya `SubMenu` (hover-triggered) — ProviderMenu's settings page is an internal `View` swap, kept.
- Per-item hover/press animations; new menu types.
- Close/delete conversation (separate deferred item).
