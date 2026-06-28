# Menu Interrupt-Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Inside the composer model-picker, clicking a toggle / reasoning-radio / Composer-settings submenu keeps the menu open; only a model pick, an outside click, or Escape dismisses it.

**Architecture:** Two opt-in axes added to the shared menu primitives. `MenuSurface::light_dismiss(true)` stops handing `on_close` to the inner Freya `Menu` (whose no-hit-test global-press closes on every inside click), instead hosting a bounds-checked outside-press + Escape dismissal and providing a `MenuDismiss` context. `MenuRow::auto_dismiss(bool)` (default true) calls that context on press so regular items close while toggle/submenu rows (set false) stay open. Wired only on `ProviderMenu`; all other menus keep today's behavior.

**Tech Stack:** Rust, Freya (vendored `blog/0.4` at `/run/media/system/fastdrive/repos/freya`), `freya_testing`.

## Global Constraints

- Frontend-only; all changes in `oxide-ui` (`oxide-ui` must NOT depend on `oxide-freya`).
- `cargo clippy` clean (warnings = defects). Hand-formatted: match surrounding style, only format lines you add; NO repo-wide `cargo fmt`.
- No gold-plating; bug/feature changes stand alone. Three similar lines beat a premature abstraction.
- Freya rule: hooks (`use_state`, `use_provide_context`, `use_try_consume`) are called UNCONDITIONALLY at the top of `render`; only their *effects* may be applied conditionally.
- `MenuRow::auto_dismiss` defaults to `true` (regular item). `light_dismiss` defaults to `false` (today's behavior).
- Freya pointer coord accessor is `e.global_location()`; `on_sized` gives `e.area` (an `Area`). Coordinate-space sameness between the two is a verify-during-implementation risk — confirm in the live build; worst case is a mis-aimed outside-click, never a crash.
- Build: distrobox `claude_development`, from `oxide-app/`, `CARGO_TARGET_DIR=<dedicated reflink-warm target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo …`. Worktree off explicit start-point `2b-collapsible-panels`.
- Dismissal *timing* (global outside-press, Escape) is controller live-verified; headless tests cover the pure bounds helper, the context wiring, and an in-bounds click via `t.click_cursor`.

---

### Task 1: `MenuDismiss` context + `MenuSurface::light_dismiss`

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/menu/surface.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/menu/mod.rs` (re-export `MenuDismiss`)

**Interfaces:**
- Produces:
  - `pub struct MenuDismiss(pub Option<EventHandler<()>>)` — `#[derive(Clone, Copy)]`. Context value; `Some` only when the surface is in light-dismiss mode with an `on_close`.
  - `pub(crate) fn point_in_rect(x: f64, y: f64, w: f64, h: f64, px: f64, py: f64) -> bool`.
  - `MenuSurface::light_dismiss(self, bool) -> Self`.
  - `MenuSurface` always calls `use_provide_context(|| MenuDismiss(..))` and (when `light_dismiss`) hosts outside-press + Escape dismissal.

- [ ] **Step 1: Add the `point_in_rect` helper + its unit tests (failing).**

In `surface.rs`, above `impl Component for MenuSurface`, add:

```rust
/// True if `(px,py)` lies within the rect at `(x,y)` of size `(w,h)`.
/// Pure geometry so the light-dismiss bounds-check is unit-testable without
/// Freya event types (and explicit about which coord fields feed it).
pub(crate) fn point_in_rect(x: f64, y: f64, w: f64, h: f64, px: f64, py: f64) -> bool {
    px >= x && px <= x + w && py >= y && py <= y + h
}
```

At the bottom of `surface.rs` add a test module:

```rust
#[cfg(test)]
mod tests {
    use super::point_in_rect;

    #[test]
    fn point_in_rect_inside_edge_outside() {
        // rect at (100,200) size 80x40 -> spans x[100,180], y[200,240]
        assert!(point_in_rect(100., 200., 80., 40., 140., 220.), "center is inside");
        assert!(point_in_rect(100., 200., 80., 40., 100., 200.), "top-left corner is inside (inclusive)");
        assert!(point_in_rect(100., 200., 80., 40., 180., 240.), "bottom-right corner is inside (inclusive)");
        assert!(!point_in_rect(100., 200., 80., 40., 99., 220.), "left of rect is outside");
        assert!(!point_in_rect(100., 200., 80., 40., 140., 241.), "below rect is outside");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails.**

Run (in distrobox, from `oxide-app/`):
```
CARGO_TARGET_DIR=<target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui point_in_rect
```
Expected: FAIL to compile — `cannot find function point_in_rect` is resolved by Step 1, so actually expect it to PASS once Step 1 is in. (If you wrote the test before the helper, expect "cannot find function `point_in_rect`".) Proceed when this test passes.

- [ ] **Step 3: Add `MenuDismiss` + the `light_dismiss` field/builder.**

In `surface.rs`, add near the top (after imports):

```rust
/// Context that lets a descendant menu row ask the surface to dismiss.
/// `Some(handler)` only when the surface is in light-dismiss mode with an `on_close`.
#[derive(Clone, Copy)]
pub struct MenuDismiss(pub Option<EventHandler<()>>);
```

Add the field to the struct and `new`, and a builder:

```rust
// in struct MenuSurface { ... }
    light_dismiss: bool,

// in MenuSurface::new -> Self { ... }
    light_dismiss: false,

// new builder method (impl MenuSurface)
    /// Opt into light-dismiss: the menu closes only on an outside press or Escape
    /// (not on inside clicks), and provides a `MenuDismiss` context so rows can
    /// dismiss declaratively. Default false keeps Freya's any-click-close.
    pub fn light_dismiss(mut self, v: bool) -> Self {
        self.light_dismiss = v;
        self
    }
```

- [ ] **Step 4: Rewrite `MenuSurface::render` for light-dismiss.**

Replace the body of `impl Component for MenuSurface { fn render(&self) -> impl IntoElement { ... } }` with:

```rust
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let (container_theme, _) = menu_theme(th);

        // Hooks first, unconditionally.
        let mut area = use_state(|| None::<Area>);
        let dismiss = self.on_close.filter(|_| self.light_dismiss);
        use_provide_context(move || MenuDismiss(dismiss));

        let body: Element = self.child.clone().unwrap_or_else(|| rect().into_element());

        // Our `Popover` overlay positions the menu; tell the inner Freya `Menu` to
        // skip its own overflow self-offset. In light-dismiss mode do NOT thread
        // `on_close` into the `Menu` (its global press fires on every inside click);
        // dismissal is handled below. Otherwise keep today's Freya-driven dismissal.
        let mut menu = Menu::new().theme(container_theme).host_positioned(true);
        if !self.light_dismiss {
            if let Some(h) = self.on_close {
                menu = menu.on_close(h);
            }
        }
        let menu = menu.child(body);

        let mut surface = rect()
            .corner_radius(CornerRadius::new_all(12.))
            .shadow((0.0_f32, 18.0_f32, 44.0_f32, 0.0_f32, th.shadow_deep()))
            .content(Content::fit())
            .min_width(Size::px(self.min_w))
            .max_width(Size::px(self.max_w));

        if self.light_dismiss {
            let on_close = self.on_close;
            surface = surface
                .on_sized(move |e: Event<SizedEventData>| {
                    area.set_if_modified(Some(e.area));
                })
                .on_global_pointer_press(move |e: Event<PointerEventData>| {
                    if let (Some(a), Some(h)) = (*area.peek(), on_close) {
                        let p = e.global_location();
                        let inside = point_in_rect(
                            a.origin.x as f64, a.origin.y as f64,
                            a.size.width as f64, a.size.height as f64,
                            p.x as f64, p.y as f64,
                        );
                        if !inside {
                            h.call(());
                        }
                    }
                })
                .on_global_key_down(move |e: Event<KeyboardEventData>| {
                    if e.key == Key::Named(NamedKey::Escape) {
                        if let Some(h) = on_close {
                            h.call(());
                        }
                    }
                });
        }

        surface.child(menu)
    }
