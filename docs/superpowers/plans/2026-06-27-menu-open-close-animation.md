# Menu Open/Close Animations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Animate our popup menus open AND closed like the Projects `Select` dropdown by adopting the Select animation pattern in `Popover` + `OxideContextMenuViewer`.

**Architecture:** A *persistent* `use_animation` (`OnChange::Rerun` + **`OnCreation::Finish`**) drives `scale 0.9→1` + `opacity 0→1` + a placement-aware `offset_y` slide; on close it `into_reversed()` in place, and the overlay stays mounted while `opacity > 0` so the exit tween plays. Edge-aware positioning is untouched.

**Tech Stack:** Rust, Freya blog/0.4 (`use_animation`/`AnimNum`/`OnChange`/`OnCreation`/`Ease`/`Function`, `.scale()`/`.opacity()`/`.offset_y()`), `freya_testing`.

## ⚠️ Reconciliation with the existing `popover.rs` module-doc warning (READ FIRST)

`popover.rs:42-49` explicitly warns: *"Do NOT revert to hoisting `use_animation` into `Popover::render` or using `OnChange::Rerun` + `open()` — both defeat the per-open replay"* because a prior attempt's hook *"ran to 1.0 immediately"* at first mount, making the fade a no-op.

That prior attempt used **`OnCreation::Run`** (run-on-mount), which is exactly what runs the tween to 1.0 before the first open. **This plan uses `OnCreation::Finish`** (jump to the *finished* state on mount, do NOT run) — the same as Freya's `Select`, which provably replays on every open/close. With `OnCreation::Finish` + a factory that returns the **reversed** animation when closed, the mount state is "closed-finished" = `opacity 0` (hidden), and the first `open=true` triggers `OnChange::Rerun` forward. This is a *different* mechanism than the documented-broken one. **Task 1 Step 1 is a de-risking spike that proves the close animation actually plays before the full migration**, and Step 7 rewrites the now-stale module doc. If the spike fails, Step 1 specifies the fallback.

## Global Constraints

- Animation params (match Select exactly), each via `AnimNum::new(a,b).time(125).ease(Ease::Out).function(Function::Quart)`: `scale = (0.9, 1.)`, `opacity = (0., 1.)`, `slide = (SLIDE_FROM, 0.)`. `use freya::animation::*;` (already imported in popover.rs).
- Hook config: `conf.on_change(OnChange::Rerun); conf.on_creation(OnCreation::Finish);` then `if open { (scale, opacity, slide) } else { (scale.into_reversed(), opacity.into_reversed(), slide.into_reversed()) }`. Read: `let (scale, opacity, slide) = animation.read().value();`.
- `SLIDE_FROM`: Popover = `-8.` when effective placement is `Below`, `+8.` when `Above`. Context menu = `-8.` (always opens downward from cursor).
- Keep the overlay/menu node mounted while `open || opacity > 0.0` (Popover) / `ctx.menu.is_some() || opacity > 0.0` (context menu) so the exit tween plays before unmount.
- Apply animation as the LAST visual layer on the already-positioned rect: `.scale(scale).offset_y(off_top_or_global + slide).opacity(gated)` — never mutate the edge-aware `off_top`/`off_left`/`overflow_offset` math.
- Cleanup on close: clear cached measurement (`content_size` / `measured`) once `!open && opacity == 0.0` so the next open re-measures.
- `oxide-ui` only; no new deps. Rust quality: `cargo clippy -p oxide-ui` clean; hand-formatted; no gold-plating.

**Run every cargo command as:**
```
distrobox enter claude_development -- bash -lc 'cd <WORKTREE>/oxide-app && CARGO_TARGET_DIR=<WARM_TARGET> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo <args>'
```
Gold-standard reference: `/run/media/system/fastdrive/repos/freya/crates/freya-components/src/select.rs:140-172` (read it).

---

