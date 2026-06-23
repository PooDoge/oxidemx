# Freya App 2b — Phase 2 (Sidebar) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use `- [ ]`.

**Goal:** Restyle the OxideMX Freya **left Sidebar** to the "Collapsible Panels" design — project switcher + search + new-conversation + styled conversation rows (state dot + worktree chip) + the collapsed rail.

**Architecture:** Restyle the existing `ListItem` into a design conversation row, add a `SidebarHeader` component, recompose the `Sidebar` region (drop the 2a placeholder-page nav demo — the per-region-nav invariant is independently proven by `nav.rs::region_nav_is_independent`). Theme + thread (P1) already landed; reuse `Theme`, `StatusDot`, `WorktreeChip`.

**Tech Stack:** Rust, Freya v0.4.0-rc.23, `oxide-ui` + `oxide-freya` (in `oxide-app/`), `freya-testing`.

## Global Constraints

- **Source of truth:** the `freya.json` `Sidebar` subtree (`oxide-app/design-pipeline/OxideMX Freya - Collapsible Panels.freya.json`) + the **`claude-design-to-freya`** skill (run it + `freya-gui-framework` + read `oxide-app/FREYA-PATTERNS.md`). The `freya.json` maps: `ProjectSwitcher`→`Select`, `SearchBox`→`Input`, `NewConversation`→`Button`, `ConversationList`→`ScrollView`, `ConvRow`→`SideBarItem` (active = `with_alpha(accent,0x14)` fill + `with_alpha(accent,0x3a)` border, radius 9) with a `StateDot` (state→tone) + `WorktreeChip`.
- **Rule 1:** the sidebar renders only transport-delivered data (`state.conversations`); no fabricated rows. Selecting a row calls the existing `state.open_conversation(id)`.
- **Rule 2:** clippy-clean, hand-formatted, no gold-plating (style the listed parts; the project-switcher dropdown menu + a settings second-page are future — a styled static switcher row is enough for P2). Reuse P1's `Theme`/`StatusDot`/`WorktreeChip`.
- **Verified rc.23 APIs (from P1):** `Theme::with_alpha(base, 0x14)`; `CornerRadius { top_left, top_right, bottom_right, bottom_left, smoothing: 0. }` or `CornerRadius::new_all(f)`; `Border::new().fill(c).width(1.)`; built-in `Input` themed via **`.theme_colors(InputColorsThemePartial { background, focus_background, border_fill, focus_border_fill, color, placeholder_color: Option<Preference<Color>> })`** (dual setter — NOT `.theme()`); built-in `Button` via `.theme_colors(ButtonColorsThemePartial{..})` + `.filled()`. Gradient `.stop((color, pos))` is a TUPLE. The skill + `oxide-app/API-NOTES.md` + freya source are the tiebreaker — treat compile errors as ground truth.
- **Token dims:** sidebar full width `SIDEBAR_FULL_W = 274.0`, rail `SIDEBAR_RAIL_W = 60.0` (already in tokens.rs). Panel bg = `theme.panel()` (mantle).
- **Build:** from `oxide-app/` with `LIBRARY_PATH=/tmp/oxidemx-lib-links`; `oxide-freya` is a BINARY crate. **The `nav.rs::region_nav_is_independent` test MUST stay green** (it proves the per-region-nav invariant). The 2a `sidebar_nav_does_not_clear_center_thread` test in `app.rs` depended on the now-removed placeholder page — it is updated/removed in Task 3 (the invariant remains covered by `nav.rs`).

---

## File Structure
- Modify `oxide-app/crates/oxide-ui/src/components/list_item.rs` — restyle into the design conversation row (+ state dot + worktree chip + selected accent-tint).
- Create `oxide-app/crates/oxide-ui/src/components/sidebar_header.rs` — `SidebarHeader` (project switcher row + search + new button).
- Modify `oxide-app/crates/oxide-ui/src/components/mod.rs` — export `SidebarHeader`.
- Modify `oxide-app/crates/oxide-freya/src/regions/sidebar.rs` — recompose the styled sidebar; drop the placeholder demo.
- Modify `oxide-app/crates/oxide-freya/src/app.rs` — update the placeholder-dependent test; add a sidebar snapshot.