```

Notes for the implementer:
- `Area`, `Event`, `SizedEventData`, `PointerEventData`, `KeyboardEventData`, `Key`, `NamedKey` all come from `freya::prelude::*` (already imported). If `Area` is not in prelude, import it from where `on_sized`'s `e.area` type lives.
- Confirm `e.area` field names (`origin.x/y`, `size.width/height`) and `global_location()` returning `.x/.y`. If the f32/f64 casts or coord space differ, adjust — this is the flagged risk. The menu floats in window space, as does `global_location()`.
- `EventHandler<()>` is `Copy`; `self.on_close` is `Option<EventHandler<()>>` (Copy), so capturing it into the closures by value is fine. `area` (a `State`) is `Copy`.

- [ ] **Step 5: Re-export `MenuDismiss` from the module.**

In `menu/mod.rs`, change:
```rust
pub use surface::MenuSurface;
```
to:
```rust
pub use surface::{MenuDismiss, MenuSurface};
```

- [ ] **Step 6: Build + clippy.**

Run:
```
CARGO_TARGET_DIR=<target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build -p oxide-ui
CARGO_TARGET_DIR=<target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo clippy -p oxide-ui
```
Expected: builds; zero warnings on our code. Existing menus still pass `on_close` to Freya `Menu` (light_dismiss defaults false), so no behavior change for them.

- [ ] **Step 7: Run the helper test.**

Run:
```
CARGO_TARGET_DIR=<target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui point_in_rect
```
Expected: PASS.

- [ ] **Step 8: Commit.**

```bash
git add oxide-app/crates/oxide-ui/src/components/menu/surface.rs oxide-app/crates/oxide-ui/src/components/menu/mod.rs
git commit -m "feat(menu): MenuSurface::light_dismiss + MenuDismiss context"
```

---

### Task 2: `MenuRow::auto_dismiss`

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/menu/row.rs`

