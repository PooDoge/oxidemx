# Sidebar "+" New-Conversation Button Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the sidebar's full-width "+ New" stub with a compact "+" icon button inline with the search input, and wire it to create a new conversation in the current project.

**Architecture:** Client-only — the backend (`Transport::create_conversation`, agentd `POST /v1/conversations`) already exists. Add `AppState::create_conversation()` (spawn → optimistic insert → activate), change `SidebarHeader`'s layout + add an `on_new` prop, and wire the two together in `sidebar.rs`.

**Tech Stack:** Rust, Freya blog/0.4 (`rect`/`svg`/`Content::Flex`/`Shadow`, `freya_icons::lucide::plus`), `EventHandler`, `Arc<dyn Transport>` seam, `freya_testing` (`launch_test` + `poll_n`).

## Global Constraints

- Backend is DONE — call `Transport::create_conversation(project_id: &str, working_dir: Option<&str>)` (oxide-client/src/transport.rs:14). **No agentd changes.**
- `AppState` is `#[derive(Clone)]` with a custom `PartialEq`; its signals are `Copy` `State<T>`. `current_project: State<Option<ProjectId>>`, `conversations: State<Vec<Conversation>>`, `active: State<Option<ConversationId>>`. Construct only inside a Freya component (`AppState::new` calls `use_state`).
- Spawn pattern: use Freya `spawn` (NOT tokio), capture `Copy` signals + `self.transport.clone()`; mirror `open_conversation` (state.rs:261) / `send` (state.rs:303).
- Test-driving spawns: `launch_test(component)` then `runner.poll_n(Duration::from_millis(5), 12)`, assert via `runner.find(...)` on rendered elements (see app.rs `open_and_send_app`).
- `oxide-ui` must NOT depend on `oxide-freya`. `SidebarHeader` takes only owned data + `EventHandler`.
- Rust quality: `cargo clippy` clean; hand-formatted (only format added lines, no repo-wide fmt); borrow over clone; no gold-plating.
- Lucide: `freya_icons::lucide::plus()` (the SVG submodule is synced in the canonical Freya checkout); used already at regions/context/mod.rs:138 (`svg(lucide::chevrons_right())`).

