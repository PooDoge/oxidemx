# Conversation Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete a conversation from the sidebar (confirm only when it has content) and stop empty "New conversation" entries from stacking (reuse the existing empty one on `+`).

**Architecture:** Expose agentd's existing `delete_conversation` via a `DELETE` route; add it to the `Transport` seam (Uds + Mock); add `AppState::delete_conversation` (transport → remove from the `conversations` signal → clean `.oxide` meta → select nearest) and a reuse guard in `create_conversation`; add a reusable `ConfirmDialog` and wire a "Delete" item into the sidebar right-click menu. "Unsent" is detected purely frontend-side: a conversation has no real title until its first message is sent.

**Tech Stack:** Rust, axum (agentd), async-trait Transport, Freya (overlay), `freya_testing`.

## Global Constraints

- `cargo clippy` clean (warnings = defects), per crate touched.
- Hand-formatted: match each file's surrounding style; only format lines you add. NEVER run repo-wide `cargo fmt`.
- No gold-plating: exactly the spec, nothing extra.
- **Truthful UI:** on a transport error, do NOT optimistically mutate UI state (no removing from the list before the delete succeeds).
- Freya rule: hooks (`use_state`, etc.) are called UNCONDITIONALLY at the top of `render`.
- **Build split (critical — never mix toolchains over one `target/`):**
  - **Task 1 (agentd) builds HOST-SIDE**: from the worktree root, `CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo <cmd> -p agentd` with the rustup toolchain. No distrobox, no `-devel` libs.
  - **Tasks 2–4 (oxide-client / oxide-freya / oxide-ui) build in DISTROBOX** `claude_development`, from `oxide-app/`, `CARGO_TARGET_DIR=<dedicated reflink-warm overlay target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo <cmd>`. `oxide-client` is frontend-only (agentd does NOT depend on it) — build/test it here.
- "Unsent" detection helper signature is fixed: `is_unsent(agentd_title: &str, meta_title: Option<&str>) -> bool` returning `agentd_title.is_empty() && meta_title.is_none()`.

---

### Task 1: agentd `DELETE /v1/conversations/{id}` route

**Files:**
- Modify: `agentd/src/connector/http/routes_messaging.rs`

**Interfaces:**
- Consumes: `AgentService::delete_conversation(&self, id: &str) -> Result<(), AgentdError>` (exists, `agentd/src/interface.rs` ~845; returns `NotFound` for an unknown id).
- Produces: HTTP `DELETE /v1/conversations/{id}` → 204 on success, 404 on unknown id.

- [ ] **Step 1: Add the route.** In `router(...)`, change the `/v1/conversations/{id}` line:
```rust
        .route("/v1/conversations/{id}", get(get_conversation).delete(delete_conversation))
```
(`.delete(...)` is a `MethodRouter` combinator — no import change; `get`/`post` already imported.)

- [ ] **Step 2: Add the handler** (next to `get_conversation`):
```rust
async fn delete_conversation(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    st.svc.delete_conversation(&id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
```
(`ApiError` already wraps `AgentdError` and maps `NotFound` → 404, per the existing `get_conversation`.)

- [ ] **Step 3: Build host-side + run the agentd suite.**
```
cd /run/media/system/fastdrive/Games/mx-master-4-linux/<worktree>
CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo build -p agentd
CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo test -p agentd
CARGO_TARGET_DIR=/tmp/oxidemx-host-target cargo clippy -p agentd
```
Expected: builds; existing agentd tests pass; clippy clean. No new unit test — the route is thin wiring to the already-implemented `delete_conversation`; end-to-end delete is live-verified in Task 4. (If `routes_messaging.rs` already has a route-level test module, add a `delete_conversation` case mirroring it; otherwise do not invent a heavy harness.)

- [ ] **Step 4: Commit.**
```bash
git add agentd/src/connector/http/routes_messaging.rs
git commit -m "feat(agentd): DELETE /v1/conversations/{id} route"
```

---

### Task 2: `Transport::delete_conversation` (trait + Uds + Mock)

**Files:**
- Modify: `oxide-app/crates/oxide-client/src/transport.rs`
- Modify: `oxide-app/crates/oxide-client/src/uds.rs`
- Modify: `oxide-app/crates/oxide-client/src/mock.rs`

