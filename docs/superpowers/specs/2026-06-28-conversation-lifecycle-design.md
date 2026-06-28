# Conversation Lifecycle (delete + no-stacked-empties) — Design

**Date:** 2026-06-28
**Branch / worktree:** new branch off `2b-collapsible-panels`
**Goal:** Let the user delete a conversation from the sidebar (with an "Are you sure?" guard for
conversations that have content), and stop multiple empty "New conversation" entries from stacking
(reuse the existing empty one instead of creating another).

## Behavior

1. **Delete** — right-click a conversation row → **Delete**.
   - If the conversation is **unsent/empty** (nothing was ever sent), delete immediately.
   - If it **has content**, show a confirm modal: *Delete "&lt;title&gt;"? This can't be undone.*
     `[Cancel] [Delete]`. Only **Delete** removes it.
   - After deleting the **currently-open** conversation, select the **nearest** one (the row that
     took its index; else the previous; else clear to no active conversation).
   - Deleting removes the agentd record AND the conversation's frontend `.oxide` meta (title/icon).
2. **No stacked empties** — pressing **+** (new conversation), if an **unsent** conversation already
   exists in the list, **opens that one** instead of creating another. Guarantees at most one empty.

## "Unsent / empty" signal (frontend, no agentd change)

A conversation's `title` is only set when the first user message is sent (auto-derived in
`AppState::send`). So **unsent ⟺ no real title**: empty agentd `title` AND no `.oxide` meta title.
Pure helper (mirrors `effective_title`):
```rust
// conversation_meta.rs
pub fn is_unsent(agentd_title: &str, meta_title: Option<&str>) -> bool {
    agentd_title.is_empty() && meta_title.is_none()
}
```
Edge: renaming an empty conversation without sending masks it as "sent" — acceptable (intentional act).

## Components & changes

### A. agentd — expose delete (the service method already exists)

`AgentService::delete_conversation(id)` exists (`agentd/src/interface.rs`); it is just unrouted.
- `agentd/src/connector/http/routes_messaging.rs`: change
  `.route("/v1/conversations/{id}", get(get_conversation))` to
  `.route("/v1/conversations/{id}", get(get_conversation).delete(delete_conversation))`
  and add the handler:
  ```rust
  async fn delete_conversation(
      State(st): State<AppState>,
      AxPath(id): AxPath<String>,
  ) -> Result<axum::http::StatusCode, ApiError> {
      st.svc.delete_conversation(&id).await?;
      Ok(axum::http::StatusCode::NO_CONTENT)
  }
  ```
  (`.delete(...)` is a `MethodRouter` method — no new import.) Returns 204 on success; the existing
  `delete_conversation` already returns `NotFound` for a missing id → maps through `ApiError`.

### B. Transport seam (oxide-client)

- `transport.rs`: add to the trait
  `async fn delete_conversation(&self, conversation_id: &str) -> Result<(), TransportError>;`
- `uds.rs` (`UdsTransport`): mirror the existing `send` pattern —
  ```rust
  async fn delete_conversation(&self, conversation_id: &str) -> Result<(), TransportError> {
      let (status, _) = self.send(Method::DELETE,
          &format!("/v1/conversations/{conversation_id}"), None).await?;
      if !(200..300).contains(&status) { return Err(TransportError::Http(status)); }
      Ok(())
  }
  ```
- `mock.rs` (`MockTransport`): `async fn delete_conversation(&self, _id: &str) -> Result<(), TransportError> { Ok(()) }`
  (the mock fabricates conversations; AppState owns the frontend list, which the tests assert on).

### C. AppState (oxide-freya/src/state.rs)

- **`ConversationMetaStore::remove(&mut self, id: &str)`** (conversation_meta.rs): drop the map entry
  and `persist()` (atomic write, same as `set_title`).
- **`first_unsent_conversation(&self) -> Option<ConversationId>`**: scan `conversations.peek()` with
  `is_unsent(&c.title, meta.get(&c.id.0).and_then(|m| m.title.as_deref()))`; return the first match's id.
- **`create_conversation`**: at the top, `if let Some(existing) = self.first_unsent_conversation() { self.open_conversation(existing); return; }` — then the existing create-via-transport path.
- **`delete_conversation(&self, id: ConversationId)`**: spawn → `transport.delete_conversation(id)`;
  on Ok: record whether it was active, remove it from the `conversations` signal, compute the nearest
  id (the row now at the removed index, else previous, else None), `with_meta_store(|m| m.remove(id))`,
  and if it was active either `open_conversation(nearest)` or `active.set(None)`. On Err: `eprintln!`
  and leave state unchanged (truthful: no optimistic removal).

### D. ConfirmDialog (oxide-ui) + sidebar wiring