---

## Task 1: Restyle ListItem → design conversation row

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/list_item.rs`

**Interfaces — Produces:** `ListItem::new(label: String)` (unchanged) + `.selected(bool)` + `.on_press(..)` (unchanged) + NEW `.state(String)` (conversation state → leading dot tone) + `.worktree(Option<String>)` (trailing chip) + `.theme(Theme)`. Design row: `theme.panel()` base, radius 9, selected = `with_alpha(accent,0x14)` fill + `with_alpha(accent,0x3a)` border + `text` color; idle = transparent/panel + `subtext_hi` color; a leading 6px **StatusDot**-style tone dot (state working=yellow/delivered=green/failed=red/idle=overlay) + the title + an optional trailing `WorktreeChip`.

- [ ] **Step 1: Update the render test** — keep `list_item_renders_label`; add: a `.state("working")` row renders, and a selected row still renders its label. (Color asserts are brittle — assert label presence + that `.worktree(Some("wt"))` renders "wt".)

```rust
#[test]
fn list_item_with_worktree_renders_branch() {
    fn app() -> impl IntoElement {
        ListItem::new("Chat".into()).state("working".into()).worktree(Some("wt-x".into()))
    }
    let mut t = launch_test(app); t.sync_and_update();
    assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "Chat")).is_some());
    assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "wt-x")).is_some());
}
```

- [ ] **Step 2: Run — verify fail.** `cargo test -p oxide-ui list_item` → FAIL.

- [ ] **Step 3: Implement** — add `state: String` (default "idle") + `worktree: Option<String>` fields + builders; restyle `render`:

```rust
fn render(&self) -> impl IntoElement {
    let th = self.theme;
    let tone = match self.state.as_str() {
        "working" => th.yellow(), "delivered" => th.green(), "failed" => th.red(), _ => th.overlay(),
    };
    let (bg, txt, border) = if self.selected {
        (Theme::with_alpha(th.accent(), 0x14), th.text(), Theme::with_alpha(th.accent(), 0x3a))
    } else {
        (Color::from_argb(0, 0, 0, 0), th.subtext_hi(), Color::from_argb(0, 0, 0, 0))
    };
    let row = rect()
        .direction(Direction::Horizontal).cross_align(Alignment::Center).spacing(8.)
        .width(Size::fill()).padding(Gaps::new(8., 10., 8., 10.))
        .corner_radius(CornerRadius::new_all(9.))
        .background(bg).border(Border::new().fill(border).width(1.))
        .child(rect().width(Size::px(6.)).height(Size::px(6.))
            .corner_radius(CornerRadius::new_all(3.)).background(tone))
        .child(label().text(self.label.clone()).font_size(12.5).color(txt).width(Size::flex(1.0)))
        .maybe_child(self.worktree.clone().map(|w|
            crate::components::chip::WorktreeChip::new(w).theme(th)));
    if let Some(handler) = self.on_press.clone() { row.on_press(handler) } else { row }
}
```

(Confirm `maybe_child` + transparent `Color::from_argb(0,..)` + `on_press` chain against API-NOTES/FREYA-PATTERNS.)

- [ ] **Step 4: Run — verify pass + clippy.** `cargo test -p oxide-ui list_item && cargo clippy -p oxide-ui -- -D warnings` → PASS, clean.

- [ ] **Step 5: Commit.** `git commit -m "feat(oxide-ui): restyle ListItem into the design conversation row (state dot + worktree chip)"`

---

## Task 2: SidebarHeader (project switcher + search + new)

**Files:** Create `oxide-app/crates/oxide-ui/src/components/sidebar_header.rs`; Modify `components/mod.rs`.

**Interfaces — Produces:** `SidebarHeader::new(project: String)` builder `Component` (+ `.theme`). Renders, top to bottom: a **project switcher** row (a styled `surface()`/`hairline_strong` pill: a small accent dot + the project name + a chevron "⌄"), a **search** built-in `Input` (placeholder "Search…", themed `bg_deep`/`surface_max`), a **New conversation** built-in `Button` (`.filled()`, accent fill, `+ New` label). A static switcher is fine for P2 (the dropdown menu is future).

- [ ] **Step 1: Render test** — asserts the project name + the "+ New" button label render.

```rust
#[test]
fn sidebar_header_renders_project_and_new() {
    fn app() -> impl IntoElement { SidebarHeader::new("oxidemx-phase1".into()) }
    let mut t = launch_test(app); t.sync_and_update();
    assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("oxidemx-phase1"))).is_some());
    assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("New"))).is_some());
}
```

- [ ] **Step 2: Run — verify fail.** `cargo test -p oxide-ui sidebar_header` → FAIL.

- [ ] **Step 3: Implement** `sidebar_header.rs` — a vertical column: switcher pill (rect, `surface()` bg, `hairline_strong` border, radius 9, row: accent dot + project label + "⌄"), a search `Input::new(use_state(String::new).into_writable()).placeholder("Search…").theme_colors(InputColorsThemePartial{ background: Some(Preference::Specific(th.bg_deep())), border_fill: Some(Preference::Specific(th.surface_max())), color: Some(Preference::Specific(th.text())), placeholder_color: Some(Preference::Specific(th.faint())), ..Default::default() })`, and a `Button::new().filled().theme_colors(ButtonColorsThemePartial{ background: Some(Preference::Specific(th.accent())), color: Some(Preference::Specific(th.bg_deep())), ..Default::default() }).child("+ New")`. Spacing 8, padding 8. (Confirm the exact `InputColorsThemePartial`/`ButtonColorsThemePartial` field names against the freya source as in P1's composer; the search Input's own `use_state` is local/non-functional in P2 — wiring search is future.)

- [ ] **Step 4: Run — verify pass + clippy + export** in `mod.rs` (`pub mod sidebar_header; pub use sidebar_header::SidebarHeader;`).

- [ ] **Step 5: Commit.** `git commit -m "feat(oxide-ui): SidebarHeader (project switcher + search + new conversation)"`

---

## Task 3: Recompose the styled Sidebar region + collapsed rail + snapshot

**Files:** Modify `oxide-app/crates/oxide-freya/src/regions/sidebar.rs`; Modify `oxide-app/crates/oxide-freya/src/app.rs` (test update + snapshot).

**Interfaces — Consumes:** `SidebarHeader`, restyled `ListItem`, `Theme`, `CollapsiblePanel`, `AppState`.

- [ ] **Step 1: Recompose `Sidebar::render`.** Drop the `SidebarPage` enum + the placeholder-page nav demo + the `≡ Conversations` nav label. Keep the `collapsed` `use_state` + the `CollapsiblePanel` (full↔rail). The `full` content (vertical column, `theme.panel()` bg): `SidebarHeader::new(project_name)` + a `ScrollView` of styled `ListItem` rows from `state.conversations` (each `.state(conv.state-or-"idle").selected(active==id).worktree(conv.worktree.map(branch)).on_press(open_conversation)`) + the bottom collapse `«` `RailButton`. The `rail` content: the `»` expand button (already there). Project name: derive from the first project or a constant "oxidemx-phase1" for now (real project resolution is future). Conversation `state` field: the `Conversation` DTO has no `state` — use `"idle"` for all in P2 (per-conversation run state is the deferred activity work); worktree from `conv.worktree`.

```rust
impl Component for Sidebar {
    fn render(&self) -> impl IntoElement {
        let collapsed = use_state(|| false);
        let mut c_collapse = collapsed; let mut c_expand = collapsed;
        let state = self.state.clone();
        let th = oxide_ui::Theme::default();
        let convs = state.conversations.read().clone();
        let active = state.active.read().clone();
        let mut col = rect().direction(Direction::Vertical).content(Content::Flex)
            .width(Size::fill()).height(Size::fill()).background(th.panel())
            .child(oxide_ui::components::SidebarHeader::new("oxidemx-phase1".into()).theme(th));
        let mut list = rect().direction(Direction::Vertical).spacing(3.).width(Size::fill());
        for c in convs {
            let st = state.clone(); let id = c.id.clone();
            let sel = active.as_ref() == Some(&c.id);
            list = list.child(
                oxide_ui::components::ListItem::new(c.title.clone())
                    .selected(sel).state("idle".into())
                    .worktree(c.worktree.as_ref().map(|w| w.branch.clone()))
                    .theme(th)
                    .on_press(move |_| st.open_conversation(id.clone())));
        }
        col = col.child(rect().width(Size::fill()).height(Size::flex(1.0))
            .padding(Gaps::new(0., 8., 0., 8.)).child(ScrollView::new().child(list)));
        col = col.child(rect().padding(Gaps::new_all(8.))
            .child(RailButton::new("«".into())
                .on_press(move |_: Event<PressEventData>| c_collapse.set(true))));
        let rail = RailButton::new("»".into())
            .on_press(move |_: Event<PressEventData>| c_expand.set(false));
        CollapsiblePanel::new().collapsed(*collapsed.read()).full(col.into_element()).rail(rail)
    }
}
```

(Confirm `Conversation.worktree` field shape — `Option<Worktree { branch }>` per the agentd model — and `Theme`/component import paths. Remove the now-unused `use_region_nav`/`SidebarPage` imports.)

- [ ] **Step 2: Update the broken app test.** `sidebar_nav_does_not_clear_center_thread` in `app.rs` relied on the placeholder page — it no longer exists. REPLACE it with a test that the styled sidebar renders a seeded conversation title (the per-region-nav invariant is still covered by `nav.rs::region_nav_is_independent`):

```rust
#[test]
fn sidebar_renders_conversation_rows() {
    let mock = mock_with_history(); // seeds a "Chat 1" conversation
    let app = harness_with_history(mock);
    let mut runner = launch_test(app);
    runner.poll_n(Duration::from_millis(5), 8);
    assert!(runner.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "Chat 1")).is_some(),
        "sidebar should render the conversation row");
}
```
(Adjust `harness_with_history`/`mock_with_history` if they reference the removed placeholder. Keep all OTHER app tests + `nav.rs` tests green.)

- [ ] **Step 3: Build + full suite + clippy.** `cargo build -p oxide-freya && cargo test -p oxide-freya && cargo test -p oxide-ui && cargo clippy -p oxide-freya -p oxide-ui -- -D warnings` → all PASS, clean. (`nav::tests::region_nav_is_independent` MUST be green.)

- [ ] **Step 4: Add + render a sidebar snapshot.** Add a `#[ignore]` `snapshot_sidebar_p2` to `app.rs`: mount the full shell (or `Sidebar`) with a seeded `MockTransport` (3-4 conversations, one with a worktree) at `(900., 800.)` → `render_to_file("/tmp/oxide-sidebar-p2.png")`. Run `cargo test -p oxide-freya --bin oxide-freya snapshot_sidebar_p2 -- --ignored`; confirm the PNG renders.

