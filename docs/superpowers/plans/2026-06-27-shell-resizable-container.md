# Shell on `ResizableContainer` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the shell's left/center/right on Freya 0.4 `ResizableContainer` + `ResizablePanel` so the container owns width distribution (fixing the collapse-squeeze bug where the right panel slivers when expanded + the left collapses), and gain drag-to-resize.

**Architecture:** `shell()` wraps the three regions in a `ResizableContainer` (Horizontal). Each region is a `ResizablePanel` with `PanelSize::px`/`percent` + `min_size`; collapse is an INSTANT size change via **re-keying** the panel with a new `initial_size` (no custom controller). The per-panel hand-rolled width `use_animation`s and `CollapsiblePanel` are removed; panels fill their container-owned width, and the `collapsed` prop only switches CONTENT (full vs rail).

**Tech Stack:** Rust, Freya blog/0.4 (`ResizableContainer`/`ResizablePanel`/`PanelSize`/`ResizableHandle` — auto-inserted), `freya_testing`.

## Global Constraints

- Freya blog/0.4 builder API. `ResizableContainer::new().direction(Direction::Horizontal).panel(p0).panel(p1).panel(p2)` auto-inserts a `ResizableHandle` between panels (do NOT add handles manually). `ResizablePanel::new(PanelSize::px(v)|PanelSize::percent(v)).min_size(f32).key(DiffKey).child(elem)`. The panel registers `initial_size` via `use_hook` (once) — change size by RE-KEYING the panel. Verify exact names/paths against `freya-blog04/crates/freya-components/src/resizable_container.rs`; they re-export through `freya::prelude`.
- Collapse is INSTANT (re-key). Animated collapse (controller-size tween) is OUT of this slice.
- Effective collapsed = `*user_signal.read() || size_class.is_compact_or_narrower()`, computed in `shell()` (the size-class probe stays). Side panels: `px(60)` when effective-collapsed, `px(274)` (sidebar) / `px(348)` (context) when not. `min_size(60.)` on side panels, `min_size(320.)` on center.
- Side panels FILL their `ResizablePanel` (`width(Size::fill()).height(Size::fill())`); the container sets the width. The `collapsed` prop switches content (full vs rail) only — NO `use_animation`/`anim_w` for width.
- `OxideContextMenuViewer`, the size-class probe, and the connection banner stay mounted (overlay/global or above the container) — they are NOT panels in the container.
- Match hand-formatting; no repo-wide `cargo fmt`; `cargo clippy -p oxide-ui -p oxide-freya` clean; no gold-plating.
- Dark snapshots (`use_init_theme(dark_theme)` + `bg_deep()` root); controller reads every PNG. The KEY snapshot is the **regression case**: right panel expanded (`context_collapsed=false`) + left sidebar collapsed (`sidebar_collapsed=true`) → right panel stays 348px (NOT a sliver), left is the 60px rail.
- **Update the spec** (`docs/superpowers/specs/2026-06-27-shell-resizable-container-design.md`) in Task 1 with what the spike reveals about the real API (exact key/PanelSize/handle behavior, whether re-key resizes cleanly, any deviation).

**Run every cargo command as:**
```
distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-rightpanel/oxide-app && CARGO_TARGET_DIR=/run/media/system/fastdrive/oxidemx-blog04-target LIBRARY_PATH=/tmp/oxidemx-lib-links cargo <args>'
```

---

### Task 1: Spike + convert the shell to `ResizableContainer`

Validate the approach with a throwaway PoC first, update the spec, THEN convert the real regions. This is the cohesive change that fixes the bug (container + both side panels filling must land together).

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/app.rs` (`shell()` + `snapshot_shell_app` + any other harness)
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs` (drop width animation; fill panel)
- Modify: `oxide-app/crates/oxide-freya/src/regions/context/mod.rs` (drop `anim_w`; fill panel)
- Modify (spec): `docs/superpowers/specs/2026-06-27-shell-resizable-container-design.md`

**Interfaces:**
- Consumes: `AppState` (`sidebar_collapsed`/`context_collapsed`/`size_class`), `Sidebar { state, collapsed }`, `ContextRegion { state, collapsed }`, `MainRegion { state }`.
- Produces: `shell()` rendering a `ResizableContainer` of 3 panels; `Sidebar`/`ContextRegion` fill their panel (no internal width animation).

