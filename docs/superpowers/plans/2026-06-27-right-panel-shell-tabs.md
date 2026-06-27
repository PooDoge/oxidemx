# Right-Panel Shell + 3 Tabs + Responsive Breakpoints — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the chat shell's right panel to match the Claude Design "Collapsible Panels" mockup — a resizable 348/60 control panel with Run/Worktree/.oxide tabs — and make the shell responsive (force both side panels to icon rails when the window is `compact`).

**Architecture:** `ContextRegion` (now `regions/context/mod.rs`) renders a `ResizableContainer` that is either a 60px rail (nav `RailButton`s) or a 348px full panel (`SegmentedButton` tabs over a `ScrollView` body). A `RightTab` signal selects the body; three tab-body components live in `regions/context/{run,worktree,settings}.rs`. `shell()` measures the logical window width via a full-window probe, derives a `SizeClass`, and passes an **effective** `collapsed: bool` (= user signal OR compact) to both side panels.

**Tech Stack:** Rust, Freya v0.4.0-rc.23 (`SegmentedButton`/`ButtonSegment`, `ScrollView`, `Card`, `Chip`, `use_animation`, `freya_testing`), `oxide-ui` Theme tokens, the `claude-design-to-freya` skill.

## Global Constraints

- Freya v0.4.0-rc.23 builder API (`rect().child()`, never `rsx!`); reuse Freya built-ins (`SegmentedButton`, `ButtonSegment`, `ScrollView`, `Card`, `Chip`) over hand-rolling (Rule 4).
- **Pixel-faithful translation:** use the `claude-design-to-freya` skill + `OxideMX Freya - Collapsible Panels.freya.json` (Claude Design project `686a723e-0412-4e94-870e-b4e32ae465f2`) as the authoring contract; source structure is `freya2-right.jsx` + `freya2-shell.jsx`. Token map (design→Theme), verbatim from the spec: `T.mantle`→`panel()`, `T.crust`→`bg_deep()`, `T.hair`→`hairline()`, `T.hairStrong`→`hairline_strong()`, `T.surface0/1/2`→`surface()/surface_hi()/surface_max()`, `T.text`→`text()`, `T.subtext1`→`subtext_hi()`, `T.subtext0`→`subtext()`, `T.faint`→`faint()`, `T.accent`→`accent()`, `T.accentDim`→`accent_dim()`, `T.green/yellow/red/blue/mauve`→ same-named, `T.<tone>_NN`→`Theme::with_alpha(<tone>(), 0xNN)`. Source chips: inherited=`blue`, local=`green`, merged=`mauve`.
- Widths: `CONTEXT_FULL_W = 348.0`, `CONTEXT_RAIL_W = 60.0`. Collapse anim ~340ms ease-out (`OnCreation::Finish`, width animation scoped to the collapse signal read INSIDE the `use_animation` closure — `[[feedback_freya_use_animation_scoping]]`).
- `SizeClass` thresholds: `≥1180 Wide` / `≥920 Compact` / `≥600 Tablet` / else `Phone`. `is_compact_or_narrower()` = not Wide.
- Match surrounding hand-formatting; NO repo-wide `cargo fmt`. `cargo clippy` clean (warnings = defects). No gold-plating.
- Every `freya_testing` snapshot uses `use_init_theme(dark_theme)` + a `rect().background(Theme::default().bg_deep())` root; after `render_to_file`, the controller Reads the PNG before trusting it. Snapshots that contain the collapse animation `poll` past 340ms before `render_to_file`.
- All `oxide-app` cargo runs inside the `claude_development` distrobox with `LIBRARY_PATH=/tmp/oxidemx-lib-links`; `oxide-app/target` on disk. Work on a NEW branch off `2b-collapsible-panels`.
- DTOs: `Conversation { worktree: Option<Worktree{path,branch,base_ref}>, working_dir, title, model }`.

**Run every cargo command as:**
```
distrobox enter claude_development -- bash -lc 'cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-2b/oxide-app && LIBRARY_PATH=/tmp/oxidemx-lib-links cargo <args>'
```

---

