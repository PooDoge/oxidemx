# Chat-Shell Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the three-panel Freya chat shell fully interactive — working project switcher, re-render-safe collapse with an icon rail, a four-direction right panel, and the animations that tie them together.

**Architecture:** Lift shell state (current project, two collapse flags, active direction) onto the single `AppState` so it survives re-render and is shared; the project switcher is a Freya `Select`; the right panel is a rail + segmented expanded host dispatching to four per-direction body components. `oxide-ui` components stay free of `oxide-freya` types (it's the lower crate) — the switcher takes primitives + an `EventHandler`.

**Tech Stack:** Rust, Freya v0.4.0-rc.23 (`Select`, `MenuItem`, `use_animation`/`AnimNum`, `freya_testing`), `oxide-client` DTOs.

## Global Constraints

- Freya v0.4.0-rc.23 builder API (`rect().child()`, never `rsx!`); reuse Freya built-ins over hand-rolling.
- `oxide-ui` MUST NOT depend on `oxide-freya` (circular). `AppState`, `StatusDirection`, and `Project` stay out of `oxide-ui` component signatures — pass primitives (`String`, `Vec<(String,String)>`) + `EventHandler`.
- Name the four-direction enum `StatusDirection` (Freya already exports `Direction`).
- Match surrounding hand-formatting; do NOT run repo-wide `cargo fmt`. `cargo clippy` clean. No gold-plating.
- Every `freya_testing` snapshot uses `use_init_theme(dark_theme)` + a root `rect().background(Theme::default().bg_deep())`, else near-white text/panels render invisible. After `render_to_file`, **Read the PNG back** before claiming the visual is right and before any app relaunch.
- All `oxide-app` cargo commands run in the `claude_development` distrobox with `LIBRARY_PATH=/tmp/oxidemx-lib-links`; `oxide-app/target` is on disk. Worktree: `oxidemx-2b`, branch `2b-collapsible-panels`.
- Theme tokens (exact): `surface()` `surface_hi()` `panel()` `bg_deep()` `text()` `faint()` `accent()` `accent_hi()` `accent_dim()` `hairline_strong()`. Widths: `SIDEBAR_FULL_W = 274.0`, `SIDEBAR_RAIL_W = 60.0` (in `oxide-ui/src/tokens.rs`).
- DTOs (`oxide-client`): `Project { id: ProjectId, name, default_working_dir }`, `Conversation { id, project_id, title, working_dir, model, worktree: Option<Worktree{path,branch,base_ref}> }`, `ProjectId(pub String)` / `ConversationId(pub String)` with `as_str()`, `From<&str>`, `From<String>`.

---

### Task 1: AppState state model + `StatusDirection` + `open_project`

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/state.rs` (struct, `new`, `bootstrap`, `PartialEq`, add enum + method + tests)

**Interfaces:**
- Produces:
  - `pub enum StatusDirection { Spec, Mission, Workbench, Ambient }` with `pub const ALL: [StatusDirection; 4]`, `pub fn label(self) -> &'static str`, `pub fn icon(self) -> &'static str`, `#[derive(Clone, Copy, PartialEq, Eq, Debug)]` + `Default` (= `Spec`).
  - `AppState` new pub fields: `current_project: State<Option<ProjectId>>`, `sidebar_collapsed: State<bool>`, `context_collapsed: State<bool>`, `active_direction: State<StatusDirection>`.
  - `pub fn open_project(&self, id: ProjectId)` — sets `current_project = Some(id)`, `active = None`, spawns `list_conversations(id)` → `conversations`.

- [ ] **Step 1: Write the failing test** (append to `state.rs` `mod tests`)

```rust
#[test]
fn status_direction_all_has_four_with_labels_and_icons() {
    assert_eq!(StatusDirection::ALL.len(), 4);
    assert_eq!(StatusDirection::ALL[0], StatusDirection::Spec);
    let labels: Vec<_> = StatusDirection::ALL.iter().map(|d| d.label()).collect();
    assert_eq!(labels, ["Spec", "Mission", "Workbench", "Ambient"]);
    // Every direction has a non-empty icon glyph.
    assert!(StatusDirection::ALL.iter().all(|d| !d.icon().is_empty()));
    assert_eq!(StatusDirection::default(), StatusDirection::Spec);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya status_direction_all`
Expected: FAIL — `cannot find type StatusDirection`.

- [ ] **Step 3: Add the enum** (top of `state.rs`, after the `ConnState` enum)

```rust
// ── Status directions (right-panel facets) ──────────────────────────────────

/// The four right-panel "directions". Named `StatusDirection` to avoid colliding
/// with Freya's layout `Direction`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StatusDirection {
    #[default]
    Spec,
    Mission,
    Workbench,
    Ambient,
}

impl StatusDirection {
    pub const ALL: [StatusDirection; 4] =
        [Self::Spec, Self::Mission, Self::Workbench, Self::Ambient];

    pub fn label(self) -> &'static str {
        match self {
            Self::Spec => "Spec",
            Self::Mission => "Mission",
            Self::Workbench => "Workbench",
            Self::Ambient => "Ambient",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Spec => "📋",
            Self::Mission => "🎯",
            Self::Workbench => "🔧",
            Self::Ambient => "〰️",
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya status_direction_all`
Expected: PASS.

- [ ] **Step 5: Add the new signals + `open_project`, rewrite `bootstrap`**

Add the imports if missing: `use oxide_client::ProjectId;` (alongside the existing `oxide_client` import).

In `struct AppState`, add after `pub connection: State<ConnState>,`:

```rust
    pub current_project: State<Option<ProjectId>>,
    pub sidebar_collapsed: State<bool>,
    pub context_collapsed: State<bool>,
    pub active_direction: State<StatusDirection>,
```

In `PartialEq::eq`, add to the `&&` chain:

```rust
            && self.current_project == other.current_project
            && self.sidebar_collapsed == other.sidebar_collapsed
            && self.context_collapsed == other.context_collapsed
            && self.active_direction == other.active_direction
```

In `new`, add after `connection: use_state(|| ConnState::Unknown),`:

```rust
            current_project: use_state(|| None),
            sidebar_collapsed: use_state(|| false),
            context_collapsed: use_state(|| true),
            active_direction: use_state(StatusDirection::default),
```

Add the method (after `bootstrap`):

```rust
    /// Select a project: record it, clear the active conversation, and load the
    /// project's conversations. Transport errors leave `conversations` unchanged
    /// (same tolerant pattern as `bootstrap`).
    pub fn open_project(&self, id: ProjectId) {
        let mut current = self.current_project;
        let mut active = self.active;
        let mut conversations = self.conversations;
        current.set(Some(id.clone()));
        active.set(None);
        let t = self.transport.clone();
        spawn(async move {
            if let Ok(cs) = t.list_conversations(id.as_str()).await {
                conversations.set(cs);
            }
        });
    }
```

Rewrite the tail of `bootstrap` — replace the `if let Ok(cs) = t.list_conversations("personal").await { conversations.set(cs); }` block with project-seeded loading:

```rust
            let mut current_project = self.current_project;
            if let Ok(ps) = t.list_projects().await {
                let first = ps.first().map(|p| p.id.clone());
                projects.set(ps);
                let pid = first.unwrap_or_else(|| ProjectId::from("personal"));
                current_project.set(Some(pid.clone()));
                if let Ok(cs) = t.list_conversations(pid.as_str()).await {
                    conversations.set(cs);
                }
            }
```

Note: capture `let mut current_project = self.current_project;` in the `bootstrap` prelude alongside the existing `let mut projects = self.projects;` (move it out of the closure body if the borrow checker requires; `State` is `Copy`).

- [ ] **Step 6: Verify build + all state tests pass**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya`
Expected: PASS (existing transcript tests + the new direction test). `cargo clippy -p oxide-freya` clean.

- [ ] **Step 7: Commit**

```bash
git add oxide-app/crates/oxide-freya/src/state.rs
git commit -m "feat(state): shell signals + StatusDirection + open_project; project-seeded bootstrap"
```

---

### Task 2: Project switcher (Freya `Select`)

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/sidebar_header.rs` (rewrite switcher; new constructor)
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs` (build `SidebarHeader` from `AppState`)

**Interfaces:**
- Consumes: `AppState.projects`, `AppState.current_project`, `AppState::open_project` (Task 1).
- Produces: `SidebarHeader::new(projects: Vec<(String, String)>, current_id: String)` where each tuple is `(project_id, project_name)`; builder `.on_select(impl Into<EventHandler<String>>)` (emits the chosen `project_id`), `.theme(Theme)`.

- [ ] **Step 1: Write the failing test** (replace the body of `sidebar_header_renders_project_and_new`)

```rust
#[test]
fn sidebar_header_renders_project_and_new() {
    fn app() -> impl IntoElement {
        SidebarHeader::new(
            vec![("personal".into(), "oxidemx-phase1".into())],
            "personal".into(),
        )
    }
    let mut t = launch_test(app);
    t.sync_and_update();
    assert!(
        t.find(|_, el| Label::try_downcast(el)
            .filter(|l| l.text.as_ref().contains("oxidemx-phase1")))
            .is_some(),
        "SidebarHeader should render the current project name"
    );
    assert!(
        t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("New")))
            .is_some(),
        "SidebarHeader should render the '+ New' button label"
    );
    assert!(
        t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Search")))
            .is_some(),
        "search TextInput should render the 'Search…' placeholder"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui sidebar_header_renders`
Expected: FAIL — `new` takes one arg / arity mismatch.

- [ ] **Step 3: Rewrite `SidebarHeader`**

Replace the struct + impl head:

```rust
use crate::components::TextInput;
use crate::tokens::Theme;

/// Top-of-sidebar header: project switcher (Freya `Select`) + search + new button.
#[derive(PartialEq, Clone)]
pub struct SidebarHeader {
    /// (project_id, project_name) options.
    projects: Vec<(String, String)>,
    /// The currently-selected project id (used to resolve the displayed name).
    current_id: String,
    on_select: Option<EventHandler<String>>,
    theme: Theme,
}

impl SidebarHeader {
    pub fn new(projects: Vec<(String, String)>, current_id: String) -> Self {
        Self { projects, current_id, on_select: None, theme: Theme::default() }
    }

    pub fn on_select(mut self, h: impl Into<EventHandler<String>>) -> Self {
        self.on_select = Some(h.into());
        self
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }
}
```

Replace the `switcher` block in `render` with a `Select`. The `selected_item` is the existing pill row (accent dot + current name + chevron); options are one `MenuItem` per project:

```rust
        let th = self.theme;
        let value = use_state(String::new);

        // Resolve the displayed name from current_id (fallback: first project, else id).
        let current_name = self
            .projects
            .iter()
            .find(|(id, _)| id == &self.current_id)
            .map(|(_, name)| name.clone())
            .or_else(|| self.projects.first().map(|(_, n)| n.clone()))
            .unwrap_or_else(|| self.current_id.clone());

        let pill = rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.)
            .width(Size::fill())
            .padding(Gaps::new(8., 10., 8., 10.))
            .child(
                rect().width(Size::px(6.)).height(Size::px(6.))
                    .corner_radius(CornerRadius::new_all(3.)).background(th.accent()),
            )
            .child(
                label().text(current_name).font_size(12.5).color(th.text())
                    .width(Size::flex(1.0)),
            )
            .child(label().text("⌄").font_size(12.).color(th.faint()));

        let on_select = self.on_select;
        let current_id = self.current_id.clone();
        let options: Vec<Element> = self
            .projects
            .iter()
            .map(|(id, name)| {
                let id = id.clone();
                let selected = id == current_id;
                MenuItem::new()
                    .selected(selected)
                    .on_press(move |_: Event<PressEventData>| {
                        if let Some(h) = on_select {
                            h.call(id.clone());
                        }
                    })
                    .child(label().text(name.clone()).font_size(12.5))
                    .into()
            })
            .collect();

        let switcher = Select::new().selected_item(pill).children(options);
