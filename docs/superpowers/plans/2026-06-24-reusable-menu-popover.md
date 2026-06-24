# Reusable Menu / Popover — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** A reusable, Freya-native menu/popover layer in `oxide-ui` (`components/menu/`), and rebuild the Composer's `ProviderMenu`/`AttachMenu` on it — fixing the 4 live-test bugs (light-on-light hover, menu floats far from trigger, attach menu fills width, no dismiss on outside-click).

**Architecture:** Lean on Freya's built-ins. `Menu`/`MenuButton`/`SubMenu` already give outside-press + Escape dismissal (`on_close`), off-screen `overflow_offset`, multi-level nesting, and content-hug `MenuItem` width; `Select` shows the animated entrance + auto-flip; `Attached` does per-trigger anchoring. We add: a shared dark **theme** (fixes hover), a **`MenuSurface`** (content-hug width), **`MenuRow`/`MenuSection`** (row layout + the `Content::Flex` gotcha in one place), and a **`Popover`** anchor wrapper (Attached + animation + open-state + surfaced dismissal). Then the menus + orchestrator become thin.

**Tech Stack:** Rust, Freya 0.4.0-rc.23 (`freya-components` `Menu`/`MenuButton`/`SubMenu`/`Attached`, `freya::animation`), `freya-testing`.

## Global Constraints

- **Reuse Freya, don't reinvent** (repo Rule 0 + Rule 4): build on `Menu`/`MenuButton`/`SubMenu`/`Attached`/`Select` patterns; adhere to their API. Confirm any uncertain signature against `/run/media/system/fastdrive/repos/freya/crates/freya-components/src/` (or Serena, `freya` project).
- **Confirmed Freya APIs:**
  - `Menu::new().on_close(impl Fn(()) ...).theme(MenuContainerThemePartial).child(..)` — `on_close` fires on outside global-press + Escape.
  - `MenuButton::new().on_press(EventHandler<Event<PressEventData>>).theme(MenuItemThemePartial).child(..)`.
  - `SubMenu::new().label(impl IntoElement).theme(MenuContainerThemePartial).child(..)` — hover-opens, nests.
  - `MenuContainerThemePartial::new().background(Color).border_fill(Color).shadow(Color).corner_radius(CornerRadius)`.
  - `MenuItemThemePartial::new().background(Color).hover_background(Color).select_background(Color).border_fill(Color).select_border_fill(Color).corner_radius(CornerRadius).color(Color)`.
  - `Attached::new(inner: impl IntoElement).top()/.bottom()/.left()/.right()/.position(AttachedPosition).child(..)` — anchors child to inner; hides at opacity 0 until measured.
  - Animation: `use_animation(|conf| { conf.on_change(OnChange::Rerun); conf.on_creation(OnCreation::Finish); let a = AnimNum::new(start,end).time(ms).ease(Ease::Out).function(Function::Quart); if open() { a } else { a.into_reversed() } })`; read `anim.read().value()`, `anim.is_running().read()`. (`use freya::animation::*` — not in prelude, per reference_freya_dev.)
  - `Platform::get().root_size.peek()` for auto-flip; `on_sized(|e| ... e.area ...)` for measurement (`SizedEventData`, `Area` with `.width()/.height()/.min_y()/.max_y()`).
- **Tokens via `Theme` only** (no hardcoded hex): `panel/surface/surface_hi/hairline/text/subtext/subtext_hi/faint/accent/with_alpha/shadow_deep/tone`.
- **Content::Flex rule:** any rect with a `Size::flex` child sets `.content(Content::Flex)` — but after this slice it lives ONCE in `MenuRow`, not per menu.
- Build/test from `oxidemx-2b/oxide-app/` with `LIBRARY_PATH=/tmp/oxidemx-lib-links`; `oxide-freya` is a `--bin`. Hand-formatted (no `cargo fmt`); `cargo clippy -p oxide-ui` clean. Commit per task; add only your files. **After the mount task, rebuild the binary** (`cargo build -p oxide-freya --bin oxide-freya`) so the live app isn't stale.

---