### Task 1: `RightTab` + `SizeClass` state

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/state.rs`

**Interfaces:**
- Produces:
  - `pub enum RightTab { Run, Worktree, Settings }` — `#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]`, `#[default] Run`.
  - `pub enum SizeClass { Wide, Compact, Tablet, Phone }` — same derives, `#[default] Wide`; `pub fn from_logical_width(w: f32) -> SizeClass`; `pub fn is_compact_or_narrower(self) -> bool`.
  - `AppState` fields `pub right_tab: State<RightTab>`, `pub size_class: State<SizeClass>` (in `new` + `PartialEq`).

- [ ] **Step 1: Write the failing test** (append to `state.rs` `mod tests`)

```rust
#[test]
fn size_class_thresholds_and_right_tab_default() {
    use super::{SizeClass, RightTab};
    assert_eq!(SizeClass::from_logical_width(1280.0), SizeClass::Wide);
    assert_eq!(SizeClass::from_logical_width(1000.0), SizeClass::Compact);
    assert_eq!(SizeClass::from_logical_width(700.0), SizeClass::Tablet);
    assert_eq!(SizeClass::from_logical_width(420.0), SizeClass::Phone);
    // boundaries (inclusive lower)
    assert_eq!(SizeClass::from_logical_width(1180.0), SizeClass::Wide);
    assert_eq!(SizeClass::from_logical_width(920.0), SizeClass::Compact);
    assert!(!SizeClass::Wide.is_compact_or_narrower());
    assert!(SizeClass::Compact.is_compact_or_narrower());
    assert!(SizeClass::Phone.is_compact_or_narrower());
    assert_eq!(RightTab::default(), RightTab::Run);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p oxide-freya size_class_thresholds`
Expected: FAIL — `cannot find type SizeClass`.

- [ ] **Step 3: Add the enums** (top of `state.rs`, after the `StatusDirection` enum)

```rust
/// Which right-panel tab is active (Run / Worktree / .oxide settings).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RightTab {
    #[default]
    Run,
    Worktree,
    Settings,
}

/// Responsive size class derived from the LOGICAL window width (design's four
/// breakpoints). `Compact` and narrower force both side panels to icon rails.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SizeClass {
    #[default]
    Wide,
    Compact,
    Tablet,
    Phone,
}

impl SizeClass {
    pub fn from_logical_width(w: f32) -> SizeClass {
        if w >= 1180.0 {
            SizeClass::Wide
        } else if w >= 920.0 {
            SizeClass::Compact
        } else if w >= 600.0 {
            SizeClass::Tablet
        } else {
            SizeClass::Phone
        }
    }

    pub fn is_compact_or_narrower(self) -> bool {
        !matches!(self, SizeClass::Wide)
    }
}
```

- [ ] **Step 4: Add the signals** — in `struct AppState` after `active_direction`:

```rust
    pub right_tab: State<RightTab>,
    pub size_class: State<SizeClass>,
```

In `PartialEq::eq`, extend the `&&` chain:

```rust
            && self.right_tab == other.right_tab
            && self.size_class == other.size_class
```

In `new`, after `active_direction: use_state(StatusDirection::default),`:

```rust
            right_tab: use_state(RightTab::default),
            size_class: use_state(SizeClass::default),
```

- [ ] **Step 5: Run tests + clippy**

Run: `cargo test -p oxide-freya size_class_thresholds` → PASS.
Run: `cargo test -p oxide-freya` → all PASS. `cargo clippy -p oxide-freya` → clean.

- [ ] **Step 6: Commit**

```bash
git add oxide-app/crates/oxide-freya/src/state.rs
git commit -m "feat(state): RightTab + SizeClass enums + signals"
```

---

### Task 2: Module restructure + three tab-body components

Move `context.rs` → `context/mod.rs` (behavior unchanged so it keeps compiling), then add the three standalone tab-body components the rewrite (Task 3) will mount. Each renders available/placeholder data per the spec.

**Files:**
- Move: `oxide-app/crates/oxide-freya/src/regions/context.rs` → `oxide-app/crates/oxide-freya/src/regions/context/mod.rs`
- Create: `oxide-app/crates/oxide-freya/src/regions/context/{run,worktree,settings}.rs`

**Interfaces:**
- Consumes: `AppState` (active conversation worktree), Theme tokens.
- Produces: `RunTab { pub state: AppState }`, `WorktreeTab { pub state: AppState }`, `SettingsTab { pub state: AppState }` — each `#[derive(PartialEq, Clone)]` + `impl Component`.

