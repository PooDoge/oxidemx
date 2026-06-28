# Responsive Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Below the 1180px breakpoint the user can manually expand a sidebar (it sticks), the expanded panel floats over content as a drawer that auto-collapses on outside-click/Escape, and both the left sidebar and right context panel behave this way.

**Architecture:** Make `sidebar_collapsed`/`context_collapsed` the sole rendered truth (drop the `|| force_rail` continuous override) and drive the responsive default from an edge-triggered size-class effect. At narrow widths render each panel's 60px rail inline (stable content width) plus a `Layer::Overlay` drawer + dimmed backdrop when expanded; the backdrop converts outside-press/Escape into collapse.

**Tech Stack:** Rust, Freya (overlay), `freya_testing`.

## Global Constraints

- `cargo clippy -p oxide-freya` (and `-p oxide-ui` if touched) clean (warnings = defects).
- Hand-formatted: match each file's style; only format lines you add; NO repo-wide `cargo fmt`.
- No gold-plating: exactly the spec (no drawer slide animation, no breakpoint changes).
- Freya rule: hooks (`use_state`, `use_side_effect`) UNCONDITIONAL at the top of `render`.
- **Read a signal into a local before any `.set()`** — `if let Some(x) = sig.read()...{ sig.set(..) }` holds the read guard across the body and panics ("State already borrowed"). Bind first.
- Reuse `Sidebar`/`ContextRegion` (both take `collapsed: bool`) and `Size::window_percent(100.)` for overlays (NOT `Size::fill`, which sizes to the parent).
- Breakpoint = `SizeClass::is_compact_or_narrower()` = `!Wide` = logical width **< 1180**.
- Build: distrobox `claude_development`, from `oxide-app/`, `CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo …`.

---

### Task 1: Sole-truth signal + edge-triggered responsive default

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/state.rs` (add `crossing_collapse` + test)
- Modify: `oxide-app/crates/oxide-freya/src/app.rs` (drop `|| force_rail`; add the effect)

**Interfaces:**
- Produces: `pub fn crossing_collapse(was_wide: bool, now_wide: bool, wide_default_collapsed: bool) -> Option<bool>`.
- `SizeClass` already derives `Clone, Copy, PartialEq, Eq, Debug, Default` (verified) — no change.

- [ ] **Step 1: Write the `crossing_collapse` test (failing).** Append to the `#[cfg(test)] mod tests` in `state.rs`:
```rust
    #[test]
    fn crossing_collapse_truth_table() {
        // no crossing → None
        assert_eq!(super::crossing_collapse(true, true, false), None);
        assert_eq!(super::crossing_collapse(false, false, true), None);
        // shrank (wide→narrow) → collapse both panels
        assert_eq!(super::crossing_collapse(true, false, false), Some(true));
        assert_eq!(super::crossing_collapse(true, false, true), Some(true));
        // grew (narrow→wide) → each panel's wide default
        assert_eq!(super::crossing_collapse(false, true, false), Some(false)); // left: expanded
        assert_eq!(super::crossing_collapse(false, true, true), Some(true));   // right: collapsed
    }
```

- [ ] **Step 2: Run it (fails to compile — `crossing_collapse` undefined).**
```
distrobox enter claude_development -- bash -lc "cd $PWD && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya crossing_collapse"
```
Expected: compile error `cannot find function crossing_collapse`.

- [ ] **Step 3: Implement `crossing_collapse`** in `state.rs` (near `SizeClass`):
```rust
/// What a breakpoint crossing should do to a panel's `collapsed` flag.
/// `None` = no crossing (leave the user's value). On a crossing: shrinking (now narrow)
/// collapses; growing (now wide) restores the panel's wide default.
pub fn crossing_collapse(was_wide: bool, now_wide: bool, wide_default_collapsed: bool) -> Option<bool> {
    if was_wide == now_wide {
        None
    } else if now_wide {
        Some(wide_default_collapsed)
    } else {
        Some(true)
    }
}
```