### Task 1: Menu primitives — theme + surface + section + row

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/menu/mod.rs` (declares submodules + re-exports)
- Create: `oxide-app/crates/oxide-ui/src/components/menu/theme.rs`, `surface.rs`, `row.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/mod.rs` (add `pub mod menu;` + re-exports)

**Interfaces:**
- Produces: `menu_theme(theme: Theme) -> (MenuContainerThemePartial, MenuItemThemePartial)` — container = `panel` bg / `hairline` border / `Color::TRANSPARENT` shadow (deep shadow lives on the surface wrapper) / radius 12; item = `Color::TRANSPARENT` bg / **`hover_background = surface_hi`** / `select_background = with_alpha(accent,0x14)` / `border_fill = TRANSPARENT` / `select_border_fill = with_alpha(accent,0x33)` / radius 9 / `color = text`. `MenuSurface { theme, min_w: f32, max_w: f32 }` (+ `.min_w`/`.max_w` builders; defaults 180/360) impl Component — a deep-shadow wrapper (`shadow (0,18,44, shadow_deep)`, radius 12, `Content::fit()`) holding a `Menu` themed via `menu_theme().0`; `.child(Element)` for the body; width hugs content within `[min_w,max_w]`. `MenuSection { title, tone, theme }` impl Component — group header (tinted icon optional + bold tinted label). `MenuRow { theme }` + `.icon(Option<&'static str>)` `.title(String)` `.sub(Option<String>)` `.trailing(Option<Element>)` `.selected(bool)` `.on_press(EventHandler<()>)` impl Component — a `MenuButton` themed via `menu_theme().1` whose child is the `Content::Flex` row (leading icon? · title(+sub) flex · trailing?).

- [ ] **Step 1: failing tests** (`row.rs`): `menu_theme_has_dark_hover` — `menu_theme(Theme::default()).1` built with `hover_background = surface_hi` (assert by constructing + reading back is hard for partials; instead assert the helper returns without panic AND a `MenuRow` mounts). `menu_row_renders_title_and_trailing` — mount `MenuRow::new(Theme::default()).title("Hello").trailing(Some(label().text("✓").into_element()))`; assert both "Hello" and "✓" render (freya_testing `launch_test`/`find`/`Label::try_downcast`).
- [ ] **Step 2: run → FAIL.**
- [ ] **Step 3: implement** theme.rs (the helper), surface.rs (`MenuSurface`), row.rs (`MenuSection` + `MenuRow`). Follow the `prompt_input.rs`/`chip.rs` builder idiom. `MenuRow`'s row uses `.content(Content::Flex)` with the title column `Size::flex(1.0)`.
- [ ] **Step 4: run → PASS;** `cargo clippy -p oxide-ui` clean.
- [ ] **Step 5: headless snapshot** `snapshot_menu_hover` in oxide-freya: a `MenuSurface` with 3 `MenuRow`s, one `.selected(true)` → `/tmp/oxide-menu-primitives.png`; render without panic (controller inspects dark hover/select tint).
- [ ] **Step 6: commit** `feat(oxide-ui): menu primitives — dark theme + surface + row/section`

---

### Task 2: `Popover` anchor wrapper

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/menu/popover.rs`
- Modify: `menu/mod.rs` (`pub mod popover;` + re-export `Popover`, `Placement`)
- Reference: `freya/crates/freya-components/src/{attached.rs,select.rs,menu.rs}` + `examples/component_menu.rs`

**Interfaces:**
- Produces: `enum Placement { Above, Below }`; `Popover { anchor: Element, open: bool, placement: Placement }` + `Popover::new(anchor: Element)` + `.open(bool)` + `.placement(Placement)` + `.on_dismiss(EventHandler<()>)` + `.content(Element)` impl Component. Renders `anchor`; when `open`, renders `content` via `Attached` adjacent (`.top()` for Above / `.bottom()` for Below) with a `use_animation` entrance (scale 0.9→1, opacity 0→1, slide ∓8→0; ~125ms `Ease::Out` `Function::Quart`; reversed when closing) and surfaces dismissal: outside-press + Escape → `on_dismiss(())`. Auto-flip: if `placement == Below` but the measured content height exceeds space below the anchor (`Platform::get().root_size` − anchor.max_y) and fits above, render Above (and vice-versa). Lazy: only render the overlay when `open || anim still running`; opacity-gate until measured.

**Dismissal detail (the subtle part):** the `content` is wrapped so its inner Freya `Menu`'s `on_close` (outside global-press + Escape) maps to `on_dismiss`. The press that toggles the trigger open must NOT immediately fire dismiss — guard like Freya's `Select`/`ContextMenu`: the trigger's own `on_press` calls `stop_propagation()` and the orchestrator toggles open; `on_dismiss` only fires for presses OUTSIDE both anchor and content. If a clean guard isn't reachable from the public API, implement the closest correct behavior (dismiss on outside-press via a full-window backdrop `rect` like `PopupBackground`, or via `on_global_pointer_press` that checks the press is outside the measured content area) and note it.

- [ ] **Step 1: failing tests** (`popover.rs`): `popover_hidden_when_closed` — mount `Popover::new(label().text("anchor").into_element()).open(false).content(label().text("MENU").into_element())`; assert "anchor" renders but "MENU" does NOT. `popover_shows_content_when_open` — same with `.open(true)`; assert "MENU" renders.
- [ ] **Step 2: run → FAIL.**
- [ ] **Step 3: implement** on `Attached` + `use_animation`. Study `select.rs` for the 3-part anim + flip math and `menu.rs` for `on_close` wiring; reuse, don't reinvent.
- [ ] **Step 4: run → PASS;** clippy clean.
- [ ] **Step 5: snapshot** `snapshot_popover_anchored` in oxide-freya: a small trigger rect with a `Popover.open(true)` showing a `MenuSurface` body, rendered mid-canvas → `/tmp/oxide-popover.png`; confirm the menu sits ADJACENT to the trigger (not detached).
- [ ] **Step 6: commit** `feat(oxide-ui): Popover anchor wrapper (Attached + animation + dismissal)`

---

### Task 3: Rebuild `ProviderMenu` on the primitives

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/composer/provider_menu.rs`

**Interfaces:** unchanged public API (`ProviderMenu::new(theme).selected_id(..).thinking(..).optimizer(..).send_on_enter(..).on_select_model(..).on_thinking(..).on_toggle_optimizer(..).on_toggle_send_on_enter(..)`).

**Changes:** replace the bespoke container/shadow/`width(280px)` + ad-hoc `MenuButton` rows with `MenuSurface` + `MenuSection` (group headers) + `MenuRow` (model rows: `.icon`/`.title(name)`/`.sub`/`.trailing(check when active)`/`.selected`/`.on_press`). Keep the `SegmentedButton` thinking control + `Switch` rows (wrap in `MenuRow`'s trailing slot where natural). The models↔settings sub-view: try Freya `SubMenu` for true multi-level; if `SubMenu`'s hover-to-open doesn't suit a click-driven settings page (read `menu.rs` SubMenu semantics first), keep the internal `View` swap but render both views through the primitives. The dark hover now comes from `menu_theme` (bug #1 fixed for free). Width hugs content via `MenuSurface` (bug #3).

- [ ] **Step 1:** keep the existing tests (`provider_menu_renders_all_models_and_check`, `provider_menu_shows_group_headers`) — they must still pass after the rebuild (label assertions are layout-agnostic). Add `provider_menu_row_uses_menurow` only if cheap; otherwise rely on the existing two + the snapshot.
- [ ] **Step 2: run existing tests → confirm they still gate** (they pass pre-change; after refactor they must still pass).
- [ ] **Step 3: implement** the rebuild on primitives.
- [ ] **Step 4: run → PASS;** clippy clean.
- [ ] **Step 5: snapshot** update `snapshot_provider_menu` → `/tmp/oxide-provider-menu.png`; confirm dark readable rows, content-hug width, groups/check intact.
- [ ] **Step 6: commit** `refactor(oxide-ui): ProviderMenu on shared menu primitives`

---

### Task 4: Rebuild `AttachMenu` on the primitives

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/composer/attach_menu.rs`

**Interfaces:** unchanged (`AttachMenu::new(theme).on_pick(EventHandler<&'static str>)`).

**Changes:** replace the bespoke `Menu` + width-less rows with `MenuSurface` (narrow `[min,max]`, hugs content → bug #3) + `MenuRow` per `ATTACH_SOURCES` entry (`.icon(source.icon)`/`.title(source.label)`/`.sub(source.hint)`/`.on_press(|| on_pick(source.id))`). Dark hover from `menu_theme` (bug #1).

- [ ] **Step 1:** keep existing tests (`attach_menu_shows_upload_file_label`, `attach_menu_renders_all_six_sources`) — must still pass.
- [ ] **Step 2: run → confirm gate.**
- [ ] **Step 3: implement** rebuild on primitives.
- [ ] **Step 4: run → PASS;** clippy clean.
- [ ] **Step 5: snapshot** `snapshot_attach_menu` (new, in oxide-freya) at 760px canvas → `/tmp/oxide-attach-menu.png`; confirm the menu is NARROW (content-width, not full-canvas).
- [ ] **Step 6: commit** `refactor(oxide-ui): AttachMenu on shared menu primitives`

---

### Task 5: Orchestrator anchoring via `Popover` + live verify

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/composer/mod.rs` (the orchestrator)

**Interfaces:** unchanged public API.

**Changes:** remove the toolbar-wide `Attached.top()` mounting. Instead anchor each menu to its specific trigger: the Toolbar exposes its `+` button and provider pill as the `Popover` anchors — simplest path is to wrap the relevant toolbar trigger region in a `Popover` driven by `attach_open`/`provider_open`, with `.content(AttachMenu/ProviderMenu...)`, `.placement(Placement::Above)`, and `.on_dismiss(|_| { attach_open.set(false) })` / `provider_open`. (If the trigger lives inside `Toolbar` and isn't individually reachable, add a minimal seam: have `Toolbar` accept optional `attach_popover`/`provider_popover` content, OR mount two `Popover`s in the orchestrator anchored to small invisible anchors co-located with the buttons — pick the cleaner one after reading toolbar.rs; document the choice.) Net behavior: menus open adjacent to their button (bug #2) and close on outside-click/Escape via `on_dismiss` (bug #4).

- [ ] **Step 1:** keep the orchestrator test (`composer_renders_send_button_and_activity_line`) — must still pass.
- [ ] **Step 2: run → confirm gate.**
- [ ] **Step 3: implement** the Popover anchoring + dismissal wiring.
- [ ] **Step 4: run `cargo test -p oxide-ui` + `-p oxide-freya --bin oxide-freya`** → all green; clippy clean.
- [ ] **Step 5: snapshots** re-render `snapshot_composer_full` (provider menu now anchored to the pill + dark rows) and `snapshot_shell` → Read both; confirm menu adjacency + dark hover + content width. **Rebuild the live binary:** `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build -p oxide-freya --bin oxide-freya`.
- [ ] **Step 6: commit** `feat(oxide-freya): anchor composer menus via Popover + outside-click dismiss`

---

## Self-Review

- **Bug coverage:** #1 hover → Task 1 (`menu_theme` dark hover). #2 anchoring → Task 2 (`Popover`/Attached) + Task 5 (per-trigger). #3 width → Task 1 (`MenuSurface` content-hug) applied in Tasks 3/4. #4 dismissal → Task 2 (`on_dismiss`) + Task 5 (wired to open-state).
- **Reuse:** every task builds on Freya `Menu`/`MenuButton`/`SubMenu`/`Attached`/`Select` patterns; no bespoke positioning/dismissal engine.
- **Type consistency:** `menu_theme`/`MenuSurface`/`MenuSection`/`MenuRow`/`Popover`/`Placement` names used consistently across tasks; ProviderMenu/AttachMenu/orchestrator public APIs unchanged (downstream mount untouched).
- **No placeholders:** the one genuinely open decision (SubMenu vs view-swap in Task 3; trigger-anchor seam in Task 5) is explicitly flagged to be resolved by reading the named Freya/our source first, with a documented fallback — not a vague TODO.