- [ ] **Step 1: Move the file** (keeps the OLD `ContextRegion` compiling; Task 3 rewrites it)

```bash
cd /run/media/system/fastdrive/Games/mx-master-4-linux/oxidemx-2b
git mv oxide-app/crates/oxide-freya/src/regions/context.rs oxide-app/crates/oxide-freya/src/regions/context/mod.rs
```
Add to the TOP of `context/mod.rs` (module declarations):
```rust
mod run;
mod worktree;
mod settings;
```
(`regions/mod.rs` already has `pub mod context;` — a `context/mod.rs` satisfies it unchanged.)

- [ ] **Step 2: Run-tab body** — create `regions/context/run.rs`

```rust
//! Run tab — live flow-run stages. Slice 1: empty-state only (real conductor
//! run state lands in the Run-data follow-on slice).
use freya::prelude::*;
use oxide_ui::Theme;

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct RunTab {
    pub state: AppState,
}

impl Component for RunTab {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        rect()
            .direction(Direction::Vertical)
            .spacing(8.)
            .padding(Gaps::new_all(14.))
            .child(section_label(th, "Run"))
            .child(label().text("No active run").font_size(12.5).color(th.subtext()))
            .child(label().text("flow stages coming").font_size(10.5).color(th.faint()))
    }
}

/// Design's uppercase section label (faint, letter-spaced).
pub(super) fn section_label(th: Theme, t: &str) -> impl IntoElement {
    label().text(t.to_uppercase()).font_size(10.).color(th.faint())
}
```

- [ ] **Step 3: Worktree-tab body** — create `regions/context/worktree.rs`

```rust
//! Worktree tab — the active conversation's worktree + (later) changed files.
use freya::prelude::*;
use oxide_ui::Theme;

use crate::state::AppState;
use super::run::section_label;

#[derive(PartialEq, Clone)]
pub struct WorktreeTab {
    pub state: AppState,
}

impl Component for WorktreeTab {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let active = self.state.active.read().clone();
        let convs = self.state.conversations.read().clone();
        let conv = active.as_ref().and_then(|id| convs.iter().find(|c| &c.id == id));
        let wt = conv
            .and_then(|c| c.worktree.as_ref())
            .map(|w| format!("{} @ {}", w.branch, w.path))
            .unwrap_or_else(|| "no worktree".into());
        rect()
            .direction(Direction::Vertical)
            .spacing(8.)
            .padding(Gaps::new_all(14.))
            .child(section_label(th, "Worktree"))
            .child(label().text(wt).font_size(12.).color(th.text()))
            .child(label().text("changed files coming").font_size(10.5).color(th.faint()))
    }
}
```

- [ ] **Step 4: Settings-tab body** — create `regions/context/settings.rs`

```rust
//! .oxide settings tab — resolved config nav. Slice 1: placeholder category
//! rows with inheritance-source chips (real .oxide resolution + Hooks/MCP
//! editors land in the settings follow-on slice). Rows inert this slice.
use freya::prelude::*;
use oxide_ui::Theme;

use crate::state::AppState;
use super::run::section_label;

#[derive(PartialEq, Clone)]
pub struct SettingsTab {
    pub state: AppState,
}

/// (label, count text, source) — source: "inherited" | "local" | "merged".
const NAV: [(&str, &str, &str); 4] = [
    ("Hooks", "3 hooks", "local"),
    ("MCP servers", "2 servers", "inherited"),
    ("Permissions", "allow / ask / deny", "merged"),
    ("Environment", "4 vars", "inherited"),
];

fn source_color(th: Theme, source: &str) -> Color {
    match source {
        "local" => th.green(),
        "merged" => th.mauve(),
        _ => th.blue(),
    }
}

impl Component for SettingsTab {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let mut col = rect()
            .direction(Direction::Vertical)
            .spacing(6.)
            .padding(Gaps::new_all(14.))
            .child(section_label(th, "Resolved .oxide"));
        for (lbl, count, source) in NAV {
            let c = source_color(th, source);
            col = col.child(
                rect()
                    .direction(Direction::Horizontal)
                    .cross_align(Alignment::Center)
                    .spacing(11.)
                    .width(Size::fill())
                    .padding(Gaps::new(10., 11., 10., 11.))
                    .corner_radius(CornerRadius::new_all(9.))
                    .background(th.surface())
                    .border(Border::new().fill(th.hairline()).width(1.))
                    .child(
                        rect()
                            .direction(Direction::Vertical)
                            .width(Size::fill())
                            .child(label().text(lbl).font_size(13.).color(th.text()))
                            .child(label().text(count).font_size(10.5).color(th.subtext())),
                    )
                    .child(
                        rect()
                            .padding(Gaps::new(1., 6., 1., 6.))
                            .corner_radius(CornerRadius::new_all(999.))
                            .background(Theme::with_alpha(c, 0x16))
                            .border(Border::new().fill(Theme::with_alpha(c, 0x3a)).width(1.))
                            .child(label().text(source).font_size(9.).color(c)),
                    ),
            );
        }
        col
    }
}
```