```

Keep the `search` + `new_btn` + the outer vertical `rect` exactly as they are (still `.child(switcher).child(search).child(new_btn)`).

- [ ] **Step 4: Run the test to verify it passes**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui sidebar_header_renders`
Expected: PASS.

- [ ] **Step 5: Wire `SidebarHeader` from `AppState`** in `sidebar.rs`

Replace `.child(SidebarHeader::new("oxidemx-phase1".into()).theme(th))` with:

```rust
            .child({
                let st = state.clone();
                let projects: Vec<(String, String)> = st
                    .projects
                    .read()
                    .iter()
                    .map(|p| (p.id.0.clone(), p.name.clone()))
                    .collect();
                let current_id = st
                    .current_project
                    .read()
                    .as_ref()
                    .map(|p| p.0.clone())
                    .unwrap_or_default();
                let on_st = state.clone();
                SidebarHeader::new(projects, current_id)
                    .on_select(move |id: String| on_st.open_project(id.into()))
                    .theme(th)
            });
```

- [ ] **Step 6: Snapshot the switcher (closed + open) and READ the PNGs**

Add to `sidebar_header.rs` tests a render-to-file (dark theme):

```rust
#[test]
fn snapshot_switcher_dark() {
    fn app() -> impl IntoElement {
        use_init_theme(dark_theme);
        rect().background(Theme::default().bg_deep()).padding(Gaps::new_all(16.)).child(
            SidebarHeader::new(
                vec![
                    ("personal".into(), "oxidemx-phase1".into()),
                    ("work".into(), "client-app".into()),
                ],
                "personal".into(),
            ),
        )
    }
    let mut t = launch_test_with_config(
        app,
        TestingConfig::<()>::default().with_size((300., 240.).into()),
    );
    t.sync_and_update();
    t.render_to_file("/tmp/shell-switcher-closed.png");
}
```

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui snapshot_switcher_dark`
Then **Read** `/tmp/shell-switcher-closed.png` — confirm the pill shows "oxidemx-phase1" + chevron on a dark surface. (If `launch_test_with_config`/`TestingConfig` names differ in this Freya rc, mirror the signature used by existing snapshot tests in `oxide-ui` — grep `render_to_file` for a working example.)

- [ ] **Step 7: Commit**

```bash
git add oxide-app/crates/oxide-ui/src/components/sidebar_header.rs oxide-app/crates/oxide-freya/src/regions/sidebar.rs
git commit -m "feat(sidebar): project switcher via Freya Select wired to open_project"
```

---

### Task 3: Lift sidebar collapse to state + animatable `CollapsiblePanel` width

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/collapsible_panel.rs` (optional width override)
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs` (drive collapse from `state.sidebar_collapsed`)

**Interfaces:**
- Consumes: `AppState.sidebar_collapsed` (Task 1).
- Produces: `CollapsiblePanel::override_width(Option<f32>)` — when `Some(w)`, the panel renders at `w` px regardless of `collapsed`; the *body* (`full`/`rail`) is still chosen by `collapsed`. Default `None` preserves current behavior.

- [ ] **Step 1: Write the failing test** (add to `collapsible_panel.rs` tests)

```rust
#[test]
fn override_width_sets_panel_width_but_body_follows_collapsed() {
    fn app() -> impl IntoElement {
        CollapsiblePanel::new()
            .collapsed(true)
            .override_width(Some(120.))
            .full(label().text("FULL"))
            .rail(label().text("RAIL"))
    }
    let mut t = launch_test(app);
    t.sync_and_update();
    // collapsed=true still shows the RAIL body...
    assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "RAIL")).is_some());
    // ...and the outer panel node measured 120px wide (the override), not 60.
    let node = t.find(|node, _| node.layout().map(|l| (l.area.width() - 120.).abs() < 0.5).unwrap_or(false));
    assert!(node.is_some(), "override_width should force a 120px panel width");
}
```

(If the `node.layout().area` accessor differs in this Freya rc, mirror the width-assertion style used by an existing `oxide-ui` layout test; the behavioral point is: override wins over the `SIDEBAR_RAIL_W`/`width` pick.)

- [ ] **Step 2: Run test to verify it fails**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui override_width_sets_panel`
Expected: FAIL — `no method override_width`.