### Task 1: `Popover` — Select-style open+close animation

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/menu/popover.rs`

**Interfaces:**
- Produces: an animated `Popover` (no API change to `new`/`open`/`placement`/`content`). Internal: `PopoverOverlay` gains `scale: f32`, `opacity: f32`, `slide: f32` fields; a free helper `fn slide_from(p: Placement) -> f32` (Below → -8., Above → +8.).

- [ ] **Step 1 (DE-RISK SPIKE): write a test that proves the close animation plays, THEN make it pass.** Add to the `tests` module:
```rust
/// The exit animation must keep the overlay mounted briefly after close
/// (proves the persistent-hook reverse tween plays, not an instant unmount).
#[test]
fn popover_exit_animation_keeps_content_mounted_briefly() {
    use freya_testing::TestingRunner;
    fn app() -> Element {
        let mut open = use_state(|| false);
        let trigger = rect().width(Size::px(80.)).height(Size::px(32.))
            .on_press(move |_: Event<PressEventData>| open.toggle())
            .child(label().text("anchor").font_size(12.)).into_element();
        Popover::new(trigger).open(open()).content(label().text("MENU").into_element()).into_element()
    }
    let (mut t, _) = TestingRunner::new(app, (400., 320.).into(), |_| {}, 1.);
    t.poll(std::time::Duration::from_millis(5), std::time::Duration::from_millis(40));
    t.sync_and_update();
    let center = t.find(|node, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("anchor"))
        .map(|_| { let c = node.layout().visible_area().center(); (c.x as f64, c.y as f64) })).unwrap();
    t.click_cursor(center); // open
    t.poll(std::time::Duration::from_millis(10), std::time::Duration::from_millis(200));
    t.sync_and_update();
    let center = t.find(|node, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("anchor"))
        .map(|_| { let c = node.layout().visible_area().center(); (c.x as f64, c.y as f64) })).unwrap();
    t.click_cursor(center); // close
    // Only a SHORT poll — less than the 125ms exit tween. Content must STILL be mounted.
    t.poll(std::time::Duration::from_millis(5), std::time::Duration::from_millis(30));
    t.sync_and_update();
    let mid_close = t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("MENU")));
    assert!(mid_close.is_some(), "content must remain mounted during the exit animation");
}
```
Run it BEFORE implementing → it FAILS (today the overlay unmounts instantly). Then do Steps 2-5. After, this test passing proves the exit tween plays. **If after Steps 2-5 this test still fails because `OnChange::Rerun` does not fire on the plain-`bool` prop re-render** (Select reads a signal, we read a prop), apply the FALLBACK: mirror the prop into a local signal so the factory subscribes —
```rust
let mut open_sig = use_state(|| open);
open_sig.set_if_modified(open);             // sync prop → signal each render
// ...then read `open_sig()` (a real signal) inside the use_animation factory instead of the bare `open`.
```
Document in the report which path (bare prop vs `open_sig` fallback) was needed.

- [ ] **Step 2: Add the persistent animation hook in `Popover::render`.** After `effective_placement` is known (so the slide direction is correct), before building the overlay:
```rust
        let animation = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            conf.on_creation(OnCreation::Finish);
            let scale = AnimNum::new(0.9_f32, 1.).time(125).ease(Ease::Out).function(Function::Quart);
            let opacity = AnimNum::new(0._f32, 1.).time(125).ease(Ease::Out).function(Function::Quart);
            let slide = AnimNum::new(slide_from(effective_placement), 0.).time(125).ease(Ease::Out).function(Function::Quart);
            if open { (scale, opacity, slide) } else { (scale.into_reversed(), opacity.into_reversed(), slide.into_reversed()) }
        });
        let (scale, opacity, slide) = animation.read().value();
```
Add the helper above the impls:
```rust
fn slide_from(p: Placement) -> f32 { match p { Placement::Below => -8.0, Placement::Above => 8.0 } }
```

- [ ] **Step 3: Keep the overlay mounted while fading.** Change the overlay gate (currently `(open && content_el.is_some())`):
```rust
        let overlay: Option<Element> = ((open || opacity > 0.0) && content_el.is_some()).then(|| {
            PopoverOverlay::new(
                content_el.clone().unwrap(), content_size, offset_top, offset_left, positioned,
                scale, opacity, slide,
            ).into_element()
        });
```
And clear the cached size once fully closed (place after the overlay let-binding):
```rust
        if !open && opacity == 0.0 && content_size.peek().is_some() {
            let mut cs = content_size; cs.set(None);
        }
```

- [ ] **Step 4: Thread anim values into `PopoverOverlay`.** Add `scale: f32, opacity: f32, slide: f32` to the struct + `new(...)` (after `positioned`). In `PopoverOverlay::render`, DELETE the `use_animation(OnCreation::Run)` block (lines ~119-126); replace the opacity/position application:
```rust
        let final_opacity = if self.positioned { self.opacity } else { 0.0_f32 };
        let mut parent_size = self.parent_size;
        let content = self.content.clone();
        rect()
            .layer(Layer::Overlay)
            .position(Position::new_absolute().top(self.offset_top).left(self.offset_left))
            .scale(self.scale)
            .offset_y(self.slide)
            .opacity(final_opacity)
            .on_sized(move |e: Event<SizedEventData>| {
                // Only record size while opening/visible so a fading overlay doesn't re-measure.
                if final_opacity > 0.0 { parent_size.set_if_modified(Some(e.area.size)); }
            })
            .child(content)