**Interfaces:**
- Produces: `async fn delete_conversation(&self, conversation_id: &str) -> Result<(), TransportError>` on the `Transport` trait.

- [ ] **Step 1: Add the trait method.** In `transport.rs`, inside `pub trait Transport`, after `send_message`:
```rust
    async fn delete_conversation(&self, conversation_id: &str) -> Result<(), TransportError>;
```

- [ ] **Step 2: Implement for `UdsTransport`.** In `uds.rs`, in `impl Transport for UdsTransport`, mirror `send`:
```rust
    async fn delete_conversation(&self, conversation_id: &str) -> Result<(), TransportError> {
        let (status, _) = self.send(Method::DELETE,
            &format!("/v1/conversations/{conversation_id}"), None).await?;
        if !(200..300).contains(&status) { return Err(TransportError::Http(status)); }
        Ok(())
    }
```
(`Method` is already imported — it's used as `Method::GET/POST` in this file.)

- [ ] **Step 3: Implement for `MockTransport`.** In `mock.rs`, in `impl Transport for MockTransport`, after `send_message`:
```rust
    async fn delete_conversation(&self, _conversation_id: &str) -> Result<(), TransportError> { Ok(()) }
```
(The mock fabricates conversations; `AppState` owns the frontend list the Task-3 tests assert on.)

- [ ] **Step 4: Build + clippy (distrobox).**
```
cd /run/media/system/fastdrive/Games/mx-master-4-linux/<worktree>/oxide-app
distrobox enter claude_development -- bash -lc "cd $PWD && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build -p oxide-client && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo clippy -p oxide-client"
```
Expected: builds; clippy clean. (No new test — the Uds path is exercised live in Task 4; the Mock is exercised by Task 3's AppState tests.)

- [ ] **Step 5: Commit.**
```bash
git add oxide-app/crates/oxide-client/src/transport.rs oxide-app/crates/oxide-client/src/uds.rs oxide-app/crates/oxide-client/src/mock.rs
git commit -m "feat(transport): delete_conversation (trait + Uds + Mock)"
```

---

### Task 3: AppState delete + reuse + meta (oxide-freya)

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/conversation_meta.rs`
- Modify: `oxide-app/crates/oxide-freya/src/state.rs`

**Interfaces:**
- Consumes: `Transport::delete_conversation` (Task 2); `ConversationMetaStore` (has `map: HashMap<String, ConvMeta>`, `persist()`, `load(dir, pid)`, `as_map()`, `get(id)`); `AppState` signals `conversations: State<Vec<Conversation>>`, `active: State<Option<ConversationId>>`, `conversation_meta: State<HashMap<String, ConvMeta>>`; `AppState::open_conversation`, `AppState::with_meta_store`.
- Produces: `is_unsent`, `ConversationMetaStore::remove`, `AppState::first_unsent_conversation`, `AppState::delete_conversation`; reuse guard in `create_conversation`.

- [ ] **Step 1: Write the `is_unsent` unit tests (failing).** Add to the test module in `conversation_meta.rs`:
```rust
    #[test]
    fn is_unsent_truth_table() {
        assert!(super::is_unsent("", None), "empty title + no meta = unsent");
        assert!(!super::is_unsent("Fix the bug", None), "agentd title = sent");
        assert!(!super::is_unsent("", Some("My chat")), "meta title = sent");
        assert!(!super::is_unsent("Fix", Some("My chat")), "both = sent");
    }
```

- [ ] **Step 2: Implement `is_unsent` + `ConversationMetaStore::remove`.** In `conversation_meta.rs`:
```rust
/// A conversation is "unsent" until its first message is sent: its agentd title is
/// only set then, and no user-set meta title exists. Mirrors `effective_title`'s inputs.
pub fn is_unsent(agentd_title: &str, meta_title: Option<&str>) -> bool {
    agentd_title.is_empty() && meta_title.is_none()
}
```
And add a method on `impl ConversationMetaStore` (next to `set_title`):
```rust
    /// Drop a conversation's metadata entry and persist (used when the conversation is deleted).
    pub fn remove(&mut self, id: &str) {
        self.map.remove(id);
        self.persist();
    }
```

- [ ] **Step 3: Run the helper tests (distrobox).**
```
cd .../oxide-app
distrobox enter claude_development -- bash -lc "cd $PWD && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya is_unsent"
```
Expected: PASS.

- [ ] **Step 4: Write the AppState tests (failing).** In the `state.rs` test module, using `MockTransport` (preset `conversations` with explicit titles) and the existing AppState test harness in that module (follow its `bootstrap`/construction pattern):
```rust
    // Helper convs: unsent has empty title; sent has a non-empty title.
    // (Construct via the module's existing Conversation builder/helper.)

    #[test]
    fn create_conversation_reuses_existing_unsent() {
        // Bootstrap with ONE unsent conversation already in the list.
        let st = /* build AppState over a MockTransport whose conversations = [unsent("c1")] */;
        st.create_conversation();
        // run pending spawns / flush as the existing tests do
        assert_eq!(st.conversations.peek().len(), 1, "must reuse, not grow");
        assert_eq!(st.active.peek().as_ref().map(|i| i.as_str()), Some("c1"));
    }

    #[test]
    fn create_conversation_creates_when_none_unsent() {
        let st = /* AppState over MockTransport whose conversations = [sent("c1","Title")] */;
        st.create_conversation();
        assert_eq!(st.conversations.peek().len(), 2, "no unsent → create");
    }

    #[test]
    fn delete_active_selects_nearest() {
        // list = [sent c1, sent c2, sent c3], active = c2
        let st = /* ... */;
        st.delete_conversation(ConversationId::from("c2"));
        // flush
        let ids: Vec<_> = st.conversations.peek().iter().map(|c| c.id.as_str().to_string()).collect();
        assert_eq!(ids, vec!["c1", "c3"]);
        assert_eq!(st.active.peek().as_ref().map(|i| i.as_str()), Some("c3"), "took the index → next");
    }

    #[test]
    fn delete_last_clears_active() {
        // list = [sent c1], active = c1
        let st = /* ... */;
        st.delete_conversation(ConversationId::from("c1"));
        assert!(st.conversations.peek().is_empty());
        assert!(st.active.peek().is_none());
    }
```
Note for the implementer: match the existing AppState tests' exact construction + async-flush idiom (how they await/poll `spawn`ed work). If the test harness has no flush, follow whatever the existing `create_conversation`/`send` tests in this module use. If the MockTransport `conversations` need explicit titles, set them on the preset `Conversation` values (mock `create_conversation` returns id `"mock-conv"`, title `"New"`).

- [ ] **Step 5: Implement `first_unsent_conversation`, the reuse guard, and `delete_conversation`.** In `impl AppState` (state.rs):
```rust
    /// First conversation in the list that has never had a message sent.
    fn first_unsent_conversation(&self) -> Option<ConversationId> {
        let meta = self.conversation_meta.peek();
        self.conversations.peek().iter()
            .find(|c| crate::conversation_meta::is_unsent(
                &c.title,
                meta.get(c.id.as_str()).and_then(|m| m.title.as_deref()),
            ))
            .map(|c| c.id.clone())
    }

    pub fn delete_conversation(&self, id: ConversationId) {
        let mut conversations = self.conversations;
        let mut active = self.active;
        let this = self.clone();
        let t = self.transport.clone();
        spawn(async move {
            if let Err(e) = t.delete_conversation(id.as_str()).await {
                eprintln!("delete_conversation failed: {e}");
                return; // truthful: leave UI unchanged on error
            }
            let was_active = active.peek().as_ref() == Some(&id);
            let nearest = conversations.with_mut(|mut cs| {
                let idx = cs.iter().position(|c| c.id == id);
                if let Some(i) = idx {
                    cs.remove(i);
                    // nearest = row now at i (the old next), else previous, else None
                    cs.get(i).or_else(|| i.checked_sub(1).and_then(|p| cs.get(p)))
                        .map(|c| c.id.clone())
                } else { None }
            });
            this.with_meta_store(|s| s.remove(id.as_str()));
            if was_active {
                match nearest {
                    Some(n) => this.open_conversation(n),
                    None => active.set(None),
                }
            }
        });
    }
```
And prepend the reuse guard to `create_conversation` (before `let Some(project_id) = …`):
```rust
        if let Some(existing) = self.first_unsent_conversation() {
            self.open_conversation(existing);
            return;
        }
```

- [ ] **Step 6: Run the AppState tests + clippy (distrobox).**
```
distrobox enter claude_development -- bash -lc "cd $PWD && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo clippy -p oxide-freya"
```
Expected: the 4 new tests + `is_unsent` pass; existing suite green; clippy clean.

- [ ] **Step 7: Commit.**
```bash
git add oxide-app/crates/oxide-freya/src/conversation_meta.rs oxide-app/crates/oxide-freya/src/state.rs
git commit -m "feat(state): delete_conversation + reuse-unsent + meta.remove + is_unsent"
```

---

### Task 4: ConfirmDialog + sidebar Delete wiring

**Files:**
- Create: `oxide-app/crates/oxide-ui/src/components/confirm_dialog.rs`
- Modify: `oxide-app/crates/oxide-ui/src/components/mod.rs` (re-export)
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs`

**Interfaces:**
- Consumes: `AppState::delete_conversation` (Task 3), `is_unsent` (Task 3), `ConfirmDialog` (this task).
- Produces: `ConfirmDialog` component.

- [ ] **Step 1: Write the ConfirmDialog snapshot test (failing).** Create `confirm_dialog.rs` with a test that mounts it (dark theme) and asserts the title, body, and both button labels render:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;
    #[test]
    fn confirm_dialog_renders_title_body_buttons() {
        fn app() -> impl IntoElement {
            ConfirmDialog::new(Theme::default())
                .title("Delete \"My chat\"?".to_string())
                .body("This can't be undone.".to_string())
                .confirm_label("Delete")
                .danger(true)
                .on_confirm((|()| {}).into())
                .on_cancel((|()| {}).into())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        for needle in ["My chat", "can't be undone", "Delete", "Cancel"] {
            assert!(
                t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains(needle))).is_some(),
                "ConfirmDialog must render {needle:?}"
            );
        }
    }
}
```
(Use the repo's dark-theme snapshot idiom if `launch_test` defaults to a light theme — match how other `oxide-ui` component tests construct the harness.)

- [ ] **Step 2: Implement `ConfirmDialog`.** In `confirm_dialog.rs`, a struct component (builder fields: `theme: Theme`, `title: String`, `body: String`, `confirm_label: String` default `"Confirm"`, `danger: bool`, `on_confirm: Option<EventHandler<()>>`, `on_cancel: Option<EventHandler<()>>`). `render` (hooks first, unconditional — none needed here): a `Layer::Overlay` full-window dimmed backdrop `rect` (semi-transparent bg) with `on_press` → `on_cancel` and `on_global_key_down` Escape → `on_cancel`; centered card (`rect().center()` parent) containing the title (bold), body (subtext), and a right-aligned button row: a Cancel button (→ `on_cancel`) and a Confirm button labeled `confirm_label`, tinted with `theme` danger/accent when `danger`. Follow existing `oxide-ui` button/Surface styling (reuse `Btn`/Surface style resolvers if present; otherwise plain themed `rect`+`label` with `on_press`). Keep it minimal — no animation.

- [ ] **Step 3: Re-export.** In `oxide-ui/src/components/mod.rs`, add the module + re-export:
```rust
pub mod confirm_dialog;
pub use confirm_dialog::ConfirmDialog;
```

- [ ] **Step 4: Run the snapshot test + clippy (distrobox).**
```
distrobox enter claude_development -- bash -lc "cd $PWD && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui confirm_dialog && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo clippy -p oxide-ui"
```
Expected: PASS; clippy clean.

- [ ] **Step 5: Wire Delete into the sidebar.** In `regions/sidebar.rs`:
  1. Declare a confirm state near `icon_pick` (~line 43): `let mut confirm_delete = use_state::<Option<(ConversationId, String)>>(|| None);`
  2. In the right-click `Menu` (after the "Set icon…" `MenuButton`), add a **Delete** item. Capture the row's `id`, its `effective` title, and the unsent-ness (compute `is_unsent(&conv.title, meta_title_for_this_row)` using the same meta lookup the row already does for its title/icon):
```rust
        .child(
            MenuButton::new()
                .theme(item_theme_del)
                .on_press({
                    let st = state.clone();
                    let id = id_del.clone();
                    let title = title_del.clone();
                    let unsent = unsent_del;
                    move |_: Event<PressEventData>| {
                        if unsent {
                            st.delete_conversation(id.clone());
                        } else {
                            confirm_delete.set(Some((id.clone(), title.clone())));
                        }
                    }
                })
                .child("Delete"),
        )