**Interfaces:**
- Consumes: `MenuDismiss` (from Task 1, via `use_try_consume`).
- Produces: `MenuRow::auto_dismiss(self, bool) -> Self` (default `true`). On press, after the row's own `on_press`, calls `MenuDismiss` when `auto_dismiss` is true and the context is present.

- [ ] **Step 1: Add the `auto_dismiss` field + builder.**

In `row.rs`, add the import:
```rust
use super::surface::MenuDismiss;
```
Add to `struct MenuRow { ... }`:
```rust
    auto_dismiss: bool,
```
Add to `MenuRow::new`:
```rust
    auto_dismiss: true,
```
Add the builder (in `impl MenuRow`):
```rust
    /// Whether activating this row also dismisses the menu. Default true (a
    /// regular item). Set false for toggle / submenu / nav rows that should keep
    /// the menu open. Only has effect under a `MenuSurface::light_dismiss(true)`
    /// (otherwise there is no `MenuDismiss` context and Freya's any-click-close applies).
    pub fn auto_dismiss(mut self, v: bool) -> Self {
        self.auto_dismiss = v;
        self
    }
```

- [ ] **Step 2: Wire the dismiss into the press handler.**

In `impl Component for MenuRow { fn render(&self) }`, add the hook at the top (with the other top-of-render lets):
```rust
        let dismiss = use_try_consume::<MenuDismiss>();
        let auto_dismiss = self.auto_dismiss;
```
Replace the `MenuButton ... .on_press(...)` closure at the end of render with:
```rust
        MenuButton::new()
            .theme(item_theme)
            .on_press(move |_: Event<PressEventData>| {
                if let Some(h) = &on_press {
                    h.call(());
                }
                if auto_dismiss {
                    if let Some(d) = dismiss {
                        if let Some(h) = d.0 {
                            h.call(());
                        }
                    }
                }
            })
            .child(inner)
```

- [ ] **Step 3: Write the behavior test (failing first).**

Add to the existing `#[cfg(test)] mod tests` in `row.rs` (it already imports `MenuSurface` + `freya_testing::prelude::*`):