- [ ] **Step 4: Run the test (PASS).** Same command as Step 2 → PASS.

- [ ] **Step 5: Drop the continuous override + add the edge effect in `app.rs`.** In `shell()`, replace:
```rust
    let sc = *state.size_class.read();
    let force_rail = sc.is_compact_or_narrower();
    let sidebar_collapsed = *state.sidebar_collapsed.read() || force_rail;
    let context_collapsed = *state.context_collapsed.read() || force_rail;
```
with:
```rust
    let sc = *state.size_class.read();
    let narrow = sc.is_compact_or_narrower();
    let sidebar_collapsed = *state.sidebar_collapsed.read();
    let context_collapsed = *state.context_collapsed.read();

    // Edge-triggered responsive default: a breakpoint crossing collapses (shrink) or
    // restores the wide default (grow). Between crossings the user's toggle is authoritative,
    // so manual expand BELOW the breakpoint now sticks. `prev_sc` starts Wide so a narrow
    // first render counts as a down-crossing (collapses on launch when small).
    let mut prev_sc = use_state(|| crate::state::SizeClass::Wide);
    {
        let mut sidebar_c = state.sidebar_collapsed;
        let mut context_c = state.context_collapsed;
        let size_class_sig = state.size_class;
        use_side_effect(move || {
            let now = *size_class_sig.read();
            let was = *prev_sc.peek(); // peek: react to size_class, not to prev_sc
            let was_wide = matches!(was, crate::state::SizeClass::Wide);
            let now_wide = matches!(now, crate::state::SizeClass::Wide);
            if let Some(v) = crate::state::crossing_collapse(was_wide, now_wide, false) {
                sidebar_c.set(v); // left panel: wide default = expanded (false)
            }
            if let Some(v) = crate::state::crossing_collapse(was_wide, now_wide, true) {
                context_c.set(v); // right panel: wide default = collapsed (true)
            }
            prev_sc.set(now);
        });
    }
```
Notes: `state.size_class` / `state.sidebar_collapsed` / `state.context_collapsed` are `State<_>` (Copy) — capture by copy. `now`/`was` are read into locals before any `.set()` (avoids the borrow panic). `prev_sc.peek()` does not subscribe, so the effect re-runs on `size_class` changes (the probe), not on its own `prev_sc.set`. The existing `mut size_class = state.size_class;` binding for the probe stays.

- [ ] **Step 6: Build + clippy.**
```
distrobox enter claude_development -- bash -lc "cd $PWD && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build -p oxide-freya && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo clippy -p oxide-freya"
```
Expected: builds; clippy clean. At this point the layout is still inline (Task 2 adds the drawer), but manual expand below the breakpoint already sticks, and crossings auto-collapse/restore. `cargo test -p oxide-freya` stays green.

- [ ] **Step 7: Commit.**
```bash
git add oxide-app/crates/oxide-freya/src/state.rs oxide-app/crates/oxide-freya/src/app.rs
git commit -m "feat(shell): sole-truth collapse + edge-triggered responsive default"
```

---