- [ ] **Step 1: Spike PoC — validate `ResizableContainer` + re-key collapse.** In a scratch `#[ignore]` snapshot test in `app.rs` (name it `spike_resizable`), build a throwaway:
```rust
fn spike_app() -> Element {
    use_init_theme(dark_theme);
    let collapsed = use_state(|| false); // toggle in two render variants
    ResizableContainer::new()
        .direction(Direction::Horizontal)
        .panel(ResizablePanel::new(PanelSize::px(if *collapsed.read() {60.} else {274.}))
            .min_size(60.).key(if *collapsed.read() {1usize} else {0usize})
            .child(rect().width(Size::fill()).height(Size::fill()).background((40,120,200))))
        .panel(ResizablePanel::new(PanelSize::percent(100.)).min_size(320.)
            .child(rect().width(Size::fill()).height(Size::fill()).background((20,22,28))))
        .panel(ResizablePanel::new(PanelSize::px(348.)).min_size(60.)
            .child(rect().width(Size::fill()).height(Size::fill()).background((200,80,80))))
        .into()
}
```
Render it at 1200×800 to `/tmp/spike-resizable.png`. **The implementer must verify the exact API against `freya-blog04/crates/freya-components/src/resizable_container.rs`** — the `.key(...)` accepts `impl Into<DiffKey>` (use `1usize`/`0usize` or `DiffKey::from(...)` as the source requires); `PanelSize::px`/`percent` and `ResizablePanel::new`/`min_size`/`child`/`panel` are as named there. If `ResizableContainer`/`ResizablePanel`/`PanelSize` aren't in `freya::prelude`, import from `freya::components` (check the re-export).

- [ ] **Step 2: Render the spike + READ it.** Run `cargo test -p oxide-freya spike_resizable -- --ignored`. Controller reads `/tmp/spike-resizable.png`: a 274px blue panel + flex center + 348px red panel, with handles. Render a second variant with the left panel keyed-collapsed (60px) to confirm re-key resizes it. **If re-key does NOT cleanly resize** (e.g. stale size), STOP and report — the spec's controller approach is the fallback (provide a `controller: Writable<ResizableContext>` and write `panels()[i].size`); update the plan before proceeding.

- [ ] **Step 3: Update the spec** with the spike findings — exact API used (`PanelSize`, `.key`, import path), whether re-key resizes cleanly (or the controller fallback was needed), handle behavior, any deviation from the design. Commit the spec update with Step 8.

- [ ] **Step 4: Convert `Sidebar`** (`sidebar.rs`) — remove the width `use_animation` + `CollapsiblePanel`. The render returns the full column OR the rail, each `width(Size::fill()).height(Size::fill())`, chosen by `self.collapsed`. Keep the `«`/`»`/rail buttons writing `state.sidebar_collapsed`. Concretely: replace the `use_animation`/`anim_w`/`CollapsiblePanel::new()...override_width(...)` tail with:
```rust
        if self.collapsed {
            rail.into_element()   // the existing 60px icon rail, but its root rect uses Size::fill()
        } else {
            col.into_element()    // the existing full column, root rect Size::fill()
        }
```
(Ensure `col` and `rail` root rects use `.width(Size::fill()).height(Size::fill())` so they fill the panel; remove the `SIDEBAR_FULL_W`/`SIDEBAR_RAIL_W` width plumbing and the `freya::animation` import if now unused.)

- [ ] **Step 5: Convert `ContextRegion`** (`context/mod.rs`) — remove `width_anim`/`anim_w`/`OnCreation`/the `freya::animation` import. Both the rail (collapsed) and the full panel (expanded) root rects use `.width(Size::fill()).height(Size::fill())` instead of `Size::px(anim_w)`. Everything else (tab header, ScrollView body, rail nav, `»`) unchanged.

- [ ] **Step 6: Rewrite `shell()`** (`app.rs`) — wrap the 3 regions in a `ResizableContainer`. The root becomes a vertical stack holding the overlays + the container:
```rust
    rect()
        .direction(Direction::Vertical)
        .expanded()
        .background((5u8, 7u8, 11u8))
        .child(/* the existing global size-probe rect, unchanged */)
        .child(OxideContextMenuViewer::new())
        .maybe_child(connection_banner(conn))
        .child(
            ResizableContainer::new()
                .direction(Direction::Horizontal)
                .panel(
                    ResizablePanel::new(PanelSize::px(if sidebar_collapsed { 60. } else { 274. }))
                        .min_size(60.)
                        .key(if sidebar_collapsed { 1usize } else { 0usize })
                        .child(Sidebar { state: state.clone(), collapsed: sidebar_collapsed }),
                )
                .panel(
                    ResizablePanel::new(PanelSize::percent(100.))
                        .min_size(320.)
                        .child(MainRegion { state: state.clone() }),
                )
                .panel(
                    ResizablePanel::new(PanelSize::px(if context_collapsed { 60. } else { 348. }))
                        .min_size(60.)
                        .key(if context_collapsed { 1usize } else { 0usize })
                        .child(ContextRegion { state: state.clone(), collapsed: context_collapsed }),
                ),
        )
```
Update the `snapshot_shell_app` harness (and any other shell harness) the same way (it computes its own effective-collapsed already). The `MainRegion` wrapper rect with `Size::flex(1.0)` is no longer needed — `MainRegion` goes directly in the percent panel.

- [ ] **Step 7: Build + test + the regression snapshot.** Run `cargo test -p oxide-ui -p oxide-freya` (all pass) + `cargo clippy -p oxide-ui -p oxide-freya` (clean). Add/refresh a `snapshot_shell_regression` test: render `snapshot_shell_app` (or a copy) with `context_collapsed=false` (right expanded) AND `sidebar_collapsed=true` (left rail) → `/tmp/oxide-shell-regression.png`. Also refresh `/tmp/oxide-shell-expanded.png` (Wide) + `/tmp/oxide-shell-compact.png`. Controller reads all three.