**Run every cargo command as:**
```
distrobox enter claude_development -- bash -lc 'cd <WORKTREE>/oxide-app && CARGO_TARGET_DIR=<WARM_TARGET> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo <args>'
```
(`<WORKTREE>` = this slice's worktree; `<WARM_TARGET>` = its dedicated reflink target.)

---

### Task 1: `AppState::create_conversation()` + MockTransport failure flag + test

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/state.rs` (add method; add a test)
- Modify: `oxide-app/crates/oxide-client/src/mock.rs` (add `create_conversation_fails` flag)

**Interfaces:**
- Consumes: `Transport::create_conversation`, `AppState.{current_project, conversations, active, transport}`, `AppState::open_conversation`.
- Produces: `pub fn AppState::create_conversation(&self)` — spawns create; on Ok pushes the new `Conversation` into `conversations` and calls `open_conversation(id)`; on Err logs. `MockTransport.create_conversation_fails: bool` (default false).

- [ ] **Step 1: Add the failure flag to MockTransport.** In `mock.rs`, add `pub create_conversation_fails: bool,` to the struct; set `create_conversation_fails: false` in `MockTransport::new()`. Change the `create_conversation` impl to:
```rust
async fn create_conversation(&self, project_id: &str, _wd: Option<&str>) -> Result<Conversation, TransportError> {
    if self.create_conversation_fails {
        return Err(TransportError::Unreachable("mock create".into()));
    }
    Ok(Conversation { id: ConversationId::from("mock-conv"), project_id: ProjectId::from(project_id),
        title: "New".into(), working_dir: String::new(), model: String::new(), created_at: 0, updated_at: 0,
        worktree: None })
}
```
(Use the same `TransportError` variant `health()` uses — `TransportError::Unreachable(String)`; if its shape differs, mirror `health()`'s error.)

- [ ] **Step 2: Build mock to verify the flag compiles.**

Run: `cargo build -p oxide-client`
Expected: Finished.

- [ ] **Step 3: Add `create_conversation` to `AppState`.** In `state.rs`, immediately after `open_conversation` (ends ~line 295) add:
```rust
    /// Create a new conversation in the current project, then make it active.
    /// Project comes from `current_project`; model + working_dir default server-side.
    /// On success the conversation is inserted optimistically and opened; transport
    /// errors are logged and leave the UI unchanged.
    pub fn create_conversation(&self) {
        let Some(project_id) = self.current_project.peek().clone() else { return };
        let mut conversations = self.conversations;
        let this = self.clone();
        let t = self.transport.clone();
        spawn(async move {
            match t.create_conversation(project_id.as_str(), None).await {
                Ok(conv) => {
                    let id = conv.id.clone();
                    conversations.with_mut(|mut cs| cs.push(conv));
                    this.open_conversation(id);
                }
                Err(e) => tracing::warn!("create_conversation failed: {e}"),
            }
        });
    }
```
(If `tracing` isn't imported in this file, use the crate's existing logging — grep for `tracing::` or `log::` usage in state.rs and match it; if none, `eprintln!("create_conversation failed: {e}")` is acceptable and clippy-clean.)

- [ ] **Step 4: Write the failing integration test.** In `state.rs` `#[cfg(test)] mod tests`, add (mirrors app.rs `open_and_send_app` poll pattern):
```rust
    #[test]
    fn create_conversation_ok_inserts_and_activates() {
        use freya_testing::prelude::*;
        use oxide_client::mock::MockTransport;
        fn app() -> impl IntoElement {
            let state = AppState::new(Arc::new(MockTransport::new()));
            let st = state.clone();
            use_hook(move || {
                st.current_project.clone().set(Some(ProjectId::from("personal")));
                st.create_conversation();
            });
            let n = state.conversations.read().len();
            let active = state.active.read().clone().map(|i| i.as_str().to_string()).unwrap_or_default();
            label().text(format!("n={n} active={active}"))
        }
        let mut runner = launch_test(app);
        runner.poll_n(Duration::from_millis(5), 12);
        let found = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("n=1") && l.text.as_ref().contains("active=mock-conv"))
        });
        assert!(found.is_some(), "create_conversation should optimistically insert + activate the new conversation");
    }

    #[test]
    fn create_conversation_err_leaves_unchanged() {
        use freya_testing::prelude::*;
        use oxide_client::mock::MockTransport;
        fn app() -> impl IntoElement {
            let mock = MockTransport { create_conversation_fails: true, ..MockTransport::new() };
            let state = AppState::new(Arc::new(mock));
            let st = state.clone();
            use_hook(move || {
                st.current_project.clone().set(Some(ProjectId::from("personal")));
                st.create_conversation();
            });
            let n = state.conversations.read().len();
            label().text(format!("n={n}"))
        }
        let mut runner = launch_test(app);
        runner.poll_n(Duration::from_millis(5), 12);
        assert!(runner.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("n=0"))).is_some(),
            "a failed create must leave conversations empty");
    }
```
(If `Duration`/`ProjectId`/`Arc` aren't already imported in the test module, add the `use`s — grep the existing test module header at state.rs:320 and match; app.rs's send test shows the exact `launch_test`+`poll_n`+`find` API.)

- [ ] **Step 5: Run the tests — expect them to drive correctly.**

Run: `cargo test -p oxide-freya create_conversation`
Expected: both PASS. (If a signal-read-in-render doesn't re-trigger after the spawn, increase `poll_n` count to 20 — match what app.rs uses; do not change the production code to satisfy the test.)

- [ ] **Step 6: clippy + commit**

Run: `cargo clippy -p oxide-client -p oxide-freya` → clean.
```bash
git add oxide-app/crates/oxide-freya/src/state.rs oxide-app/crates/oxide-client/src/mock.rs
git commit -m "feat(state): AppState::create_conversation (optimistic insert + activate) + mock failure flag"
```

---

### Task 2: `SidebarHeader` — inline search + "+" icon button + `on_new`

**Files:**
- Modify: `oxide-app/crates/oxide-ui/src/components/sidebar_header.rs`
- Modify: `oxide-app/crates/oxide-ui/Cargo.toml` (add `freya-icons`)

**Interfaces:**
- Consumes: `freya_icons::lucide::plus`, `EventHandler<()>`.
- Produces: `SidebarHeader::on_new(impl Into<EventHandler<()>>)` builder; the header now renders `switcher` + a horizontal `[search(flex) | "+" 34×34]` row; the old "+ New" `Button` is gone.

- [ ] **Step 1: Add `freya-icons` to oxide-ui.** In `oxide-ui/Cargo.toml` `[dependencies]` add:
```toml
freya-icons = { workspace = true, features = ["lucide"] }
```
(Match how `oxide-freya/Cargo.toml` declares it. If the workspace dep doesn't carry `features`, copy oxide-freya's exact line.)

- [ ] **Step 2: Add the `on_new` field + builder.** In `sidebar_header.rs`, add `on_new: Option<EventHandler<()>>,` to the `SidebarHeader` struct; initialise `on_new: None` in `new()`; add:
```rust
    pub fn on_new(mut self, h: impl Into<EventHandler<()>>) -> Self {
        self.on_new = Some(h.into());
        self
    }
```
(`EventHandler<()>` is `Copy`; `Option<EventHandler<()>>` is fine in the `#[derive(PartialEq, Clone)]` struct — `EventHandler` is `PartialEq`.)

- [ ] **Step 3: Replace the "+ New" button with the inline "+" icon button + row.** In `render`, delete the `new_btn` `Button` block and its `ButtonColorsThemePartial`. Capture the handler and give the search a flex width:
```rust
        let on_new = self.on_new;
        let search = TextInput::new(value.into_writable(), th)
            .placeholder("Search…")
            .width(Size::flex(1.0));

        let plus = rect()
            .width(Size::px(34.)).height(Size::px(34.))
            .corner_radius(CornerRadius::new_all(9.))
            .background(th.accent())
            .center()
            .a11y_role(AccessibilityRole::Button)
            .a11y_alt("New conversation")
            .shadow(Shadow::new(0., 4., 12., 0., Theme::with_alpha(th.accent(), 0x40)))
            .on_press(move |_: Event<PressEventData>| { if let Some(h) = on_new { h.call(()); } })
            .child(svg(freya_icons::lucide::plus()).width(Size::px(17.)).height(Size::px(17.)).color(th.bg_deep()));

        let search_row = rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.)
            .width(Size::fill())
            .child(search)
            .child(plus);
```
Then change the final assembly from `.child(switcher).child(search).child(new_btn)` to:
```rust
        rect()
            .direction(Direction::Vertical)
            .spacing(8.)
            .padding(Gaps::new_all(8.))
            .width(Size::fill())
            .child(switcher)
            .child(search_row)
```
(Verify `TextInput` has a `.width(...)` builder; if not, wrap it: `rect().width(Size::flex(1.0)).child(search)`. Verify `svg(...).color(...)` is the right tint method — grep an existing `svg(lucide::` call; context/mod.rs:138 shows the pattern. `Shadow::new` arg order: `(x, y, blur, spread, color)` — confirm against an existing `.shadow(Shadow::new(...))` in oxide-ui; if the signature differs, mirror it.)

- [ ] **Step 4: Update the unit test.** The `sidebar_header_renders_project_and_new` test asserts a "New" label that no longer exists. Replace that assertion — keep the project-name + "Search…" assertions, and assert the new button via its a11y alt:
```rust
        assert!(
            t.find(|_, el| el.get_accessibility().and_then(|a| a.alt.clone())
                .map(|alt| alt.contains("New conversation")).unwrap_or(false)).is_some(),
            "SidebarHeader should render the '+' new-conversation button (a11y alt)"
        );
```
(If reading a11y alt from a test node isn't ergonomic in `freya_testing`, instead assert the SVG renders: `t.find(|_, el| Svg::try_downcast(el).is_some())` — grep `freya_testing` for the right downcast helper (`Svg`/`Image`); use whichever exists. The point: assert the button is present, not the dead "+ New" text.)

- [ ] **Step 5: Extend the dark snapshot.** `snapshot_switcher_dark` already renders the header — no code change needed beyond confirming it still renders the new layout. Update its doc note to mention the inline "+" button. Keep it `#[ignore]` writing to `/tmp/shell-switcher-closed.png`.

- [ ] **Step 6: Build, test, snapshot, READ.**

Run: `cargo build -p oxide-ui` → Finished; `cargo test -p oxide-ui sidebar_header` → pass; `cargo test -p oxide-ui snapshot_switcher_dark -- --ignored` (writes the PNG); `cargo clippy -p oxide-ui` → clean. You CANNOT view images — render without panic + report `/tmp/shell-switcher-closed.png` for the controller to read.

- [ ] **Step 7: Commit**
```bash
git add oxide-app/crates/oxide-ui/src/components/sidebar_header.rs oxide-app/crates/oxide-ui/Cargo.toml oxide-app/Cargo.lock
git commit -m "feat(sidebar-header): '+' icon button inline with search (replaces '+ New'); on_new prop"
```

---

### Task 3: Wire the button → `create_conversation` + full verify

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs`

**Interfaces:**
- Consumes: `SidebarHeader::on_new` (Task 2), `AppState::create_conversation` (Task 1).

- [ ] **Step 1: Wire the handler at the construction site.** In `sidebar.rs` where `SidebarHeader::new(projects, current_id)` is built (~line 55), add the `on_new` handler:
```rust
                SidebarHeader::new(projects, current_id)
                    .on_new({
                        let st = state.clone();
                        move |_| st.create_conversation()
                    })
```
(Keep any existing `.on_select(...)`/`.theme(...)` chaining intact — just add `.on_new(...)`. `state` is the `AppState` already in scope in this region.)

- [ ] **Step 2: Full build + suite + clippy.**

Run:
```
cargo build -p oxide-freya --bin oxide-freya     # Finished
cargo test -p oxide-ui -p oxide-freya            # all pass
cargo clippy -p oxide-ui -p oxide-freya -p oxide-client   # clean (note any pre-existing main_region.rs test warnings as known)
```

- [ ] **Step 3: Re-read the header snapshot** (`/tmp/shell-switcher-closed.png`) — controller confirms the "+" sits inline to the right of the search at 34×34, accent-filled. (Live click-to-create is verified later with agentd running; this slice's headless check is the layout + the green create_conversation tests from Task 1.)

- [ ] **Step 4: Commit**
```bash
git add oxide-app/crates/oxide-freya/src/regions/sidebar.rs
git commit -m "feat(sidebar): wire '+' button to AppState::create_conversation"
```

---

## Self-Review notes

- **Spec coverage:** inline "+" button + 34×34/accent/lucide-plus/a11y (T2) ✓; `on_new` prop (T2) ✓; `AppState::create_conversation` optimistic-insert+activate+err-log (T1) ✓; call-site wiring (T3) ✓; mock-based test of Ok + Err (T1, via launch_test+poll, the codebase's established spawn-test pattern) ✓; updated header unit test + dark snapshot (T2) ✓; no agentd changes ✓.
- **Type consistency:** `create_conversation(&self)`, `on_new(EventHandler<())>`, `MockTransport.create_conversation_fails`, `lucide::plus()`, `Size::flex(1.0)`, `Content::Flex` used consistently across tasks.
- **Flagged API caveats** (verify-against-source, don't guess): `TextInput.width`, `svg().color`, `Shadow::new` arg order, `EventHandler` `PartialEq`/`Copy`, a11y-alt read in `freya_testing`, `tracing` import in state.rs, `poll_n` count. Each step says "mirror the existing call / grep" rather than assume.
- **Deferred:** disabled-state when no project (early-return suffices); search wiring; rename/delete; Slices B + C.