```rust
    use freya_testing::TestingRunner;

    // Mount a light-dismiss surface whose on_close bumps a counter shown in a label.
    // Clicking an auto_dismiss(false) row must NOT bump it; an auto_dismiss(true) row must.
    fn dismiss_count_after_row_click(auto_dismiss: bool) -> String {
        let app = move || -> Element {
            let mut closes = use_state(|| 0_u32);
            MenuSurface::new(Theme::default())
                .light_dismiss(true)
                .on_close(move |_| *closes.write() += 1)
                .child(
                    rect()
                        .direction(Direction::Vertical)
                        .child(label().text(format!("closes={}", closes.read())).font_size(12.))
                        .child(
                            MenuRow::new(Theme::default())
                                .title("Row")
                                .auto_dismiss(auto_dismiss)
                                .on_press(move |_| {})
                                .into_element(),
                        )
                        .into_element(),
                )
                .into_element()
        };
        let (mut t, _) = TestingRunner::new(app, (320., 240.).into(), |_| {}, 1.);
        t.poll(std::time::Duration::from_millis(5), std::time::Duration::from_millis(60));
        t.sync_and_update();
        let center = t
            .find(|node, el| {
                Label::try_downcast(el)
                    .filter(|l| l.text.as_ref().contains("Row"))
                    .map(|_| {
                        let c = node.layout().visible_area().center();
                        (c.x as f64, c.y as f64)
                    })
            })
            .expect("Row label present");
        t.click_cursor(center);
        t.poll(std::time::Duration::from_millis(5), std::time::Duration::from_millis(60));
        t.sync_and_update();
        let txt = t
            .find(|_, el| {
                Label::try_downcast(el).filter(|l| l.text.as_ref().contains("closes="))
            })
            .expect("counter label present");
        txt.text.as_ref().to_string()
    }

    #[test]
    fn auto_dismiss_false_row_does_not_close_menu() {
        assert_eq!(dismiss_count_after_row_click(false), "closes=0");
    }

    #[test]
    fn auto_dismiss_true_row_closes_menu() {
        assert_eq!(dismiss_count_after_row_click(true), "closes=1");
    }
```

Notes:
- If `TestingRunner::new` requires a plain `fn` root rather than a capturing closure, hoist the `auto_dismiss` value through two tiny wrapper `fn`s (`fn app_keep() { body(false) }` / `fn app_close() { body(true) }`) calling a shared `fn body(auto_dismiss: bool) -> Element`.
- The `find` closure returns the label's text via the existing `Label::try_downcast` API used elsewhere in this file's tests.

- [ ] **Step 4: Run the tests to verify they fail then pass.**

Run:
```
CARGO_TARGET_DIR=<target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui auto_dismiss
```
Expected before Step 1-2: compile error (`auto_dismiss` not found). After Step 1-2: both PASS — `closes=0` for the kept-open row, `closes=1` for the closing row. If `auto_dismiss_true` returns `closes=2`, the click is also triggering the bounds-checked outside-press (row center read as outside the measured area) — confirm `on_sized` fired (area is `Some`) and the coordinate space matches; this is the flagged coord-space risk surfacing in the test.

- [ ] **Step 5: clippy + commit.**

```
CARGO_TARGET_DIR=<target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo clippy -p oxide-ui
```
Expected: zero warnings on our code.
```bash
git add oxide-app/crates/oxide-ui/src/components/menu/row.rs
git commit -m "feat(menu): MenuRow::auto_dismiss (default true) via MenuDismiss context"
```

---

### Task 3: Wire `ProviderMenu` + composer cleanup

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/composer/provider_menu.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/composer/mod.rs`

**Interfaces:**
- Consumes: `MenuSurface::light_dismiss` (Task 1), `MenuRow::auto_dismiss` (Task 2), `MenuDismiss` context (Task 1).

- [ ] **Step 1: Turn on light-dismiss for the ProviderMenu surface.**

In `provider_menu.rs`, in `impl Component for ProviderMenu { fn render }`, change the surface construction:
```rust
        let mut surface = MenuSurface::new(th)
            .min_w(260.)
            .max_w(320.)
            .light_dismiss(true)
            .child(body);
```
(the `.on_close(h)` threading below it stays unchanged.)

- [ ] **Step 2: Make the model row dismiss on pick.**

In `provider_menu.rs`, in `fn model_row`, add the dismiss hook + call. Add the import at the top of the file:
```rust
use crate::components::menu::MenuDismiss;
```
At the top of `model_row` (it is a `&self` method, not a component `render`; `use_try_consume` is a hook and must run during render — `model_row` is called from within `models_view`, itself called from `render`, so the hook executes in render context. Read the context there):
```rust
        let dismiss = use_try_consume::<MenuDismiss>();
```
Update the `MenuButton::on_press` closure in `model_row`:
```rust
        MenuButton::new()
            .theme(item_theme)
            .on_press(move |_: Event<PressEventData>| {
                if let Some(h) = &on_select {
                    h.call(model_id);
                }
                if let Some(d) = dismiss {
                    if let Some(h) = d.0 {
                        h.call(());
                    }
                }
            })
            .child(inner)
            .into_element()