- [ ] **Step 5: Commit.** `git commit -m "feat(oxide-freya): styled Sidebar (header + conversation rows + rail); drop placeholder nav demo + snapshot"`

---

## Self-Review

**Spec coverage (P2):** project switcher + search + new (Task 2) ✓ · conversation rows w/ state dot + worktree chip + selected accent-tint (Task 1) ✓ · collapsed rail (Task 3, reuses CollapsiblePanel) ✓ · wired to `conversations`/`open_conversation`, Rule 1 (real data only) ✓ · snapshot compared (Task 3) ✓. Deferred-per-spec: the switcher dropdown menu, search wiring, per-conversation run state — correctly absent.

**Placeholder scan:** none. The "static switcher / idle state / oxidemx-phase1 project name" are explicit P2 simplifications noted as future, not vague TODOs.

**Type consistency:** `ListItem` new `.state(String)/.worktree(Option<String>)` used in Task 3; `SidebarHeader::new(String)` used in Task 3; `StatusDot`/`WorktreeChip`/`Theme` from P1. The invariant test moves from app.rs (placeholder) to relying on `nav.rs` (seam) — coverage preserved.

## Execution Handoff

superpowers:subagent-driven-development, in the existing `2b-collapsible-panels` worktree. After P2, rebuild + relaunch; the left panel then matches the design too. P3 (native chrome + right rail) follows.
