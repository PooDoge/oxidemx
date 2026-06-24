# Reusable Menu / Popover component — design

**Date 2026-06-24.** From live-test feedback on the Composer menus. Goal: replace the two
hand-rolled menu bodies (`ProviderMenu`, `AttachMenu`) with a small **reusable, Freya-native**
menu/popover layer in `oxide-ui`, fixing four bugs in the process — by *using* the machinery
Freya's `Menu`/`Select` already ship rather than reinventing it (so we ride Freya's upstream
fixes + enhancements).

## The four bugs → root cause → Freya-native fix

1. **Light-on-light row hover (unreadable).** Freya `MenuItem`'s default `hover_background` is a
   light color; our rows draw fixed near-white labels (`th.text()`) and never theme the hover →
   light-on-light. **Fix:** a shared `MenuItemThemePartial` with a DARK `hover_background`
   (`surface_hi`) + `select_background`, applied to every row.
2. **Menu floats far from its trigger.** The orchestrator anchors via `Attached.top()` on the
   *whole toolbar*, not the specific button. **Fix:** a `Popover` anchor wrapper built on Freya
   `Attached` (measures the trigger, positions the content adjacent) + `Select`-style auto-flip
   (`Platform::get().root_size` — flip above/below by available space).
3. **Attach menu fills the composer width.** `AttachMenu` sets no width; the Menu fills its
   parent. `ProviderMenu` hardcodes 280px. **Fix:** content-hug width — `Content::fit()` +
   `min_width`/`max_width` (mirror Freya `MenuItem`'s `fill_minimum()` + `min_width(105)`).
4. **Menus don't dismiss on outside-click / focus-loss.** Freya `Menu` ALREADY emits outside-press
   + Escape via `on_close`, but our `AttachMenu`/`ProviderMenu` never surface it, so the
   orchestrator's `attach_open`/`provider_open` never flips. **Fix:** the `Popover` owns dismissal
   and surfaces a single `on_dismiss` the orchestrator wires to its open-state.

## What Freya already gives us (verified in source — reuse, don't rebuild)

- `Menu` (`freya-components/src/menu.rs`): outside-press close (`on_global_pointer_press → on_close`),
  Escape close, **off-screen `overflow_offset` auto-shift**, **multi-level `SubMenu` nesting** (a
  `MenuId` stack, hover-to-open), content-hug `MenuItem` width (`fill_minimum` + `min_width`).
- `Select` (`select.rs`): **auto-flip** edge detection + the **animated entrance** we want
  (scale 0.9→1, opacity 0→1, slide −8→0; 125ms Quart Out; reversed on close via `.into_reversed()`).
- `Attached` (`attached.rs`): per-trigger anchored positioning (measures inner + overlay `Area`,
  `Position::new_absolute`, hides at `opacity(0)` until measured).
- `Tooltip`/`Popup`: the same `use_animation` + measure-then-reveal pattern.
- Example to copy: `freya/examples/component_menu.rs` (3-level nesting).

## Component design (`oxide_ui::components::menu`)

Small surface, maximally Freya-native:

- **`menu_theme(theme) -> (MenuContainerThemePartial, MenuItemThemePartial)`** — the shared dark
  theming: container = `panel` bg, hairline border, radius, suppressed inner shadow (deep shadow
  via the wrapper, our existing pattern); item = transparent bg, **`hover_background = surface_hi`**,
  `select_background = with_alpha(accent,0x14)`, readable `color`. Fixes bug #1 once, everywhere.
- **`Popover`** — `Popover::new(anchor: Element).open(bool).placement(Placement).on_dismiss(EventHandler<()>).content(Element)`.
  Renders the anchor; when `open`, renders `content` via `Attached` adjacent to the anchor with the
  `Select`-style auto-flip + scale/opacity/slide entrance, and owns dismissal (outside-press +
  Escape → `on_dismiss`). `Placement { Above, Below }` (auto-flip overrides when space is tight).
  Fixes bugs #2 and #4. Animation-ready by construction; lazy-render + opacity-gate until measured
  (performance).
- **`MenuSurface`** — themed scroll-safe container (`Content::fit()`, `min_width`/`max_width`,
  deep-shadow wrapper). Fixes bug #3. Bodies pass their rows; width hugs content within a
  `[min,max]` band (so AttachMenu is narrow, ProviderMenu wider, both content-driven).
- **`MenuRow`** — one themed row: leading icon? · title (+ optional sub) · trailing slot
  (check / Switch / chevron / submenu-arrow). Wraps Freya `MenuButton` + our item theme; the
  `Content::Flex` + flex-title pattern lives here ONCE (kills that recurring per-row gotcha).
- **`MenuSection`** — group header (icon + tinted label).
- **Multi-level:** ProviderMenu's models↔settings becomes a real Freya `SubMenu` (or stays a
  view-swap if SubMenu's hover-open doesn't fit a click-driven settings page — decide in plan by
  reading `menu.rs` SubMenu semantics). Either way the nesting primitive is Freya's.

## Rebuild on it

- `ProviderMenu`/`AttachMenu` become thin: build `MenuSection`+`MenuRow`s inside a `MenuSurface`;
  drop the bespoke container/shadow/width code (now in `MenuSurface`) and the ad-hoc row layout
  (now in `MenuRow`).
- The **orchestrator** wraps each toolbar trigger (+ button, provider pill) in a `Popover` so the
  menu anchors to *that* button, open-state driven by `attach_open`/`provider_open`, and
  `on_dismiss` flips them off. Removes the current toolbar-wide `Attached.top()`.

## Non-goals / carry-forward

- Keyboard arrow-navigation of menu items (Freya's Menu doesn't; defer).
- The menu slide-in being pixel-identical to the design's 140ms `slide` (we adopt Select's 125ms
  Quart — close enough; tune later).
- Touch/bottom-sheet variants (2b-P4 responsive).

## Testing

- Unit/`freya_testing`: `menu_theme` returns dark hover; `MenuRow` renders title+trailing;
  `Popover` renders content only when `open`, fires `on_dismiss`. Headless snapshots:
  `menu_hover_dark`, `popover_anchored` (menu sits adjacent to a trigger), `provider_menu_v2`,
  `attach_menu_v2` (content-hug width). Re-verify the full-shell snapshot.
- Adheres to repo Rule 0 (Freya-native naming/idioms) + Rule 2 (clippy clean, hand-formatted).