- [ ] **Step 8: Commit** (incl. the spec update + remove the spike test or leave it `#[ignore]` with a note):
```bash
git add oxide-app/crates/oxide-freya/src docs/superpowers/specs/2026-06-27-shell-resizable-container-design.md
git commit -m "feat(shell): rebuild on ResizableContainer (fix collapse-squeeze; re-key collapse; drag-resize)"
```

---

### Task 2: Retire `CollapsiblePanel`

**Files:**
- Delete: `oxide-app/crates/oxide-ui/src/components/collapsible_panel.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/mod.rs` (drop the `mod`/`pub use`)

**Interfaces:** Consumes nothing; removes the now-unused component.

- [ ] **Step 1: Confirm no consumers remain.** Run `grep -rn "CollapsiblePanel" oxide-app/crates/` — after Task 1, only `collapsible_panel.rs` + `mod.rs` should match. If `sidebar.rs` still references it, Task 1 is incomplete — STOP and report.

- [ ] **Step 2: Delete + de-register.**
```bash
git rm oxide-app/crates/oxide-ui/src/components/collapsible_panel.rs
```
In `oxide-ui/src/components/mod.rs`, remove the `mod collapsible_panel;` and the `pub use collapsible_panel::CollapsiblePanel;` lines.

- [ ] **Step 3: Build + clippy.** `cargo test -p oxide-ui -p oxide-freya` (pass) + `cargo clippy -p oxide-ui -p oxide-freya` (clean, no unused-import fallout).

- [ ] **Step 4: Commit.**
```bash
git add -A oxide-app/crates/oxide-ui
git commit -m "chore(oxide-ui): retire CollapsiblePanel (replaced by ResizableContainer)"
```

---

### Task 3: Final snapshots + verify

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/app.rs` (snapshot tests, if more coverage needed)

- [ ] **Step 1: Drag-resize snapshot (best-effort).** If `freya_testing` can simulate a handle drag (`move_cursor`/`click_cursor`/drag on the handle x-position between panels), add a `snapshot_shell_dragged` that drags the left handle right ~80px and renders `/tmp/oxide-shell-dragged.png`. If a reliable drag simulation isn't available in this `freya_testing`, SKIP this snapshot and note it in the report (live drag is a manual check) — do NOT fake it.

- [ ] **Step 2: Build the binary + full suite + clippy.**
```
cargo build -p oxide-freya --bin oxide-freya   # Finished
cargo test -p oxide-ui -p oxide-freya          # all pass
cargo clippy -p oxide-ui -p oxide-freya        # clean (note pre-existing main_region.rs test warnings)
```

- [ ] **Step 3: Render + read all shell snapshots.** `cargo test -p oxide-freya snapshot_shell -- --ignored` → `/tmp/oxide-shell-expanded.png` (Wide), `/tmp/oxide-shell-compact.png` (Compact), `/tmp/oxide-shell-regression.png` (right expanded + left rail). Controller confirms: Wide = full sidebar + chat + right rail; Compact = both 60px rails; Regression = right panel a proper 348px (NOT a sliver) + left 60px rail.

- [ ] **Step 4: Commit.**
```bash
git add oxide-app/crates/oxide-freya/src/app.rs
git commit -m "test(shell): regression + drag snapshots for ResizableContainer layout"
```

---

## Self-Review notes

- **Spec coverage:** ResizableContainer shell (T1 Step 6) ✓; re-key collapse (T1) ✓; side panels fill + drop width animation (T1 Steps 4-5) ✓; probe/menu/banner preserved (T1 Step 6) ✓; retire CollapsiblePanel (T2) ✓; regression + Wide/Compact + drag snapshots (T1 Step 7, T3) ✓; spec-update-as-we-go (T1 Step 3) ✓; instant collapse (Global Constraints) ✓.
- **Spike-first de-risk:** T1 Steps 1-3 validate the API (re-key resize, imports, handles) on a throwaway BEFORE converting real regions; explicit STOP-and-report fallback to the `controller`-driven size approach if re-key fails.
- **Type consistency:** `Sidebar { state, collapsed }` / `ContextRegion { state, collapsed }` unchanged (the `collapsed` prop now only switches content); `PanelSize::px/percent`, `ResizablePanel::new/min_size/key/child`, `ResizableContainer::new/direction/panel` used identically across steps.
- **Freya API caveat:** exact `ResizableContainer`/`ResizablePanel`/`PanelSize`/`DiffKey` import paths + the `.key()` arg type are verified against the freya-blog04 source in T1 Step 1 (the plan's `1usize`/`0usize` key is a starting point; adapt to the real `Into<DiffKey>`).
- **Deferred:** animated collapse (controller tween); full DockingArea; the later right-panel slices.