- [ ] **Step 3: Add `override_width`** to `CollapsiblePanel`

Add field `override_width: Option<f32>,` (init `None` in `new`), builder:

```rust
    pub fn override_width(mut self, w: Option<f32>) -> Self {
        self.override_width = w;
        self
    }
```

In `render`, change the width pick:

```rust
        let (collapsed_w, body) = if self.collapsed {
            (SIDEBAR_RAIL_W, self.rail.clone())
        } else {
            (self.width, self.full.clone())
        };
        let w = self.override_width.unwrap_or(collapsed_w);
        rect()
            .width(Size::px(w))
            .height(Size::fill())
            .background(self.theme.surface())
            .maybe_child(body)
```

- [ ] **Step 4: Run tests to verify pass**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui collapsible_panel`
Expected: PASS (the two existing tests + the new one).

- [ ] **Step 5: Drive sidebar collapse from state** in `sidebar.rs`

Replace the local-state lines:

```rust
        let collapsed = use_state(|| false);
        let mut c_collapse = collapsed;
        let mut c_expand = collapsed;
```

with reads/writes of the shared signal:

```rust
        let mut collapsed = self.state.sidebar_collapsed;
        let mut c_collapse = collapsed;
        let mut c_expand = collapsed;
```

Change the collapse button handler to `c_collapse.set(true)` and the rail handler to `c_expand.set(false)` (unchanged), and the panel:

```rust
        CollapsiblePanel::new()
            .collapsed(*collapsed.read())
            .full(col.into_element())
            .rail(rail)