- [ ] **Step 5: Build (the OLD ContextRegion still renders; tab bodies exist but unused → allow dead_code if clippy flags)**

Run: `cargo test -p oxide-freya` → PASS (existing tests; the new tab-body types compile).
If clippy flags the unused tab types, prefix the `mod run/worktree/settings;` with `#[allow(dead_code)]` on the structs is NOT needed because Task 3 consumes them next — but if this task is reviewed standalone, add `#[allow(dead_code)]` on the three structs and REMOVE it in Task 3. `cargo clippy -p oxide-freya` → clean.

- [ ] **Step 6: Snapshot the three tab bodies + READ them**

Add a snapshot test (in `context/mod.rs` test module or a new `context/tests`) rendering each tab body at 348×600 on a dark `bg_deep` root (use the `make_rich_state` pattern already in the file for a seeded conversation). `render_to_file` to `/tmp/right-tab-{run,worktree,settings}.png`. You CANNOT view images — render without panic + report the paths; the controller reads them.

Run: `cargo test -p oxide-freya right_tab -- --ignored` (if `#[ignore]`).

- [ ] **Step 7: Commit**

```bash
git add oxide-app/crates/oxide-freya/src/regions/context/
git commit -m "feat(context): move context.rs to module dir + Run/Worktree/Settings tab bodies"
```

---

### Task 3: `ContextRegion` rewrite — ResizableContainer + tabs + rail

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/regions/context/mod.rs` (full rewrite of `ContextRegion`)
- Delete: `oxide-app/crates/oxide-freya/src/regions/directions/` (whole dir)
- Modify: `oxide-app/crates/oxide-freya/src/regions/mod.rs` (drop `pub mod directions;`)
- Modify: `oxide-app/crates/oxide-freya/src/app.rs` (pass `collapsed` to `ContextRegion`)

**Interfaces:**
- Consumes: `RightTab` (Task 1), `RunTab`/`WorktreeTab`/`SettingsTab` (Task 2), `oxide_ui::components::RailButton`, Freya `SegmentedButton`/`ButtonSegment`/`ScrollView`.
- Produces: `ContextRegion { pub state: AppState, pub collapsed: bool }` — `collapsed` is the EFFECTIVE value (render rail vs full); the panel's own collapse/expand buttons write `state.context_collapsed`.

- [ ] **Step 1: Delete the wrong directions module**

```bash
git rm -r oxide-app/crates/oxide-freya/src/regions/directions
```
In `regions/mod.rs`, remove the line `pub mod directions;`.

- [ ] **Step 2: Rewrite `ContextRegion`** in `context/mod.rs` (replace the whole `impl Component for ContextRegion` + struct; keep the `mod run/worktree/settings;` lines + the existing snapshot test helpers). Add the `collapsed` field.

```rust
use freya::animation::*;
use freya::prelude::*;
use oxide_ui::{Theme, components::RailButton};

use crate::state::{AppState, RightTab};

mod run;
mod worktree;
mod settings;

const CONTEXT_FULL_W: f32 = 348.0;
const CONTEXT_RAIL_W: f32 = 60.0;

/// (RightTab, rail icon glyph, label) — drives the rail nav buttons + tab header.
const TABS: [(RightTab, &str, &str); 3] = [
    (RightTab::Run, "‣", "Run"),
    (RightTab::Worktree, "⌥", "Worktree"),
    (RightTab::Settings, "⚙", ".oxide"),
];