```
(`.offset_y(self.slide)` is ADDED on top of the absolute `.top(offset_top)` — it shifts the rendered rect without touching the positioning math. Verify `.offset_y` exists on rect; the context menu rect uses `.offset_y(...)`, so it does.)

- [ ] **Step 5: Run the spike + existing tests.**

Run: `cargo test -p oxide-ui popover` 
Expected: `popover_exit_animation_keeps_content_mounted_briefly` PASS; `popover_hidden_when_closed` PASS (closed → `OnCreation::Finish` settles opacity 0 → gate false → not mounted); `popover_shows_content_when_open` PASS (open → finishes at opacity 1 → mounted); `popover_opens_in_nested_context` PASS.

- [ ] **Step 6: Fix the reopen test for the exit fade.** `popover_content_visible_after_reopen`'s "content should NOT render after close" assertion polls only ~40ms after close — now the exit tween keeps it mounted that long. Change the post-close poll to run PAST the tween before asserting it's gone:
```rust
        // close — then poll past the 125ms exit tween so the overlay fully unmounts
        t.click_cursor(center);
        t.poll(std::time::Duration::from_millis(10), std::time::Duration::from_millis(260));
        t.sync_and_update();
        let after_close = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("MENU"))
        });
        assert!(after_close.is_none(), "content should NOT render after close + exit animation");
```
Run: `cargo test -p oxide-ui popover` → all pass.

- [ ] **Step 7: Rewrite the stale module doc.** Replace the "## Entrance animation — per-open-mounted overlay subcomponent" section + the "Do NOT revert to hoisting" warning (lines ~33-66) with the new model: persistent `use_animation` in `Popover::render` using **`OnCreation::Finish`** (NOT `Run`) + `OnChange::Rerun` + `into_reversed()` on close; overlay stays mounted while `opacity>0`; why `OnCreation::Finish` avoids the prior `OnCreation::Run`-ran-to-1.0-at-mount no-op. Keep the dismissal + edge-aware-positioning + measurement-composition sections (still accurate). `cargo clippy -p oxide-ui` clean.

- [ ] **Step 8: Snapshot a mid-open Popover + READ.** Add an `#[ignore]` snapshot that opens a Popover (click), polls to ~60ms (mid-tween), `render_to_file` to `/tmp/popover-mid-open.png`. (Mid-frame proves scale<1/opacity<1 applied; can't show motion.) Render without panic + report the path.

- [ ] **Step 9: Commit**
```bash
git add oxide-app/crates/oxide-ui/src/components/menu/popover.rs
git commit -m "feat(popover): Select-style open+close animation (scale+opacity+placement slide, persistent OnCreation::Finish hook)"
```

---

### Task 2: `OxideContextMenuViewer` — same animation

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/menu/context_menu.rs`

**Interfaces:**
- Consumes: the `OnChange::Rerun`+`OnCreation::Finish` pattern from Task 1.
- Produces: animated context menu (no API change to `open_context_menu`/`close_context_menu`).

- [ ] **Step 1: Persist the last menu so it survives the exit fade.** In `OxideContextMenuViewer::render`, after `ctx` is set up, add a root-scoped persist signal + sync:
```rust
        let mut last_menu: State<Option<(CursorPoint, Menu)>> =
            use_hook(|| State::create_in_scope(None, ScopeId::ROOT));
        use_side_effect(move || {
            if let Some(m) = ctx.menu.read().clone() { last_menu.set(Some(m)); }
        });
```
(If `Menu` is not `Clone`/`PartialEq` enough to store in `State`, fall back to persisting just `(CursorPoint, the rebuilt menu element)`; check `Menu`'s derives — `ctx.menu` already stores `Option<(CursorPoint, Menu)>`, so `Menu` is storable.)

- [ ] **Step 2: Add the persistent animation hook.** Before the render rect:
```rust
        let menu_open = ctx.menu.read().is_some();
        let animation = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            conf.on_creation(OnCreation::Finish);
            let scale = AnimNum::new(0.9_f32, 1.).time(125).ease(Ease::Out).function(Function::Quart);
            let opacity = AnimNum::new(0._f32, 1.).time(125).ease(Ease::Out).function(Function::Quart);
            let slide = AnimNum::new(-8._f32, 0.).time(125).ease(Ease::Out).function(Function::Quart);
            if menu_open { (scale, opacity, slide) } else { (scale.into_reversed(), opacity.into_reversed(), slide.into_reversed()) }
        });
        let (anim_scale, anim_opacity, anim_slide) = animation.read().value();