### Task 2: Drawer overlay + auto-collapse-on-blur (both panels)

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/app.rs` (narrow rail+drawer layout + backdrops)

**Interfaces:**
- Consumes: `narrow`, `sidebar_collapsed`, `context_collapsed` (Task 1); `Sidebar { state, collapsed }`, `ContextRegion { state, collapsed }`; `Theme` helpers `with_alpha`, `bg_deep`, `shadow_deep` (from `oxide-ui`).

- [ ] **Step 1: Make the inline body rail-based when narrow.** In `shell()`'s body closure, the
  `right_group` and `shell_body` currently branch on `context_collapsed`/`sidebar_collapsed`. Change
  each so that **when `narrow`, always render the 60px rail** (stable content width), and only at
  **wide** widths use the `collapsed`-driven rail-vs-`ResizableContainer` choice. Concretely, gate the
  existing `ResizableContainer` branches on `!narrow`:
  - `right_group`: `if context_collapsed || narrow { <60px right rail + flex content> } else { <ResizableContainer …> }`
  - `shell_body`: `if sidebar_collapsed || narrow { <60px left rail + flex right_group> } else { <ResizableContainer …> }`
  (Keep the exact rail/ResizableContainer rect code that's already there; only change the branch
  conditions to `|| narrow`. The rails already pass `collapsed: true` to `Sidebar`/`ContextRegion`.)

- [ ] **Step 2: Add the drawer overlays as root-level siblings.** After the body `.child({...})` in the
  root `rect()` (so the drawers float above everything via `Layer::Overlay`), add:
```rust
        // ── Drawers: narrow + expanded → float the full panel over content ────────
        .maybe_child((narrow && !sidebar_collapsed).then(|| {
            let mut coll = state.sidebar_collapsed;
            let mut coll2 = state.sidebar_collapsed;
            let th = oxide_ui::tokens::Theme::default(); // shell theme accessor; match how shell gets Theme
            rect()
                .layer(Layer::Overlay)
                .position(Position::new_global().left(0.0).top(0.0))
                .width(Size::window_percent(100.))
                .height(Size::window_percent(100.))
                .background(oxide_ui::tokens::Theme::with_alpha(th.bg_deep(), 0x66))
                .on_press(move |_: Event<PressEventData>| coll.set(true))
                .on_global_key_down(move |e: Event<KeyboardEventData>| {
                    if e.key == Key::Named(NamedKey::Escape) { coll2.set(true); }
                })
                .child(
                    rect()
                        .position(Position::new_global().left(0.0).top(0.0))
                        .width(Size::px(274.))
                        .height(Size::window_percent(100.))
                        .shadow((0.0_f32, 0.0_f32, 48.0_f32, 0.0_f32, th.shadow_deep()))
                        .child(Sidebar { state: state.clone(), collapsed: false }),
                )
                .into_element()
        }))
        .maybe_child((narrow && !context_collapsed).then(|| {
            let mut coll = state.context_collapsed;
            let mut coll2 = state.context_collapsed;
            let th = oxide_ui::tokens::Theme::default();
            rect()
                .layer(Layer::Overlay)
                .position(Position::new_global().left(0.0).top(0.0))
                .width(Size::window_percent(100.))
                .height(Size::window_percent(100.))
                .background(oxide_ui::tokens::Theme::with_alpha(th.bg_deep(), 0x66))
                .on_press(move |_: Event<PressEventData>| coll.set(true))
                .on_global_key_down(move |e: Event<KeyboardEventData>| {
                    if e.key == Key::Named(NamedKey::Escape) { coll2.set(true); }
                })
                .child(
                    rect()
                        .position(Position::new_global().top(0.0).right(0.0))
                        .width(Size::px(348.))
                        .height(Size::window_percent(100.))
                        .shadow((0.0_f32, 0.0_f32, 48.0_f32, 0.0_f32, th.shadow_deep()))
                        .child(ContextRegion { state: state.clone(), collapsed: false }),
                )
                .into_element()
        }))