#[derive(PartialEq, Clone)]
pub struct ContextRegion {
    pub state: AppState,
    /// Effective collapsed (user signal OR compact size class), from `shell()`.
    pub collapsed: bool,
}

impl Component for ContextRegion {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let state = self.state.clone();
        let collapsed = self.collapsed;
        let active_tab = *state.right_tab.read();

        // Width tween 348 <-> 60, scoped to the effective collapsed value,
        // settling on mount (OnCreation::Finish).
        let is_collapsed = collapsed;
        let width_anim = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            conf.on_creation(OnCreation::Finish);
            let w = AnimNum::new(CONTEXT_FULL_W, CONTEXT_RAIL_W)
                .time(340)
                .ease(Ease::Out)
                .function(Function::Expo);
            if is_collapsed { w } else { w.into_reversed() }
        });
        let anim_w = width_anim.get().value();

        let body = rect()
            .width(Size::px(anim_w))
            .height(Size::fill())
            .background(th.panel())
            .border(Border::new().fill(th.hairline()).width(1.)); // left hairline per design

        if collapsed {
            // Rail: nav buttons that expand + select a tab, then Open-editor stub.
            let mut rail = rect()
                .direction(Direction::Vertical)
                .cross_align(Alignment::Center)
                .spacing(8.)
                .padding(Gaps::new_all(8.))
                .height(Size::fill())
                .main_align(Alignment::End);
            for (tab, glyph, lbl) in TABS {
                let mut rt = state.right_tab;
                let mut coll = state.context_collapsed;
                rail = rail.child(
                    RailButton::new(glyph.to_string()).on_press(move |_: Event<PressEventData>| {
                        rt.set(tab);
                        coll.set(false);
                    }),
                );
                let _ = lbl;
            }
            body.child(rail).into_element()
        } else {
            // Full: SegmentedButton header + ScrollView body + collapse toggle.
            let header = {
                let mut seg = SegmentedButton::new();
                for (tab, _glyph, lbl) in TABS {
                    let mut rt = state.right_tab;
                    let selected = tab == active_tab;
                    seg = seg.child(
                        ButtonSegment::new()
                            .selected(selected)
                            .on_press(move |_: Event<PressEventData>| rt.set(tab))
                            .child(label().text(lbl)),
                    );
                }
                seg
            };
            let mut coll = state.context_collapsed;
            body.direction(Direction::Vertical)
                .child(
                    rect()
                        .direction(Direction::Horizontal)
                        .cross_align(Alignment::Center)
                        .spacing(8.)
                        .padding(Gaps::new(10., 12., 10., 12.))
                        .border(Border::new().fill(th.hairline()).width(0.)) // bottom hairline below
                        .width(Size::fill())
                        .child(rect().width(Size::fill()).child(header))
                        .child(
                            RailButton::new("»".into())
                                .on_press(move |_: Event<PressEventData>| coll.set(true)),
                        ),
                )
                .child(
                    ScrollView::new().child(match active_tab {
                        RightTab::Run => run::RunTab { state: state.clone() }.into_element(),
                        RightTab::Worktree => worktree::WorktreeTab { state: state.clone() }.into_element(),
                        RightTab::Settings => settings::SettingsTab { state: state.clone() }.into_element(),
                    }),
                )
                .into_element()
        }
    }
}
```

(If `SegmentedButton`/`ButtonSegment`/`ScrollView` import paths or the exact `.child`/`.children` API differ from this skeleton, consult the Freya source `crates/freya-components/src/{segmented_button,scrollviews}.rs` and the `claude-design-to-freya` skill; the structure — three segments bound to `right_tab`, a scrollable body, a `»` collapse button — is the contract. Remove any `#[allow(dead_code)]` added on the tab structs in Task 2.)

- [ ] **Step 3: Update `app.rs` `ContextRegion` construction** — both the `shell()` site and the `snapshot_shell_app` harness site: add `collapsed`. For now pass the raw signal (Task 4 swaps to effective):

```rust
.child(ContextRegion { state: state.clone(), collapsed: *state.context_collapsed.read() })
```

- [ ] **Step 4: Build + the existing context snapshot tests**

Run: `cargo test -p oxide-freya` → PASS. `cargo clippy -p oxide-freya` → clean (no `directions` references remain).