```

(`collapsed` now comes from `AppState`, so the state persists across parent re-renders — the defect fix. The `Sidebar.collapsed: bool` prop is now unused; leave it for Task 7/cleanup or remove if clippy flags it.)

- [ ] **Step 6: Build + test**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui -p oxide-freya`
Expected: PASS. `cargo clippy` clean.

- [ ] **Step 7: Commit**

```bash
git add oxide-app/crates/oxide-ui/src/components/collapsible_panel.rs oxide-app/crates/oxide-freya/src/regions/sidebar.rs
git commit -m "feat(sidebar): collapse state lifted to AppState; CollapsiblePanel width override"
```

---

### Task 4: Sidebar collapsed icon rail

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs` (build a real rail)

**Interfaces:**
- Consumes: `AppState.conversations`, `AppState.active`, `AppState.sidebar_collapsed`, `AppState::open_conversation`, `RailButton`, `StatusDot`.

- [ ] **Step 1: Replace the one-button rail** with an icon column. In `sidebar.rs`, replace:

```rust
        // Rail: just the expand button.
        let rail = RailButton::new("»".into())
            .on_press(move |_: Event<PressEventData>| c_expand.set(false));
```

with:

```rust
        // Collapsed rail: project dot, one icon per conversation, expand button.
        let convs_rail = state.conversations.read().clone();
        let active_rail = state.active.read().clone();
        let mut rail = rect()
            .direction(Direction::Vertical)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .padding(Gaps::new_all(8.))
            .width(Size::fill())
            .height(Size::fill())
            // project dot at top — press expands.
            .child(
                rect()
                    .width(Size::px(10.)).height(Size::px(10.))
                    .corner_radius(CornerRadius::new_all(5.))
                    .background(th.accent())
                    .on_press({
                        let mut e = c_expand;
                        move |_: Event<PressEventData>| e.set(false)
                    }),
            );
        for c in convs_rail {
            let st = state.clone();
            let id = c.id.clone();
            let sel = active_rail.as_ref() == Some(&c.id);
            let mut e = c_expand;
            rail = rail.child(
                rect()
                    .width(Size::px(28.)).height(Size::px(28.))
                    .corner_radius(CornerRadius::new_all(8.))
                    .center()
                    .background(if sel { th.surface_hi() } else { th.surface() })
                    .on_press(move |_: Event<PressEventData>| {
                        st.open_conversation(id.clone());
                        e.set(false);
                    })
                    .child(StatusDot::new(true)),
            );
        }
        let rail = rail.child(
            rect()
                .height(Size::flex(1.0))
                .cross_align(Alignment::Center)
                .child(
                    RailButton::new("»".into())
                        .on_press(move |_: Event<PressEventData>| c_expand.set(false)),
                ),
        );