```

(If `use_try_consume` inside `model_row` causes a hook-order issue because `model_row` is called in a loop over models, instead read `dismiss` ONCE in `models_view` and pass it as a parameter to `model_row(&self, model, th, dismiss)`. Hooks must not run in loops — prefer the parameter approach: add `dismiss: Option<MenuDismiss>` as a `model_row` argument and call `use_try_consume` a single time in `models_view`.)

- [ ] **Step 3: Mark the submenu/nav rows to stay open.**

In `provider_menu.rs`:
- In `models_view`, the Composer-settings nav row — add `.auto_dismiss(false)`:
```rust
        let settings_row = MenuRow::new(th)
            .icon(Some("gear"))
            .title("Composer settings")
            .trailing(Some(icon("chevronDown", 14., th.faint())))
            .auto_dismiss(false)
            .on_press(move |_| {
                view.set(View::Settings);
            })
            .into_element();
```
- In `settings_view`, the Back row — add `.auto_dismiss(false)`:
```rust
        let back_row = MenuRow::new(th)
            .icon(Some("chevronDown"))
            .title("Composer settings")
            .auto_dismiss(false)
            .on_press(move |_| {
                view.set(View::Models);
            })
            .into_element();
```
The optimizer / send-on-enter `MenuRow`s have no `on_press` (they toggle via their trailing `Switch`), so they never dismiss — leave them unchanged. The reasoning `SegmentedButton` is not a `MenuRow` — leave unchanged.

- [ ] **Step 4: Remove the redundant explicit close in composer/mod.rs.**

In `composer/mod.rs`, the `on_select_model` handler currently closes the popover explicitly; closing is now declarative via the model row's auto-dismiss → `MenuDismiss` → `on_close`. Change:
```rust
            .on_select_model(move |id: &'static str| {
                model_id.set(id.to_string());
                provider_open.set(false);
            })
```
to:
```rust
            .on_select_model(move |id: &'static str| {
                model_id.set(id.to_string());
            })
```
Leave `.on_close(move |_| provider_open.set(false))` unchanged — it is the `MenuDismiss` target and the outside-press/Escape sink.

- [ ] **Step 5: Build, clippy, run all oxide-ui tests.**

Run:
```
CARGO_TARGET_DIR=<target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build -p oxide-freya
CARGO_TARGET_DIR=<target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo clippy -p oxide-ui
CARGO_TARGET_DIR=<target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui
```
Expected: `oxide-freya` binary builds; zero clippy warnings on our code; all tests pass (existing + Task 1/2 additions).

- [ ] **Step 6: Commit.**

```bash
git add oxide-app/crates/oxide-ui/src/components/composer/provider_menu.rs oxide-app/crates/oxide-ui/src/components/composer/mod.rs
git commit -m "feat(composer): light-dismiss model picker; model-pick closes, toggles/submenu stay open"
```

- [ ] **Step 7: Controller live-verify (not a subagent step).**

Build + install + relaunch the overlay. In the model picker confirm: model pick closes; reasoning Low/Med/High stays open; Prompt-optimizer + Send-on-Enter toggles stay open; Composer-settings expander swaps to settings IN PLACE (no dismiss) and Back returns; outside-click closes; Escape closes. Confirm attach / project dropdown / icon picker menus are unchanged (still close on pick).

---

## Self-Review

**Spec coverage:** `light_dismiss` + bounds-checked outside-press + Escape + `MenuDismiss` (Task 1); `auto_dismiss` default-true + context consume (Task 2); ProviderMenu wiring + `composer/mod.rs` cleanup + live verify (Task 3). All spec sections mapped.

**Placeholder scan:** none — every code step shows complete code; the two flagged risks (coord-space, hook-in-loop) carry concrete fallbacks, not TODOs.

**Type consistency:** `MenuDismiss(pub Option<EventHandler<()>>)` defined Task 1, consumed identically in Tasks 2 & 3 (`d.0` → `Option<EventHandler<()>>`); `point_in_rect` signature stable; `light_dismiss`/`auto_dismiss` builder names consistent across tasks.