- [ ] **Step 5: Snapshot the rail + full panel (each tab) + READ**

Update/extend the `context/mod.rs` snapshot tests: render the rail (collapsed=true) and the full panel with `right_tab` set to each of Run/Worktree/Settings (collapsed=false), `poll` past 340ms, `render_to_file` to `/tmp/right-{rail,run,worktree,settings}.png`. Controller reads them.

Run: `cargo test -p oxide-freya context -- --ignored`.

- [ ] **Step 6: Commit**

```bash
git add -A oxide-app/crates/oxide-freya/src/regions oxide-app/crates/oxide-freya/src/app.rs
git commit -m "feat(context): right-panel rewrite — ResizableContainer + Run/Worktree/.oxide tabs + rail nav; drop directions"
```

---

### Task 4: Responsive size-class probe + effective-collapsed

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/app.rs` (`shell()` + `snapshot_shell_app`)
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs` (re-add `collapsed` prop)

**Interfaces:**
- Consumes: `SizeClass` (Task 1), `ContextRegion { collapsed }` (Task 3).
- Produces: `Sidebar { pub state: AppState, pub collapsed: bool }` — `collapsed` = effective (render); its `«`/`»`/rail buttons still write `state.sidebar_collapsed`.

- [ ] **Step 1: Re-add the `collapsed` prop to `Sidebar`** (`sidebar.rs`). Add `pub collapsed: bool` to `struct Sidebar`. In `render`, the width animation + `CollapsiblePanel.collapsed(...)` use the EFFECTIVE prop, while the buttons keep writing the user signal:

```rust
        let user_collapsed = self.state.sidebar_collapsed; // buttons write this
        let mut c_collapse = user_collapsed;
        let mut c_expand = user_collapsed;
        let is_collapsed = self.collapsed; // EFFECTIVE (render)
        let collapsed_for_anim = self.collapsed;
        let width_anim = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            conf.on_creation(OnCreation::Finish);
            let w = AnimNum::new(SIDEBAR_FULL_W, SIDEBAR_RAIL_W)
                .time(340).ease(Ease::Out).function(Function::Expo);
            if collapsed_for_anim { w } else { w.into_reversed() }
        });
```
Replace later uses of the old `is_collapsed`/`*collapsed.read()` for RENDER with `self.collapsed`; keep `c_collapse.set(true)` / `c_expand.set(false)` writing the user signal. `.collapsed(is_collapsed)` on `CollapsiblePanel` now uses the effective value.

NOTE: the animation now reads `self.collapsed` (a plain bool prop, not a signal). That's fine — `shell()` re-renders `Sidebar` with a new `collapsed` whenever `size_class` or `sidebar_collapsed` changes, and `OnCreation::Finish` settles it; the tween still plays on the value change. (This matches how `ContextRegion` consumes its `collapsed` prop.)

- [ ] **Step 2: Add the size-class probe + effective-collapsed in `shell()`** (`app.rs`). At the top of `shell()` (after `let state = AppState::new(...)`):

```rust
    let mut size_class = state.size_class;
```
Add the probe as the FIRST child of the Horizontal root (a global full-window rect measuring LOGICAL width — `[[feedback_freya_logical_vs_physical_px]]`):
```rust
        .child(
            rect()
                .layer(Layer::Overlay)
                .position(Position::new_global().left(0.0).top(0.0))
                .width(Size::fill())
                .height(Size::fill())
                .opacity(0.0_f32)
                .on_sized(move |e: Event<SizedEventData>| {
                    size_class.set_if_modified(crate::state::SizeClass::from_logical_width(e.area.size.width));
                }),
        )
```
Compute effective-collapsed and pass to both panels:
```rust
    let sc = *state.size_class.read();
    let force_rail = sc.is_compact_or_narrower();
    let sidebar_collapsed = *state.sidebar_collapsed.read() || force_rail;
    let context_collapsed = *state.context_collapsed.read() || force_rail;
```
```rust
        .child(Sidebar { state: state.clone(), collapsed: sidebar_collapsed })
        ...
        .child(ContextRegion { state: state.clone(), collapsed: context_collapsed })
```
Update `snapshot_shell_app` the same way (it has its own `state`): compute `force_rail`/effective values + pass `collapsed` to `Sidebar` and `ContextRegion`. (The harness has a fixed window size, so `size_class` stays `Wide` unless the test sets it.)