```
Implementer notes:
- **Theme access:** use whatever the shell already uses to get a `Theme` (the shell calls
  `use_init_theme(oxide_dark_theme)`; obtain the active `Theme` the same way other shell code does —
  check `MainRegion`/`Sidebar` for the idiom, e.g. a `use_theme`/`Theme::` accessor — rather than
  `Theme::default()` if the app uses a themed instance). Match the existing pattern; `Theme::default()`
  is a fallback only if that's what the shell uses.
- **Right-drawer anchor:** `Position::new_global().top(0.).right(0.)` anchors to the right edge. If
  `new_global()` has no `.right()`, compute `left = root_width - 348.` from `Platform::get().root_size`
  (note: root_size is PHYSICAL px — divide by scale factor) — but try `.right(0.)` first; it's the
  clean path.
- Two clones of the collapse signal (`coll`/`coll2`) because the press and key closures each capture
  one (`State<bool>` is Copy, so this is cheap; name them clearly).
- The drawer is only mounted while `!collapsed`, so the » press that opened it (on the rail, before
  this mounts) cannot reach the backdrop and self-close.

- [ ] **Step 3: Build + clippy.**
```
distrobox enter claude_development -- bash -lc "cd $PWD && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build -p oxide-freya && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo clippy -p oxide-freya"
```
Expected: builds; clippy clean. Resolve any `Theme`/`Position::right`/import issues per the notes (import `Key`, `NamedKey`, `PressEventData`, `KeyboardEventData` from `freya::prelude::*` as the file already does).

- [ ] **Step 4: Snapshot test — drawer floats, not pushes.** Add a test in `app.rs`'s test module
  (it already has `mock_with_history`/`harness_with_history` helpers — follow their idiom to mount the
  shell body at a fixed narrow size, e.g. `TestingRunner::new(app, (1000., 700.).into(), |_| {}, 1.)`
  so width 1000 < 1180 = narrow). With the left panel expanded (`sidebar_collapsed = false`):
  - assert a `Layer::Overlay` rect exists whose subtree contains a Sidebar marker (e.g. the "Search…"
    placeholder or the conversations list) — the drawer is present;
  - assert the MainRegion content node's `visible_area().min_x()` is ≈ 60 (the rail width), proving the
    content did NOT shift right by 274 (float, not push).
  Match the exact `find`/`visible_area()` idiom used in `popover.rs` tests. If driving the
  `sidebar_collapsed=false`-at-narrow state in the harness is awkward (the edge effect may recollapse),
  set the signal directly after mount (as other tests do) and `poll`+`sync_and_update` before asserting.
  If the harness can't hold that state deterministically, downgrade this to a comment noting it's
  live-verified and keep the Task-1 pure test as the automated gate (do not fake an assertion).

- [ ] **Step 5: Build the overlay binary + full tests + clippy.**
```
distrobox enter claude_development -- bash -lc "cd $PWD && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build -p oxide-freya && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo clippy -p oxide-freya"
```
Expected: binary builds; tests pass; clippy clean.

- [ ] **Step 6: Commit.**
```bash
git add oxide-app/crates/oxide-freya/src/app.rs
git commit -m "feat(shell): drawer overlay for narrow + expanded; auto-collapse on blur"
```

- [ ] **Step 7: Controller live-verify (not a subagent step).** Below 1180: » on a rail floats the
  drawer over content (content doesn't shift); click outside the drawer collapses it; Escape collapses;
  resize across 1180 auto-collapses on shrink and re-expands the left panel on grow (right stays
  collapsed); wide-width behavior unchanged; the right context panel behaves symmetrically.

---

## Self-Review

**Spec coverage:** sole-truth + edge effect + `crossing_collapse` (Task 1); narrow rail+drawer for both
panels + backdrop outside-press/Escape collapse (Task 2). Both spec sections + the layout table mapped.

**Placeholder scan:** the only deliberately-conditional spots are the Theme-accessor idiom, the
`Position::right` vs computed-left fallback, and the snapshot-test downgrade clause — each carries a
concrete instruction + fallback, not a vague TODO. No "handle errors"/"etc." steps.

**Type consistency:** `crossing_collapse(bool,bool,bool) -> Option<bool>` used identically in the
Task-1 effect (twice, with `false`/`true` wide-defaults) and tested in Step 1. `Sidebar`/`ContextRegion`
both take `{ state, collapsed: bool }`. `narrow`/`sidebar_collapsed`/`context_collapsed` defined in
Task 1 Step 5 and consumed in Task 2. `Size::window_percent`, `Position::new_global`, `Theme::with_alpha`
match the just-shipped `confirm_dialog.rs`.
