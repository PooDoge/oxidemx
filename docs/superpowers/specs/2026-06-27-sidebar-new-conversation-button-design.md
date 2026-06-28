# Sidebar "+" New-Conversation Button — Design (Slice A of the UI-improvements program)

**Date:** 2026-06-27
**Branch / worktree:** new branch off `2b-collapsible-panels`
**Goal:** Replace the sidebar's full-width "+ New" stub button with a compact "+" icon button placed
inline with the search input (per the Claude Design `freya-sidebar.jsx`), AND wire it to actually
create a new conversation in the current project.

## Why / context

The current `SidebarHeader` renders a vertical stack `switcher / search / "+ New"`, where "+ New" is a
filled accent `Button` with **no `on_press`** — a pure visual stub. The Claude Design puts a compact
**34×34 "+" icon button inline to the right of the search input**. The backend for creating a
conversation **already exists end-to-end** (`Transport::create_conversation(project_id, working_dir)`
in `oxide-client/src/transport.rs:14`, implemented in `uds.rs:81`; agentd `POST /v1/conversations`),
so this slice is **purely client-side**: a layout change + a new `AppState` action + call-site wiring.

## Architecture (3 files)

### 1. `oxide-ui/src/components/sidebar_header.rs` — layout + `on_new` prop
- Add field `on_new: Option<EventHandler<()>>` + builder `on_new(impl Into<EventHandler<()>>)` (mirrors
  the existing `on_select`).
- Replace the vertical `switcher / search / new_btn` with `switcher / row`, where `row` is a
  **horizontal** `rect().content(Content::Flex).cross_align(Center).spacing(8.)` containing:
  - the existing `search` TextInput, given `Size::flex(1.0)` so it fills.
  - a **34×34 "+" icon button**: a `rect().width(px 34).height(px 34).corner_radius(9)
    .background(th.accent()).center()` with a `svg(freya_icons::lucide::plus())` sized ~17px, coloured
    `th.bg_deep()`; a subtle shadow `Shadow(0, 4, 12, 0, with_alpha(accent, 0x40))`; hover →
    `th.accent_hi()`; `a11y_role(Button)` + `a11y_alt("New conversation")`; `on_press` → call
    `on_new` if present.
- Remove the old full-width `Button` "+ New" and its `ButtonColorsThemePartial`.
- `freya_icons` is already a dep of oxide-ui? If not, add `freya-icons = { workspace = true,
  features = ["lucide"] }` (the lucide SVG submodule is already synced in the canonical Freya checkout).

### 2. `oxide-freya/src/state.rs` — `AppState::create_conversation()`
Copy the spawn pattern from `send` (state.rs:303) / `open_conversation` (state.rs:261):
```rust
/// Create a new conversation in the current project, then make it active.
/// Project comes from `current_project`; model + working_dir default server-side.
/// Transport errors are logged; the UI is left unchanged on failure.
pub fn create_conversation(&self) {
    let Some(project_id) = self.current_project.peek().clone() else { return };
    let mut conversations = self.conversations;
    let this = self.clone();                 // to reuse open_conversation after insert
    let t = self.transport.clone();
    spawn(async move {
        match t.create_conversation(project_id.as_str(), None).await {
            Ok(conv) => {
                let id = conv.id.clone();
                conversations.with_mut(|mut cs| cs.push(conv));   // optimistic insert
                this.open_conversation(id);                        // active + fresh transcript + subscribe
            }
            Err(e) => tracing::warn!("create_conversation failed: {e}"),
        }
    });
}
```
(If `AppState` isn't `Clone` or capturing `this` into the async block is awkward, capture the individual
signals — `active`, `transcript`, `transport`, `conversations` — and inline the open-conversation steps;
mirror exactly what `open_conversation` does. `State<T>` is `Copy`.)

### 3. `oxide-freya/src/regions/sidebar.rs` — wire the handler
At the `SidebarHeader::new(projects, current_id)` construction (sidebar.rs:55), add
`.on_new({ let st = state.clone(); move |_| st.create_conversation() })`.

## Data flow
```
click "+"  ──on_new()──▶  AppState::create_conversation()
   └▶ spawn: transport.create_conversation(current_project, None)  [POST /v1/conversations, agentd]
        Ok(conv) ─▶ conversations.push(conv)  +  open_conversation(conv.id)
                     └▶ active=conv.id, transcript reset, SSE subscribe  ──▶ sidebar highlights it, chat opens empty
        Err     ─▶ tracing::warn, UI unchanged
```

## Error / edge handling
- **No current project** ⇒ `create_conversation` returns early (no project to create in). The button stays
  enabled (rare state; bootstrap sets a project) — acceptable; no panic.
- **Transport error** ⇒ logged via `tracing::warn`, no optimistic insert (insert happens only on `Ok`),
  UI unchanged. (Truthfulness rule 1: we only insert state we got back from the transport.)
- The "+" button is `interactive`/pressable; it does not capture beyond its 34×34 box.

## Testing
- **Unit** (`sidebar_header.rs`): the existing `sidebar_header_renders_project_and_new` test asserts a
  "New" label — update it to assert the "+" button is present via its a11y alt "New conversation" (the
  literal "+ New" label is gone). Keep the project-name + "Search…" assertions.
- **Unit** (`state.rs` or a new test): `create_conversation` against a **mock `Transport`** (the
  `Arc<dyn Transport>` seam): a mock returning a fixed `Conversation` ⇒ assert it lands in
  `conversations` and `active` becomes its id; a mock returning `Err` ⇒ assert `conversations`/`active`
  unchanged. (If no mock Transport exists yet, add a minimal one in the test module implementing the
  trait with canned responses.)
- **Dark snapshot** (extend `snapshot_switcher_dark`): the header now shows `switcher` + the
  `[search | +]` inline row → `/tmp/shell-switcher-closed.png`; controller reads it to confirm the "+"
  sits inline right of the search at 34×34.
- **Live** (after green): click "+" with agentd running → a new conversation appears in the list + becomes
  the active (empty) chat.

## File structure
| File | Responsibility |
|------|----------------|
| `oxide-ui/src/components/sidebar_header.rs` | inline search + "+" icon button; `on_new` prop; drop "+ New" |
| `oxide-freya/src/state.rs` | `AppState::create_conversation()` (spawn → create → optimistic insert → activate) |
| `oxide-freya/src/regions/sidebar.rs` | pass `on_new(|_| state.create_conversation())` |

## Out of scope (other slices in this program)
- B — menu open/close animation (Select-style) in the shared Popover/MenuSurface.
- C — menu interrupt-closure (toggles/radio/submenu keep the menu open).
- Conversation rename/delete; project creation; search wiring (the search box stays non-functional, as today).
- Disabling the "+" while no project is selected (early-return is enough; a disabled state is a later polish).