- [ ] **Step 3: Build + tests**

Run: `cargo test -p oxide-freya` → PASS. `cargo clippy -p oxide-ui -p oxide-freya` → clean.

- [ ] **Step 4: Commit**

```bash
git add oxide-app/crates/oxide-freya/src/app.rs oxide-app/crates/oxide-freya/src/regions/sidebar.rs
git commit -m "feat(shell): logical-width size-class probe + compact forces icon rails (effective collapsed)"
```

---

### Task 5: Full-shell snapshots (Wide + Compact) + verify

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/app.rs` (snapshot tests)

- [ ] **Step 1: Wide + Compact full-shell snapshots.** Ensure `snapshot_shell` renders the Wide shell (existing, 1200px — note 1200 ≥ 1180 = Wide). Add a `snapshot_shell_compact` that forces `Compact`: in the harness, after building `state`, `state.size_class.set(SizeClass::Compact)` (so `force_rail` is true) OR render `snapshot_shell_app` at a 1000px window width and let the probe set it; render to `/tmp/oxide-shell-compact.png`, `poll` past 340ms. Controller reads both: Wide = sidebar full + right panel rail/expanded as set; Compact = BOTH side panels at 60px rails.

```rust
#[test]
#[ignore = "snapshot: writes PNG to /tmp for visual review"]
fn snapshot_shell_compact() {
    let (mut runner, _) =
        TestingRunner::new(snapshot_shell_compact_app, (1000., 800.).into(), |_| {}, 1.);
    runner.poll(Duration::from_millis(1), Duration::from_millis(400));
    runner.render_to_file("/tmp/oxide-shell-compact.png");
}
```
(`snapshot_shell_compact_app` = a copy of `snapshot_shell_app` whose probe will see ~1000px logical width → `Compact`; if the harness can't host the probe cleanly, set `state.size_class.set(SizeClass::Compact)` explicitly and assert the effective-collapsed path.)

- [ ] **Step 2: Build the binary + full suite + clippy**

Run: `cargo build -p oxide-freya --bin oxide-freya` → Finished.
Run: `cargo test -p oxide-ui -p oxide-freya` → all PASS.
Run: `cargo clippy -p oxide-ui -p oxide-freya` → clean (note any pre-existing `main_region.rs` warnings as not-this-slice).

- [ ] **Step 3: Render + read all snapshots.** `cargo test -p oxide-freya -- --ignored` to (re)render `/tmp/right-{rail,run,worktree,settings}.png`, `/tmp/oxide-shell-expanded.png` (Wide), `/tmp/oxide-shell-compact.png`. Controller reads each: tabs match the design; Compact shows both rails.

- [ ] **Step 4: Commit**

```bash
git add oxide-app/crates/oxide-freya/src/app.rs
git commit -m "test(shell): Wide + Compact full-shell snapshots"
```

---

## Self-Review notes

- **Spec coverage:** RightTab+SizeClass state (T1) ✓; tab bodies (T2) ✓; ResizableContainer + SegmentedButton tabs + rail nav + delete directions (T3) ✓; size-class probe + effective-collapsed + Sidebar collapsed prop (T4) ✓; Wide+Compact snapshots (T5) ✓. Pixel-fidelity via the token map (Global Constraints) + design-to-freya skill + snapshot read-back gates.
- **Type consistency:** `ContextRegion { state, collapsed }` and `Sidebar { state, collapsed }` both gain `collapsed: bool` (effective); `RightTab`/`SizeClass` used identically across tasks; tab structs `RunTab`/`WorktreeTab`/`SettingsTab { state }`; `section_label` shared from `run.rs`.
- **Freya API caveats flagged for the implementer:** the exact `SegmentedButton`/`ButtonSegment`/`ScrollView` builder + `render_to_file`/`poll` testing API may differ by rc — each task says to mirror the Freya source / an existing working snapshot rather than guess. Drag-to-resize is intentionally OUT (ResizeGrip is vertical-only); the `DragHandle` is the `»`/rail click-toggle.
- **Deferred (later slices, per spec):** rail agent-status styles + popovers; Run/Worktree/.oxide real backends; code editor; tablet/phone layouts.