```

(If `Sidebar` does not already `use oxide_ui::components::StatusDot`, add it to the `oxide_ui::components` import list at the top.)

- [ ] **Step 2: Snapshot the collapsed rail + READ it**

Add a shell snapshot variant (or reuse the existing `app.rs` collapsed snapshot test — `/tmp/oxide-shell-collapsed.png`). Run the shell collapsed snapshot:

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya -- --nocapture shell` (the snapshot test that clicks `«` then `render_to_file`)
Then **Read** the collapsed PNG — confirm the 60px rail shows the accent project dot + conversation icon squares + the `»` at the bottom (not just one button).

- [ ] **Step 3: Build + test**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya`
Expected: PASS. `cargo clippy` clean.

- [ ] **Step 4: Commit**

```bash
git add oxide-app/crates/oxide-freya/src/regions/sidebar.rs
git commit -m "feat(sidebar): collapsed icon rail (project dot + per-conversation icons)"
```

---

### Task 5: Right panel — rail + segmented expanded host + direction dispatcher

**Files:**
- Create: `oxide-app/crates/oxide-freya/src/regions/directions/mod.rs` (`DirectionPanel` dispatcher)
- Modify: `oxide-app/crates/oxide-freya/src/regions/mod.rs` (add `pub mod directions;`)
- Modify: `oxide-app/crates/oxide-freya/src/regions/context.rs` (rewrite)
- Modify: `oxide-app/crates/oxide-freya/src/app.rs` (`ContextRegion` takes `state`)

**Interfaces:**
- Consumes: `AppState` (`context_collapsed`, `active_direction`, all data signals), `StatusDirection`.
- Produces: `ContextRegion { pub state: AppState }`; `DirectionPanel { pub state: AppState, pub direction: StatusDirection }` (renders a placeholder body now; Task 6 fills each).

- [ ] **Step 1: Create the dispatcher** `regions/directions/mod.rs`

```rust
//! The right-panel "directions" — Spec / Mission / Workbench / Ambient.
use freya::prelude::*;

use crate::state::{AppState, StatusDirection};

mod spec;
mod mission;
mod workbench;
mod ambient;

/// Renders the body for one direction. Each arm is its own component so the
/// later (2d) rich content drops into a single file.
#[derive(PartialEq, Clone)]
pub struct DirectionPanel {
    pub state: AppState,
    pub direction: StatusDirection,
}

impl Component for DirectionPanel {
    fn render(&self) -> impl IntoElement {
        let s = self.state.clone();
        match self.direction {
            StatusDirection::Spec => spec::SpecDirection { state: s }.into_element(),
            StatusDirection::Mission => mission::MissionDirection { state: s }.into_element(),
            StatusDirection::Workbench => workbench::WorkbenchDirection { state: s }.into_element(),
            StatusDirection::Ambient => ambient::AmbientDirection { state: s }.into_element(),
        }
    }
}
```

- [ ] **Step 2: Create four placeholder bodies** so the module compiles. For each of `spec.rs`, `mission.rs`, `workbench.rs`, `ambient.rs` write a minimal stub (Task 6 fills them):

```rust
// regions/directions/spec.rs   (repeat per file, renaming the struct + label)
use freya::prelude::*;
use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct SpecDirection { pub state: AppState }

impl Component for SpecDirection {
    fn render(&self) -> impl IntoElement {
        rect().padding(Gaps::new_all(12.)).child(label().text("Spec"))
    }
}
```

Names per file: `spec.rs` → `SpecDirection` / "Spec"; `mission.rs` → `MissionDirection` / "Mission"; `workbench.rs` → `WorkbenchDirection` / "Workbench"; `ambient.rs` → `AmbientDirection` / "Ambient".

- [ ] **Step 3: Register the module** — add to `regions/mod.rs`:

```rust
pub mod directions;
```

- [ ] **Step 4: Rewrite `context.rs`** as the rail + segmented expanded host

```rust
//! Right region: a 60px direction rail (collapsed) or a segmented expanded panel.
use freya::prelude::*;
use oxide_ui::Theme;

use crate::regions::directions::DirectionPanel;
use crate::state::{AppState, StatusDirection};

const CONTEXT_FULL_W: f32 = 300.0;
const CONTEXT_RAIL_W: f32 = 60.0;