```
(`use freya::animation::*;` — add the import if not present.)

- [ ] **Step 3: Render from the persisted menu while fading + apply the animation.** Replace the `.maybe_child(ctx.menu.read().clone().map(...))` with a render sourced from `ctx.menu.read().clone().or_else(|| last_menu.read().clone())`, gated on `menu_open || anim_opacity > 0.0`, and fold the animation into the existing measure-gate:
```rust
        .maybe_child(
            (menu_open || anim_opacity > 0.0)
                .then(|| ctx.menu.read().clone().or_else(|| last_menu.read().clone()))
                .flatten()
                .map(|(at, menu)| {
                    let at = at.to_f32();
                    let (offset_x, offset_y, measure_gate) = match ctx.measured.read().as_ref() {
                        None => (0.0_f32, 0.0_f32, 0.0_f32),
                        Some((area, root)) => (
                            overflow_offset(area.origin.x, area.size.width, root.width),
                            overflow_offset(area.origin.y, area.size.height, root.height),
                            1.0_f32,
                        ),
                    };
                    let final_opacity = measure_gate * anim_opacity;
                    let mut measured = ctx.measured;
                    rect()
                        .layer(Layer::Overlay)
                        .position(Position::new_global().left(at.x).top(at.y))
                        .offset_x(offset_x)
                        .offset_y(offset_y + anim_slide)
                        .scale(anim_scale)
                        .opacity(final_opacity)
                        .on_sized(move |e: Event<SizedEventData>| {
                            if measured.peek().is_none() {
                                if let Some(w) = *win.peek() { measured.set(Some((e.area, w))); }
                            }
                        })
                        .child(menu.on_close(move |_| match (ctx.close_request)() {
                            CloseReq::None => ctx.close_request.set(CloseReq::Pending),
                            CloseReq::Pending => {
                                ctx.menu.set(None);
                                ctx.close_request.set(CloseReq::None);
                            }
                        }))
                }),
        )
```
(`offset_y + anim_slide`: the slide adds to the overflow nudge, not the global cursor position. `.scale()` is new. The `on_close`/`CloseReq` dismissal is unchanged — and because the node stays mounted during the fade, click-away still works.)

- [ ] **Step 4: Clear `measured` once fully closed** so the next open re-measures at the new cursor. After the render rect (or as a side effect):
```rust
        if !menu_open && anim_opacity == 0.0 && ctx.measured.peek().is_some() {
            let mut m = ctx.measured; m.set(None);
        }
```
(Place where it runs each render; `ctx.measured` is `Copy`.)

- [ ] **Step 5: Build + clippy.**

Run: `cargo build -p oxide-ui` Finished; `cargo test -p oxide-ui` all pass; `cargo clippy -p oxide-ui` clean. (Headless can't drive the right-click open/close timing — the interaction is controller-live-tested.)

- [ ] **Step 6: Snapshot a context menu mid-open if feasible + READ.** If you can force `ctx.menu = Some(...)` + `measured = Some(...)` + poll to ~60ms in a harness, `render_to_file` to `/tmp/context-menu-mid-open.png` and report it. If forcing the root-scoped state is impractical, note that the context-menu animation is controller-live-tested only.

- [ ] **Step 7: Full verify + commit**
```
cargo build -p oxide-freya --bin oxide-freya   # Finished (the consumers compile)
cargo test -p oxide-ui -p oxide-freya          # all pass
cargo clippy -p oxide-ui -p oxide-freya        # clean (pre-existing main_region.rs test warnings KNOWN)
```
```bash
git add oxide-app/crates/oxide-ui/src/components/menu/context_menu.rs
git commit -m "feat(context-menu): Select-style open+close animation (scale+opacity+slide; persist menu during fade)"
```

---

## Self-Review notes

- **Spec coverage:** Popover persistent anim + scale/opacity/placement-slide + keep-mounted + cleanup + guard-remeasure (T1) ✓; context menu same anim + persist-last-menu + measure-gate*anim + dismissal-during-fade + measured cleanup (T2) ✓; match-Select params ✓; edge-positioning untouched ✓; mid-open snapshots + live-test note ✓.
- **The documented-warning conflict** is reconciled up front (OnCreation::Finish ≠ the prior OnCreation::Run) with a de-risk spike (T1 S1) + fallback (`open_sig`) + module-doc rewrite (T1 S7).
- **Type consistency:** `slide_from(Placement)->f32`, `PopoverOverlay{scale,opacity,slide}`, `(scale,opacity,slide) = animation.read().value()`, `menu_open`, `last_menu`, `measure_gate`, `final_opacity` consistent across tasks.
- **Flagged verify-against-source:** `OnChange::Rerun` firing on a plain-bool prop (the spike + fallback covers it), `.offset_y`/`.scale` on rect (context menu already uses offset_y), `Menu` storable in `State` (ctx.menu already does), `use_animation().read().value()` tuple read (mirror select.rs). Each step says verify/mirror.
- **Deferred:** Slice C (interrupt-closure); per-item animations; Freya `Select` (already animates).