```
  3. Where the sidebar renders overlays (next to the `icon_pick` overlay render, ~158-191), render the dialog when `confirm_delete` is `Some`:
```rust
        .maybe_child(confirm_delete.read().clone().map(|(id, title)| {
            let st = state.clone();
            let st2 = state.clone();
            let id2 = id.clone();
            ConfirmDialog::new(th)
                .title(format!("Delete \"{title}\"?"))
                .body("This can't be undone.".to_string())
                .confirm_label("Delete")
                .danger(true)
                .on_confirm(move |()| { st.delete_conversation(id.clone()); confirm_delete.set(None); })
                .on_cancel(move |()| { let _ = &st2; let _ = &id2; confirm_delete.set(None); })
                .into_element()
        }))
```
(Adjust capture/clone names to satisfy the borrow checker; `ConversationId` and `String` are `Clone`. `is_unsent` is `crate::conversation_meta::is_unsent`. Match the file's existing import for `MenuButton`/`Event`/`PressEventData`.)

- [ ] **Step 6: Build the overlay binary + clippy + full oxide-ui/oxide-freya tests (distrobox).**
```
distrobox enter claude_development -- bash -lc "cd $PWD && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build -p oxide-freya && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo clippy -p oxide-ui -p oxide-freya && CARGO_TARGET_DIR=<overlay-target> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui -p oxide-freya"
```
Expected: `oxide-freya` binary builds; clippy clean; all tests pass.

- [ ] **Step 7: Commit.**
```bash
git add oxide-app/crates/oxide-ui/src/components/confirm_dialog.rs oxide-app/crates/oxide-ui/src/components/mod.rs oxide-app/crates/oxide-freya/src/regions/sidebar.rs
git commit -m "feat(sidebar): Delete conversation w/ confirm; reusable ConfirmDialog"
```

- [ ] **Step 8: Controller live-verify (not a subagent step).** Build host-side agentd + overlay, run both, and confirm: right-click Delete on an empty conv (instant) and a conv with messages (confirm → Cancel keeps it, Delete removes it); pressing + twice yields ONE empty conversation; deleting the open conversation lands on a neighbor; agentd actually removes the record (it stays gone after restart).

---

## Self-Review

**Spec coverage:** DELETE route (T1); Transport+Uds+Mock (T2); `is_unsent` + `ConversationMetaStore::remove` + `first_unsent_conversation` + `create_conversation` reuse + `delete_conversation` nearest-selection (T3); `ConfirmDialog` + sidebar Delete + confirm gating (T4). All spec sections mapped.

**Placeholder scan:** the only intentionally-parameterized spots are the AppState test-harness construction (`/* build AppState … */`) — flagged because the existing `state.rs` test module owns that idiom and the implementer must match it exactly rather than a guessed constructor; and the `<worktree>` / `<overlay-target>` path tokens (filled from the dispatch). No vague "handle errors / add validation" steps.

**Type consistency:** `is_unsent(&str, Option<&str>) -> bool`, `delete_conversation(&self, ConversationId)`, `ConversationMetaStore::remove(&mut self, &str)`, `ConfirmDialog::new(Theme).title(String).body(String).confirm_label(impl Into<String>).danger(bool).on_confirm(EventHandler<()>).on_cancel(EventHandler<()>)` — used identically across T3/T4. `conversation_meta` signal is `State<HashMap<String, ConvMeta>>` (`.peek().get(id)`), matching `send()` and `first_unsent_conversation`.
