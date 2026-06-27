# OxideTooltip + TooltipGroup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A reusable `OxideTooltip` (text or detailed content, placement + offset + delay, animated) plus a `TooltipGroup` that makes tooltips instant after the first — wired into the collapsed sidebar + right-panel rails.

**Architecture:** `OxideTooltip` is modeled on Freya 0.4's `TooltipContainer` (hover-delay `Timer` + `Attached` positioning + `use_animation` scale/opacity), extended with a detailed-content variant, a configurable offset, and `TooltipGroup` warm-state (effective delay 0 while warm; ~400ms cooldown on leaving the group).

**Tech Stack:** Rust, Freya blog/0.4 (`Attached`/`AttachedPosition`, `use_animation`/`AnimNum`, `spawn`, `TaskHandle`, `async_io::Timer`), `freya_testing`.

## Global Constraints

- Freya blog/0.4 builder API. Reuse `Attached::new(inner).position(AttachedPosition::{Top,Bottom,Left,Right}).maybe_child(...)` and `AttachedPosition` (verify they're in `freya::prelude`; else `freya::components::{Attached, AttachedPosition}`). Source of truth: `repos/freya/crates/freya-components/src/{tooltip,attached}.rs`.
- Hover-delay pattern (port verbatim from `tooltip.rs`): `use_state::<Option<TaskHandle>>(|| None)`; `on_pointer_over` cancels any pending task then `spawn(async move { Timer::after(delay).await; is_hovering.set_if_modified(true); })`; `on_pointer_out` cancels the task + `is_hovering.set_if_modified(false)`. `Timer` is `async_io::Timer` — add `async-io` to `oxide-ui/Cargo.toml` deps if absent (it's already in the workspace lock via Freya).
- Animation: `use_animation` with `conf.on_change(OnChange::Rerun); conf.on_creation(OnCreation::Finish);` `scale = AnimNum::new(0.9,1.).time(150).ease(Ease::Out).function(Function::Expo)`, `opacity = AnimNum::new(0.,1.)` same; `if is_hovering() {(scale,opacity)} else {(scale.into_reversed(),opacity.into_reversed())}`; `let (scale,opacity) = animation.read().value();`. (`use freya::animation::*;`.)
- `delay` default **1000ms**; `TooltipGroup` cooldown **400ms**; warm ⇒ effective delay **0**.
- Placement padding (from Freya): Top `(0,0,5,0)`, Bottom `(5,0,0,0)`, Left `(0,5,0,0)`, Right `(0,0,0,5)` — ADD `offset` to the side-specific component (e.g. Right ⇒ left-pad `5+offset`).
- Tooltip floating body uses `.interactive(Interactive::No)` (never captures pointer) and `is_visible = opacity > 0.`.
- `oxide-ui` MUST NOT depend on `oxide-freya`. The component takes only owned content (`Cow<'static,str>` / `Element`) + plain config.
- Match hand-formatting; no repo-wide `cargo fmt`; `cargo clippy -p oxide-ui -p oxide-freya` clean; no gold-plating.
- Every snapshot: `use_init_theme(dark_theme)` + `rect().background(Theme::default().bg_deep())` root; for tooltip snapshots force `is_hovering=true` (or render the body directly) and `poll` past 150ms; controller Reads each PNG.
- Build: NEW branch off `2b-collapsible-panels` in `oxidemx-2b`; distrobox `claude_development`, `LIBRARY_PATH=/tmp/oxidemx-lib-links`, default `oxide-app/target` (Freya canonical `repos/freya`@blog04).

**Run every cargo command as:**
```
distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-2b/oxide-app && LIBRARY_PATH=/tmp/oxidemx-lib-links cargo <args>'
```

---

### Task 1: `OxideTooltip` component (standalone, full delay)

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/tooltip.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/mod.rs` (register + re-export)
- Modify: `oxide-app/crates/oxide-ui/Cargo.toml` (add `async-io` if absent)

**Interfaces:**
- Produces: `pub struct OxideTooltip` with `OxideTooltip::text(impl Into<Cow<'static,str>>)`, `OxideTooltip::detailed(impl IntoElement)`, builders `.placement(AttachedPosition)` (default `Bottom`), `.offset(f32)` (default 0), `.delay(Duration)` (default 1000ms), `impl ChildrenExt`/`KeyExt`. Re-exports `pub use freya::prelude::AttachedPosition` (or wherever it lives) so callers needn't import from freya directly. Helper `pub(crate) fn tooltip_surface(th: Theme, child: Element) -> impl IntoElement`.

- [ ] **Step 1: Scaffold the struct + builders.** Create `tooltip.rs`:
```rust
//! `OxideTooltip` — a hover tooltip modeled on Freya's `TooltipContainer`
//! (`freya-components/src/tooltip.rs`): hover-delay Timer + `Attached` placement +
//! a scale/opacity entrance animation. Adds a detailed-content variant, a
//! configurable offset, and (via `TooltipGroup`) "instant after the first".
use std::borrow::Cow;
use std::time::Duration;

use freya::animation::*;
use freya::prelude::*;

use crate::tokens::Theme;

pub use freya::prelude::AttachedPosition;

/// Tooltip body: a simple themed text label, or arbitrary rich content.
enum TooltipBody {
    Text(Cow<'static, str>),
    Detailed(Element),
}

impl Clone for TooltipBody {
    fn clone(&self) -> Self {
        match self {
            Self::Text(t) => Self::Text(t.clone()),
            Self::Detailed(e) => Self::Detailed(e.clone()),
        }
    }
}

#[derive(Clone)]
pub struct OxideTooltip {
    body: TooltipBody,
    position: AttachedPosition,
    offset: f32,
    delay: Duration,
    children: Vec<Element>,
    key: DiffKey,
}

impl OxideTooltip {
    pub fn text(text: impl Into<Cow<'static, str>>) -> Self {
        Self::with_body(TooltipBody::Text(text.into()))
    }
    pub fn detailed(content: impl IntoElement) -> Self {
        Self::with_body(TooltipBody::Detailed(content.into_element()))
    }
    fn with_body(body: TooltipBody) -> Self {
        Self {
            body,
            position: AttachedPosition::Bottom,
            offset: 0.0,
            delay: Duration::from_millis(1000),
            children: vec![],
            key: DiffKey::None,
        }
    }
    pub fn placement(mut self, position: AttachedPosition) -> Self {
        self.position = position;
        self
    }
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

impl PartialEq for OxideTooltip {
    fn eq(&self, _: &Self) -> bool { false } // re-render on each render of the parent (content is owned)
}
impl KeyExt for OxideTooltip {
    fn write_key(&mut self) -> &mut DiffKey { &mut self.key }
}
impl ChildrenExt for OxideTooltip {
    fn get_children(&mut self) -> &mut Vec<Element> { &mut self.children }
}

/// Shadowed surface for the detailed-content variant.
pub(crate) fn tooltip_surface(th: Theme, child: Element) -> impl IntoElement {
    rect()
        .background(th.panel())
        .border(Border::new().fill(th.hairline_strong()).width(1.))
        .corner_radius(CornerRadius::new_all(8.))
        .padding(Gaps::new_all(8.))
        .child(child)
}
```
(If `AttachedPosition`/`Attached` are not in `freya::prelude`, import from `freya::components` — check the re-export; the `pub use` must resolve.)

- [ ] **Step 2: Implement `render`** (port of `TooltipContainer::render`, no group yet):
```rust
impl Component for OxideTooltip {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let mut is_hovering = use_state(|| false);
        let mut delay_task = use_state::<Option<TaskHandle>>(|| None);

        let animation = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            conf.on_creation(OnCreation::Finish);
            let scale = AnimNum::new(0.9, 1.).time(150).ease(Ease::Out).function(Function::Expo);
            let opacity = AnimNum::new(0., 1.).time(150).ease(Ease::Out).function(Function::Expo);
            if is_hovering() { (scale, opacity) } else { (scale.into_reversed(), opacity.into_reversed()) }
        });
        let (scale, opacity) = animation.read().value();

        let delay = self.delay;
        let on_pointer_over = move |_| {
            if let Some(handle) = delay_task.write().take() { handle.cancel(); }
            let task = spawn(async move {
                async_io::Timer::after(delay).await;
                is_hovering.set_if_modified(true);
            });
            delay_task.set(Some(task));
        };
        let on_pointer_out = move |_| {
            if let Some(handle) = delay_task.write().take() { handle.cancel(); }
            is_hovering.set_if_modified(false);
        };

        let is_visible = opacity > 0.;
        let pad = match self.position {
            AttachedPosition::Top => Gaps::new(0., 0., 5. + self.offset, 0.),
            AttachedPosition::Bottom => Gaps::new(5. + self.offset, 0., 0., 0.),
            AttachedPosition::Left => Gaps::new(0., 5. + self.offset, 0., 0.),
            AttachedPosition::Right => Gaps::new(0., 0., 0., 5. + self.offset),
        };
        let body: Element = match &self.body {
            TooltipBody::Text(t) => rect()
                .interactive(Interactive::No)
                .padding(Gaps::new(4., 10., 4., 10.))
                .border(Border::new().fill(th.hairline_strong()).width(1.))
                .background(th.panel())
                .corner_radius(CornerRadius::new_all(8.))
                .child(label().max_lines(1).font_size(12.5).color(th.text()).text(t.clone()))
                .into_element(),
            TooltipBody::Detailed(e) => tooltip_surface(th, e.clone()).into_element(),
        };

        rect()
            .a11y_role(AccessibilityRole::Tooltip)
            .a11y_focusable(false)
            .on_pointer_over(on_pointer_over)
            .on_pointer_out(on_pointer_out)
            .child(
                Attached::new(rect().children(self.children.clone()))
                    .position(self.position)
                    .maybe_child(is_visible.then(|| {
                        rect().opacity(opacity).scale(scale).padding(pad).child(body)
                    })),
            )
    }
    fn render_key(&self) -> DiffKey { self.key.clone().or(self.default_key()) }
}
```
(If `Gaps::new` arg order differs, mirror an existing `Gaps::new(...)` call in oxide-ui — it is `(top, right, bottom, left)`.)

- [ ] **Step 3: Register** in `oxide-ui/src/components/mod.rs`:
```rust
pub mod tooltip;
pub use tooltip::{OxideTooltip, AttachedPosition};
```

- [ ] **Step 4: Add `async-io`** to `oxide-ui/Cargo.toml` `[dependencies]` if `cargo build` errors on `async_io` (`async-io = "2"`; match the version Freya uses in the lock).

- [ ] **Step 5: Build + snapshot a text + detailed tooltip + READ.** Add an `#[ignore]` snapshot test in `tooltip.rs` that renders an `OxideTooltip` with the body forced visible (render the `body` element directly inside `tooltip_surface`/text on a dark `bg_deep` root) for `text` and `detailed`, `render_to_file` to `/tmp/tooltip-text.png` + `/tmp/tooltip-detailed.png`. (Forcing visibility avoids depending on the hover timer in a headless test — render the body element directly.) You CANNOT view images — render without panic + report paths.

Run: `cargo build -p oxide-ui` → Finished; `cargo test -p oxide-ui tooltip -- --ignored`.

- [ ] **Step 6: Commit**
```bash
git add oxide-app/crates/oxide-ui/src/components/tooltip.rs oxide-app/crates/oxide-ui/src/components/mod.rs oxide-app/crates/oxide-ui/Cargo.toml oxide-app/Cargo.lock
git commit -m "feat(oxide-ui): OxideTooltip (text/detailed, placement+offset+delay, hover-timer + animation)"
```

---

### Task 2: `TooltipGroup` warm-state ("instant after the first")

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/tooltip.rs` (add `TooltipGroup` + `TooltipGroupState`; group-aware `render`)
- Modify: `oxide-app/crates/oxide-ui/src/components/mod.rs` (re-export `TooltipGroup`)

**Interfaces:**
- Consumes: `OxideTooltip` (Task 1).
- Produces: `pub struct TooltipGroup` (`::new()`, `impl ChildrenExt`); `pub(crate) struct TooltipGroupState { pub warm: State<bool>, pub cold_task: State<Option<TaskHandle>> }` provided via context; `OxideTooltip::render` reads it with `use_try_consume::<TooltipGroupState>()` and uses effective delay 0 while warm. `pub fn group_effective_delay(group: &Option<TooltipGroupState>, base: Duration) -> Duration` (pure, unit-tested).

- [ ] **Step 1: Write the failing unit test** (append to `tooltip.rs` `#[cfg(test)] mod tests`):
```rust
#[test]
fn warm_makes_delay_instant_else_base() {
    use std::time::Duration;
    let base = Duration::from_millis(1000);
    // No group → base delay.
    assert_eq!(super::group_effective_delay_value(false, true, base), base, "no group → base");
    // Group present but cold → base delay.
    assert_eq!(super::group_effective_delay_value(true, false, base), base, "cold group → base");
    // Group present + warm → instant.
    assert_eq!(super::group_effective_delay_value(true, true, base), Duration::ZERO, "warm group → 0");
}
```
(We test a pure helper that takes `has_group: bool, warm: bool, base` — the signal-reading wrapper `group_effective_delay` calls it; this keeps the test free of Freya context.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p oxide-ui warm_makes_delay`
Expected: FAIL — `group_effective_delay_value` not found.

- [ ] **Step 3: Add `TooltipGroupState` + the pure helper + `TooltipGroup`** to `tooltip.rs`:
```rust
const TOOLTIP_COOLDOWN: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct TooltipGroupState {
    pub warm: State<bool>,
    pub cold_task: State<Option<TaskHandle>>,
}

/// Pure delay rule: instant only when a group is present AND warm.
pub(crate) fn group_effective_delay_value(has_group: bool, warm: bool, base: Duration) -> Duration {
    if has_group && warm { Duration::ZERO } else { base }
}

/// Groups a set of `OxideTooltip`s so that after the first shows, moving to
/// another is instant; leaving the group for `TOOLTIP_COOLDOWN` resets it.
#[derive(Default, Clone, PartialEq)]
pub struct TooltipGroup {
    children: Vec<Element>,
    key: DiffKey,
}
impl TooltipGroup { pub fn new() -> Self { Self::default() } }
impl KeyExt for TooltipGroup { fn write_key(&mut self) -> &mut DiffKey { &mut self.key } }
impl ChildrenExt for TooltipGroup { fn get_children(&mut self) -> &mut Vec<Element> { &mut self.children } }
impl Component for TooltipGroup {
    fn render(&self) -> impl IntoElement {
        use_provide_context(|| TooltipGroupState {
            warm: State::create(false),
            cold_task: State::create(None),
        });
        rect().children(self.children.clone())
    }
}
```

- [ ] **Step 4: Make `OxideTooltip::render` group-aware.** In `render`, after the hooks, add:
```rust
        let group = use_try_consume::<TooltipGroupState>();
        let has_group = group.is_some();
        let warm = group.as_ref().map(|g| *g.warm.read()).unwrap_or(false);
        let effective_delay = group_effective_delay_value(has_group, warm, self.delay);
```
Change the timer closure to use `effective_delay` and, on show, mark the group warm + on enter cancel its cooldown; on out, start the cooldown:
```rust
        let on_pointer_over = move |_| {
            if let Some(g) = &group { if let Some(h) = g.cold_task.write().take() { h.cancel(); } }
            if let Some(handle) = delay_task.write().take() { handle.cancel(); }
            let mut group2 = group;
            let task = spawn(async move {
                async_io::Timer::after(effective_delay).await;
                is_hovering.set_if_modified(true);
                if let Some(g) = &mut group2 { g.warm.set_if_modified(true); }
            });
            delay_task.set(Some(task));
        };
        let on_pointer_out = move |_| {
            if let Some(handle) = delay_task.write().take() { handle.cancel(); }
            is_hovering.set_if_modified(false);
            if let Some(mut g) = group {
                let mut warm = g.warm;
                let task = spawn(async move {
                    async_io::Timer::after(TOOLTIP_COOLDOWN).await;
                    warm.set_if_modified(false);
                });
                g.cold_task.set(Some(task));
            }
        };
```
(`State<T>` is `Copy`; capture copies into the closures. If borrow-checker friction arises, copy the needed signals into locals before the closures — mirror Task 1's pattern.)

- [ ] **Step 5: Run unit test + build**

Run: `cargo test -p oxide-ui warm_makes_delay` → PASS; `cargo test -p oxide-ui` → all pass; `cargo clippy -p oxide-ui` → clean.

- [ ] **Step 6: Re-export + commit**

In `mod.rs`: `pub use tooltip::{OxideTooltip, TooltipGroup, AttachedPosition};`
```bash
git add oxide-app/crates/oxide-ui/src/components/tooltip.rs oxide-app/crates/oxide-ui/src/components/mod.rs
git commit -m "feat(oxide-ui): TooltipGroup warm-state (instant after first; 400ms cooldown)"
```

---

### Task 3: Wire the sidebar collapsed rail

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs`

**Interfaces:**
- Consumes: `OxideTooltip`, `TooltipGroup`, `AttachedPosition` (`oxide_ui::components`).

- [ ] **Step 1: Wrap the rail in a `TooltipGroup` + each item in an `OxideTooltip`.** In the collapsed-rail builder (the `convs_rail` block): wrap the project dot in `OxideTooltip::text(<project name or "Projects">).placement(AttachedPosition::Right).offset(6.).child(<dot>)`; wrap each conversation icon in `OxideTooltip::detailed(<detail body>).placement(AttachedPosition::Right).offset(6.).child(<icon>)` where the detail body is a small vertical `rect` with the conversation `title` (text()) + a faint line `model · worktree-branch` (reuse the data already in the `c`/`convs_rail` loop). Wrap the whole rail column in `TooltipGroup::new().child(rail.into_element())`. Add `OxideTooltip, TooltipGroup, AttachedPosition` to the `oxide_ui::components::{...}` import.

```rust
// detail body for a conversation icon:
let detail = rect()
    .direction(Direction::Vertical)
    .spacing(3.)
    .child(label().text(c.title.clone()).font_size(12.5).color(th.text()))
    .child(label()
        .text(format!("{}{}", c.model,
            c.worktree.as_ref().map(|w| format!(" · {}", w.branch)).unwrap_or_default()))
        .font_size(10.5).color(th.faint()));
```
(If `c.title`/`c.model`/`c.worktree` field access differs, mirror the existing `ListItem`/rail usage in this file. Use `th` already in scope.)

- [ ] **Step 2: Build + snapshot the collapsed-rail-with-tooltip + READ.** Reuse/extend the existing collapsed-sidebar snapshot to render with one conversation tooltip forced visible (or render the rail + a tooltip body). `render_to_file` to `/tmp/sidebar-rail-tooltip.png`. Controller reads it.

Run: `cargo test -p oxide-freya -- --ignored` (the sidebar/shell collapsed snapshot); `cargo clippy -p oxide-freya` clean.

- [ ] **Step 3: Commit**
```bash
git add oxide-app/crates/oxide-freya/src/regions/sidebar.rs
git commit -m "feat(sidebar): tooltips on collapsed-rail items (project + conversations, placement Right)"
```

---

### Task 4: Wire the right-panel direction rail

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/regions/context/mod.rs`

- [ ] **Step 1: Wrap the direction rail in `TooltipGroup` + each icon in `OxideTooltip::text`.** In the collapsed branch's rail loop (the 3 direction icon buttons), wrap each in `OxideTooltip::text(<RightTab label, e.g. "Run"/"Worktree"/".oxide">).placement(AttachedPosition::Left).offset(6.).child(<icon button>)`; wrap the rail column in `TooltipGroup::new().child(...)`. (Right rail tooltips point LEFT, into the window.) The labels come from the existing `TABS` constant in this file. Import `OxideTooltip, TooltipGroup, AttachedPosition`.

- [ ] **Step 2: Build + snapshot + READ.** Render the right-panel rail with a tooltip forced visible → `/tmp/context-rail-tooltip.png`. Controller reads it.

Run: `cargo test -p oxide-freya context -- --ignored`; `cargo clippy -p oxide-freya` clean.

- [ ] **Step 3: Commit**
```bash
git add oxide-app/crates/oxide-freya/src/regions/context/mod.rs
git commit -m "feat(context): tooltips on direction-rail icons (placement Left)"
```

---

### Task 5: Placement snapshots + full verify

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/tooltip.rs` (placement snapshots)

- [ ] **Step 1: Snapshot the text + detailed tooltip at all 4 placements.** Add `#[ignore]` snapshots rendering an `OxideTooltip` trigger (a small box) with the body forced visible at `Top/Bottom/Left/Right` (text variant) + one `detailed` at `Right`, on a dark `bg_deep` root centered (so the Attached body is on-screen), `poll` past 150ms, `render_to_file` to `/tmp/tooltip-{top,bottom,left,right}.png` + `/tmp/tooltip-detailed-right.png`. Controller reads them — confirm placement + the panel-surface look.

- [ ] **Step 2: Full build + suite + clippy + binary.**
```
cargo build -p oxide-freya --bin oxide-freya     # Finished
cargo test -p oxide-ui -p oxide-freya            # all pass
cargo clippy -p oxide-ui -p oxide-freya          # clean (note pre-existing main_region.rs test warnings)
```

- [ ] **Step 3: Render + read all tooltip snapshots** (`/tmp/tooltip-*.png`, `/tmp/sidebar-rail-tooltip.png`, `/tmp/context-rail-tooltip.png`). Controller confirms: text + detailed bodies, correct placement, rail integration.

- [ ] **Step 4: Commit**
```bash
git add oxide-app/crates/oxide-ui/src/components/tooltip.rs
git commit -m "test(tooltip): placement snapshots (text + detailed, all sides)"
```

---

## Self-Review notes

- **Spec coverage:** OxideTooltip text/detailed + placement + offset + delay + animation (T1) ✓; TooltipGroup warm/cooldown/instant-after-first (T2) ✓; sidebar rail integration (T3) ✓; right rail integration placement Left (T4) ✓; placement + rail snapshots (T1/T5) ✓; unit-test the warm rule (T2) ✓; live-feel deferred to live check (noted in spec) ✓.
- **Type consistency:** `OxideTooltip::{text,detailed,placement,offset,delay,child}`, `AttachedPosition`, `TooltipGroup::new`, `TooltipGroupState{warm,cold_task}`, `group_effective_delay_value(has_group,warm,base)`, `tooltip_surface(th,child)` used identically across tasks. `delay` 1000ms / cooldown 400ms / anim 150ms consistent.
- **Freya API caveats flagged:** `AttachedPosition`/`Attached` import path, `Gaps::new` arg order, `async-io` dep, and the `render_to_file`/`poll` testing API each say "mirror the Freya source / an existing call" rather than guess. The headless tests render the body element directly (force-visible) since the hover *timer* can't be exercised headlessly.
- **Crate boundary:** `oxide-ui` tooltip takes only owned content + config; no `oxide-freya` types. ✓
- **Deferred:** slices A (+ button), B (menu animation), C (menu item types); tooltips beyond the two rails.