#[derive(PartialEq, Clone)]
pub struct ContextRegion {
    pub state: AppState,
}

impl Component for ContextRegion {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let state = self.state.clone();
        let collapsed = *state.context_collapsed.read();
        let active = *state.active_direction.read();

        if collapsed {
            // Rail: 4 direction icons + expand toggle.
            let mut rail = rect()
                .direction(Direction::Vertical)
                .cross_align(Alignment::Center)
                .spacing(6.)
                .padding(Gaps::new_all(8.))
                .width(Size::px(CONTEXT_RAIL_W))
                .height(Size::fill())
                .background(th.panel());
            for d in StatusDirection::ALL {
                let mut act = state.active_direction;
                let mut coll = state.context_collapsed;
                let selected = d == active;
                rail = rail.child(
                    rect()
                        .width(Size::px(34.)).height(Size::px(34.))
                        .corner_radius(CornerRadius::new_all(9.))
                        .center()
                        .background(if selected { th.surface_hi() } else { th.surface() })
                        .on_press(move |_: Event<PressEventData>| {
                            act.set(d);
                            coll.set(false);
                        })
                        .child(
                            label().text(d.icon()).font_size(15.)
                                .color(if selected { th.accent() } else { th.faint() }),
                        ),
                );
            }
            rail.into_element()
        } else {
            // Expanded: segmented header + active direction body.
            let mut header = rect()
                .direction(Direction::Horizontal)
                .content(Content::Flex)
                .spacing(4.)
                .padding(Gaps::new_all(8.))
                .width(Size::fill());
            for d in StatusDirection::ALL {
                let mut act = state.active_direction;
                let selected = d == active;
                header = header.child(
                    rect()
                        .width(Size::flex(1.0))
                        .center()
                        .padding(Gaps::new(6., 4., 6., 4.))
                        .corner_radius(CornerRadius::new_all(8.))
                        .background(if selected { th.accent_dim() } else { th.surface() })
                        .on_press(move |_: Event<PressEventData>| act.set(d))
                        .child(label().text(d.label()).font_size(11.5)
                            .color(if selected { th.text() } else { th.faint() })),
                );
            }
            let mut coll = state.context_collapsed;
            rect()
                .direction(Direction::Vertical)
                .width(Size::px(CONTEXT_FULL_W))
                .height(Size::fill())
                .background(th.panel())
                .child(
                    rect()
                        .direction(Direction::Horizontal)
                        .cross_align(Alignment::Center)
                        .width(Size::fill())
                        .child(rect().width(Size::flex(1.0)).child(header))
                        .child(
                            RailButtonText("»".into())
                                .on_press(move |_: Event<PressEventData>| coll.set(true)),
                        ),
                )
                .child(DirectionPanel { state: state.clone(), direction: active })
                .into_element()
        }
    }
}
```

Use the existing `RailButton` for the collapse toggle (import `oxide_ui::components::RailButton`) rather than the placeholder `RailButtonText` above — replace `RailButtonText("»".into())` with `oxide_ui::components::RailButton::new("»".into())`.

- [ ] **Step 5: Wire `ContextRegion` in `app.rs`** — replace `.child(ContextRegion { collapsed: true })` with:

```rust
        .child(ContextRegion { state: state.clone() })
```

- [ ] **Step 6: Snapshot the right panel (rail + each direction) and READ them**

Add a snapshot test in `context.rs` rendering the expanded panel for each direction (dark theme, `bg_deep` root). For the rail and for `StatusDirection::Spec` expanded, `render_to_file` then **Read** the PNGs — confirm the 4 rail icons and the segmented header with the active segment tinted.

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya context`
Expected: PASS; PNGs visually correct.

- [ ] **Step 7: Build + test + commit**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya`

```bash
git add oxide-app/crates/oxide-freya/src/regions/directions/ oxide-app/crates/oxide-freya/src/regions/mod.rs oxide-app/crates/oxide-freya/src/regions/context.rs oxide-app/crates/oxide-freya/src/app.rs
git commit -m "feat(context): right-panel direction rail + segmented expanded host"
```

---

### Task 6: The four direction bodies (now-content)

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/regions/directions/{spec,mission,workbench,ambient}.rs`

**Interfaces:**
- Consumes: `AppState.projects`, `current_project`, `conversations`, `active`, `connection`.
- Produces: each `*Direction` renders its now-content + a faint "more coming" line.

Helper used by all four (define once at the top of `mod.rs` and `pub(crate) use` it, or inline per file): a small `field(label, value)` row.

- [ ] **Step 1: Spec body** (`spec.rs`) — current project name + working dir