- **`oxide-ui/src/components/confirm_dialog.rs`** — a small reusable modal (no such primitive exists
  today; the icon-picker is an ad-hoc inline overlay). Builder:
  `ConfirmDialog::new(theme).title(String).body(String).confirm_label("Delete").danger(true)
   .on_confirm(EventHandler<()>).on_cancel(EventHandler<()>)`. Renders a `Layer::Overlay` dimmed
  backdrop (click = cancel) + a centered card with title, body, and `[Cancel] [Confirm]` buttons
  (confirm tinted danger when `danger`). Escape = cancel. Re-export from `components/mod.rs`.
- **`oxide-freya/src/regions/sidebar.rs`**: in the existing right-click `Menu` (Rename / Set icon…),
  add a **Delete** `MenuButton`. On press: if `is_unsent(&conv.title, meta_title)` →
  `state.delete_conversation(id)` immediately; else set a `confirm_delete: State<Option<(ConversationId, String)>>`
  (declared like `icon_pick`). Render `ConfirmDialog` when `confirm_delete` is `Some` →
  `on_confirm`: `state.delete_conversation(id)` + clear; `on_cancel`: clear.

## Data flow
```
+ press ─▶ create_conversation ─▶ first_unsent? ─ yes ─▶ open it (no new record)
                                              └─ no ──▶ transport.create ─▶ push + open
right-click Delete ─▶ unsent? ─ yes ─▶ delete_conversation (immediate)
                              └─ no ─▶ confirm_delete=Some ─▶ ConfirmDialog ─▶ Delete ─▶ delete_conversation
delete_conversation ─▶ transport.delete ─Ok─▶ remove from signal + meta.remove + (if active) select nearest
```

## Error handling
- Transport delete failure → `eprintln!`, no state change (no optimistic UI; list stays truthful).
- Deleting a non-active conversation → no selection change.
- Deleting the last conversation → `active = None` (empty chat state); the next **+** creates fresh.
- `delete` of an id agentd doesn't know → 404 → `TransportError::Http(404)` → logged, no-op.

## Testing
- **Unit (pure):** `is_unsent` truth table (empty+no-meta = true; titled = false; meta-titled = false).
- **Unit (MockTransport + AppState):**
  - `create_conversation` with a preset unsent conversation in the list opens it and does NOT grow
    the list; with no unsent one, it creates (list grows by 1).
  - `delete_conversation` removes the target from the `conversations` signal; deleting the active one
    selects the nearest (assert `active` becomes the expected neighbor); deleting the last clears `active`.
  - `ConversationMetaStore::remove` drops the entry + persists (load-back shows it gone).
- **Live (controller):** right-click Delete on an empty conv (instant) and a conv with messages
  (confirm → Cancel keeps it, Delete removes it); pressing + twice yields one empty conversation;
  deleting the open conversation lands on a neighbor.

## File structure
| File | Responsibility |
|------|----------------|
| `agentd/src/connector/http/routes_messaging.rs` | `DELETE /v1/conversations/{id}` route + handler |
| `oxide-client/src/transport.rs` | `Transport::delete_conversation` trait method |
| `oxide-client/src/uds.rs` | `UdsTransport` HTTP DELETE impl |
| `oxide-client/src/mock.rs` | `MockTransport` impl (Ok) |
| `oxide-freya/src/conversation_meta.rs` | `is_unsent` helper + `ConversationMetaStore::remove` |
| `oxide-freya/src/state.rs` | `first_unsent_conversation`, `create_conversation` reuse, `delete_conversation` |
| `oxide-ui/src/components/confirm_dialog.rs` | reusable `ConfirmDialog` modal (+ mod.rs re-export) |
| `oxide-freya/src/regions/sidebar.rs` | Delete menu item + `confirm_delete` state + ConfirmDialog render |

## Decomposition / sequencing (4 tasks)
1. **agentd DELETE route** (+ existing-method wiring). Builds host-side.
2. **Transport seam**: trait method + `UdsTransport` + `MockTransport`.
3. **AppState + meta**: `is_unsent`, `ConversationMetaStore::remove`, `first_unsent_conversation`,
   `create_conversation` reuse, `delete_conversation` (+ unit tests against MockTransport).
4. **UI**: `ConfirmDialog` component + sidebar Delete menu item + confirm gating + render. Final live verify.

## Build notes
- **agentd builds host-side** (rustup, `CARGO_TARGET_DIR=/tmp/oxidemx-host-target`); the **overlay**
  (oxide-freya/oxide-ui) builds in the `claude_development` distrobox with its own target. Never mix
  toolchains over one `target/`.
- `oxide-client` is shared; it builds in both — build it where the consumer under test builds.

## Out of scope
- Bulk delete / multi-select; archive (vs delete); undo.
- An `X`-on-hover affordance (right-click Delete only, per decision).
- Cleaning up pre-existing duplicate empties already in the list (the reuse rule prevents NEW stacking).
- Renaming the "New conversation" default label.