```rust
use freya::prelude::*;
use oxide_ui::Theme;
use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct SpecDirection { pub state: AppState }

impl Component for SpecDirection {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let s = &self.state;
        let cur = s.current_project.read().clone();
        let projects = s.projects.read().clone();
        let proj = cur.as_ref().and_then(|id| projects.iter().find(|p| &p.id == id));
        let (name, dir) = proj
            .map(|p| (p.name.clone(), p.default_working_dir.clone()))
            .unwrap_or_else(|| ("—".into(), "—".into()));
        rect().direction(Direction::Vertical).spacing(8.).padding(Gaps::new_all(12.))
            .child(label().text("Project").font_size(11.).color(th.faint()))
            .child(label().text(name).font_size(13.).color(th.text()))
            .child(label().text("Working dir").font_size(11.).color(th.faint()))
            .child(label().text(dir).font_size(12.).color(th.text()))
            .child(label().text("Spec viewer coming").font_size(10.5).color(th.faint()))
    }
}
```

- [ ] **Step 2: Mission body** (`mission.rs`) — active conversation title + model

```rust
use freya::prelude::*;
use oxide_ui::Theme;
use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct MissionDirection { pub state: AppState }

impl Component for MissionDirection {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let s = &self.state;
        let active = s.active.read().clone();
        let convs = s.conversations.read().clone();
        let conv = active.as_ref().and_then(|id| convs.iter().find(|c| &c.id == id));
        let body = match conv {
            Some(c) => rect().direction(Direction::Vertical).spacing(8.)
                .child(label().text("Conversation").font_size(11.).color(th.faint()))
                .child(label().text(if c.title.is_empty() { "untitled".into() } else { c.title.clone() })
                    .font_size(13.).color(th.text()))
                .child(label().text("Model").font_size(11.).color(th.faint()))
                .child(label().text(if c.model.is_empty() { "—".into() } else { c.model.clone() })
                    .font_size(12.).color(th.text())),
            None => rect().child(label().text("No conversation selected").font_size(12.).color(th.faint())),
        };
        rect().direction(Direction::Vertical).spacing(8.).padding(Gaps::new_all(12.))
            .child(body)
            .child(label().text("Mission tracker coming").font_size(10.5).color(th.faint()))
    }
}
```

- [ ] **Step 3: Workbench body** (`workbench.rs`) — working dir + worktree + attachment count

```rust
use freya::prelude::*;
use oxide_ui::Theme;
use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct WorkbenchDirection { pub state: AppState }

impl Component for WorkbenchDirection {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let s = &self.state;
        let active = s.active.read().clone();
        let convs = s.conversations.read().clone();
        let conv = active.as_ref().and_then(|id| convs.iter().find(|c| &c.id == id));
        let body = match conv {
            Some(c) => {
                let wt = c.worktree.as_ref()
                    .map(|w| format!("{} @ {}", w.branch, w.path))
                    .unwrap_or_else(|| "no worktree".into());
                rect().direction(Direction::Vertical).spacing(8.)
                    .child(label().text("Working dir").font_size(11.).color(th.faint()))
                    .child(label().text(if c.working_dir.is_empty() { "—".into() } else { c.working_dir.clone() })
                        .font_size(12.).color(th.text()))
                    .child(label().text("Worktree").font_size(11.).color(th.faint()))
                    .child(label().text(wt).font_size(12.).color(th.text()))
            }
            None => rect().child(label().text("No conversation selected").font_size(12.).color(th.faint())),
        };
        rect().direction(Direction::Vertical).spacing(8.).padding(Gaps::new_all(12.))
            .child(body)
            .child(label().text("Diffs + tools coming").font_size(10.5).color(th.faint()))
    }
}
```

- [ ] **Step 4: Ambient body** (`ambient.rs`) — connection + active id (feed deferred per spec)

```rust
use freya::prelude::*;
use oxide_ui::Theme;
use crate::state::{AppState, ConnState};

#[derive(PartialEq, Clone)]
pub struct AmbientDirection { pub state: AppState }

impl Component for AmbientDirection {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let s = &self.state;
        let conn = *s.connection.read();
        let (txt, col) = match conn {
            ConnState::Connected => ("connected", th.accent()),
            ConnState::Reconnecting => ("reconnecting…", Color::from_rgb(255, 171, 64)),
            ConnState::Unreachable => ("unreachable", Color::from_rgb(255, 120, 120)),
            ConnState::Unknown => ("unknown", th.faint()),
        };
        rect().direction(Direction::Vertical).spacing(8.).padding(Gaps::new_all(12.))
            .child(label().text("Connection").font_size(11.).color(th.faint()))
            .child(label().text(txt).font_size(13.).color(col))
            .child(label().text("Activity feed coming").font_size(10.5).color(th.faint()))
    }
}
```

- [ ] **Step 5: Snapshot each direction expanded + READ all four PNGs**

Extend the `context.rs` snapshot test to render the expanded panel once per `StatusDirection` (set `active_direction` before render). `render_to_file` to `/tmp/shell-dir-{spec,mission,workbench,ambient}.png`, then **Read** each — confirm the labelled fields render in light-on-dark.

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya directions`
Expected: PASS.

- [ ] **Step 6: Build + test + commit**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya`

```bash
git add oxide-app/crates/oxide-freya/src/regions/directions/
git commit -m "feat(directions): spec/mission/workbench/ambient now-content bodies"
```

---

### Task 7: Animations (collapse width tween + direction cross-fade)

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs` (animate width)
- Modify: `oxide-app/crates/oxide-freya/src/regions/context.rs` (animate width + body cross-fade)

**Interfaces:**
- Consumes: `use_animation`, `AnimNum`, `Ease`, `Function` (Freya). Pattern reference: `crates/freya-components/src/select.rs` in the Freya repo (`/run/media/system/fastdrive/repos/freya`) — read it for the exact `use_animation(...).read().value()` shape before writing.

This task is **additive**: tasks 3–6 already work with instant swaps. If the width `AnimNum` proves fiddly, the fallback is to keep the instant swap (do not block the slice on the tween).

- [ ] **Step 1: Sidebar width tween** — in `sidebar.rs`, before building `CollapsiblePanel`, add:

```rust
        use oxide_ui::tokens::{SIDEBAR_FULL_W, SIDEBAR_RAIL_W};
        let is_collapsed = *collapsed.read();
        let animation = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            let w = AnimNum::new(SIDEBAR_FULL_W, SIDEBAR_RAIL_W)
                .time(180).ease(Ease::Out).function(Function::Quart);
            if is_collapsed { w } else { w.into_reversed() }
        });
        let anim_w = animation.read().value();
```

Then pass it:

```rust
        CollapsiblePanel::new()
            .collapsed(is_collapsed)
            .override_width(Some(anim_w))
            .full(col.into_element())
            .rail(rail)
```

- [ ] **Step 2: Context width tween + body cross-fade** — in `context.rs`, drive `CONTEXT_FULL_W`↔`CONTEXT_RAIL_W` with the same `use_animation` pattern and apply the width to the panel's outer `rect`. Add an opacity `AnimNum` (time 120) keyed on `active_direction` to the `DirectionPanel` wrapper (`rect().opacity(fade).child(DirectionPanel{..})`).

Mirror Step 1's structure; read `select.rs` for the exact `AnimNum`/`.value()` call. Keep the rail/expanded branch structure from Task 5; only the widths/opacity become animated.

- [ ] **Step 3: Snapshot end-states + READ**

Snapshots assert end-states only (not mid-tween). Re-run the Task 4 + Task 5 snapshots; **Read** the collapsed-rail and expanded PNGs — confirm no regression (animation must not change the resting frames).

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add oxide-app/crates/oxide-freya/src/regions/sidebar.rs oxide-app/crates/oxide-freya/src/regions/context.rs
git commit -m "feat(shell): animated collapse width + direction cross-fade"
```

---

### Task 8: Live verification + cleanup

**Files:** none (build + run) — plus any clippy cleanup (e.g. drop the now-unused `Sidebar.collapsed` / `ContextRegion` old field).

- [ ] **Step 1: Full build + clippy + tests**

Run: `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui -p oxide-freya && LIBRARY_PATH=/tmp/oxidemx-lib-links cargo clippy -p oxide-ui -p oxide-freya`
Expected: all PASS, clippy clean.

- [ ] **Step 2: Build the binary, relaunch, and verify live** (only after green snapshots)

```bash
LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build -p oxide-freya --bin oxide-freya
pkill -f "debug/oxide-freya$"; sleep 1
setsid bash -c 'LIBRARY_PATH=/tmp/oxidemx-lib-links exec ./target/debug/oxide-freya' </dev/null >/tmp/oxide-freya-live.log 2>&1 &
```

Verify (ask the user): switcher opens + picking a project swaps conversations; both panels collapse to rails with the icon columns + animate; each of the 4 directions shows its body.

- [ ] **Step 3: Commit any cleanup**

```bash
git add -A oxide-app/crates/oxide-freya
git commit -m "chore(shell): clippy cleanup + drop unused collapse props"
```

---

## Self-Review notes

- **Spec coverage:** state model (T1) ✓; project switcher (T2) ✓; collapse lift + animated width (T3) ✓; icon rail (T4) ✓; right-panel framework (T5) ✓; 4 direction bodies (T6) ✓; animations (T7) ✓; app wiring folded into T2/T3/T5; live verify (T8) ✓.
- **Crate boundary:** `oxide-ui` (`SidebarHeader`, `CollapsiblePanel`) takes only primitives/`EventHandler` — no `AppState`/`Project`/`StatusDirection`. ✓
- **Type consistency:** `StatusDirection` (label/icon/ALL/Default=Spec), `open_project(ProjectId)`, `current_project`/`sidebar_collapsed`/`context_collapsed`/`active_direction`, `CollapsiblePanel::override_width(Option<f32>)`, `ContextRegion { state }`, `DirectionPanel { state, direction }`, `*Direction { state }` used identically across tasks. ✓
- **Freya API caveats flagged for the implementer:** the exact `freya_testing` snapshot config call and `use_animation().read().value()` shape may differ slightly by rc — each task says to mirror an existing working example (`render_to_file` in `oxide-ui`, `select.rs` for animation) rather than guess.
