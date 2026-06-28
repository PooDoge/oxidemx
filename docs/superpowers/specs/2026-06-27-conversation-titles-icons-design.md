# Conversation Titles & Icons — Design (UI-improvements program)

**Date:** 2026-06-27
**Branch / worktree:** new branch off `2b-collapsible-panels`
**Goal:** Give each conversation a frontend-owned, editable **title** and **icon**, persisted to a
per-project file in `.oxide/`. Kills the blank-title bug (new chats show no title) entirely client-side,
and adds right-click **Rename** + **Set icon**. No agentd / HTTP / DTO / Transport changes.

## Why frontend-file-based

agentd *does* set a title (first user message, first 60 chars) but the client never re-fetches, so the
optimistically-inserted `title: ""` goes stale; there is no title-update SSE event. Rather than add
backend plumbing (rename HTTP route + Transport method + an `icon` DTO field), we make titles + icons
**frontend-owned**, stored in `<project>/.oxide/conversation-meta.json`. This fixes the bug with zero
round-trips and is trivially liftable into agentd later (it is already a clean per-conversation record).

## Architecture

### 1. `conversation_meta` store — `oxide-freya/src/conversation_meta.rs` (new)
```rust
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConvMeta { pub title: Option<String>, pub icon: Option<String> } // icon = lucide name

/// In-memory map (conversation_id → ConvMeta) backed by a per-project JSON file.
pub struct ConversationMetaStore { dir: PathBuf, map: HashMap<String, ConvMeta> }

impl ConversationMetaStore {
    pub fn load(project_dir: &str) -> Self;             // reads <project_dir>/.oxide/conversation-meta.json; missing/corrupt → empty map (never fatal)
    pub fn get(&self, id: &str) -> Option<&ConvMeta>;
    pub fn set_title(&mut self, id: &str, title: String);   // updates map + persists
    pub fn set_icon(&mut self, id: &str, icon: String);     // updates map + persists
    fn path(project_dir: &str) -> PathBuf;             // <project_dir>/.oxide/conversation-meta.json
    fn persist(&self);                                  // create .oxide dir if needed; atomic write (tmp+rename); errors logged, non-fatal
}
```
- Pure parts (path build, (de)serialize, title-derivation rule) are unit-testable without the filesystem.
- File format: a flat JSON object `{ "<conv_id>": { "title": "...", "icon": "terminal" } }`. Entries with
  both fields `None` are pruned on persist.

### 2. `AppState` wiring — `oxide-freya/src/state.rs`
- Add `pub conversation_meta: State<HashMap<String, ConvMeta>>` (signal holding the loaded map) and a
  way to reach the current project's dir (the `projects`/`current_project` already in state give
  `default_working_dir`).
- Load the map on `bootstrap` and `open_project` (right after `list_conversations`), via
  `ConversationMetaStore::load(project_default_working_dir)`.
- **Auto-title** in `AppState::send`: when the active conversation has **no prior user turn** (its
  transcript has no user turns yet) AND `conversation_meta` has no title for it, derive
  `title = message.trim().chars().take(60).collect()` (skip if empty), write it via a store `set_title`,
  and update the `conversation_meta` signal. This is the bug fix — runs on the first send, frontend-owned.
- New mutating helpers mirror `create_conversation`'s shape (signals are `Copy`; persist on a blocking
  call or `spawn` — file writes are tiny, a synchronous `set_title`/`set_icon` + persist is acceptable;
  if it ever blocks, move persist into `spawn`).

### 3. Display merge — `oxide-freya/src/regions/sidebar.rs`
A small helper, used in BOTH the expanded list and collapsed rail:
```rust
fn effective_title(c: &Conversation, meta: &HashMap<String, ConvMeta>) -> String  // meta.title → c.title (if non-empty) → "New conversation"
fn effective_icon(c: &Conversation, meta: &HashMap<String, ConvMeta>) -> &'static str  // meta.icon (validated lucide name) → DEFAULT_ICON ("message-square")
```
- Expanded rows render the icon (lucide svg) left of the title; collapsed rail renders the icon in the
  28×28 button (replacing `StatusDot`). The icon name maps to a lucide fn via a small `icon_svg(name)`
  lookup over the **curated set only** (unknown names → default), so we never call a missing lucide fn.

### 4. Edit affordances — right-click → `open_context_menu`
- Each conversation row gets `.on_secondary_down(move |e| open_context_menu(&e, conversation_menu(...)))`.
- `conversation_menu` is a Freya `Menu` with: **Rename**, **Set icon…**. (A `Delete` item is out of scope
  but this menu is its future home.)
- **Rename:** selecting it flips a per-row `editing: State<Option<ConvId>>` to that conversation; the row
  swaps its title label for a `TextInput` seeded with the current title; commit on Enter/blur →
  `state.rename_conversation(id, text)` (store `set_title` + signal update); Escape cancels.
- **Set icon…:** opens an icon-picker `Popover`/menu anchored to the row: a grid of the curated lucide
  icons (`CURATED_ICONS: [&str; N]`); clicking one → `state.set_conversation_icon(id, name)`.

### 5. Curated icon set (`conversation_meta.rs` or a `ui` const)
`CURATED_ICONS = ["message-square","terminal","code","search","folder","bug","sparkles","git-branch",
"flask-conical","book","zap","pin","bot","wrench","file-text","globe"]` (verify each exists in the lucide
submodule; drop any that don't). `DEFAULT_ICON = "message-square"`.

## Data flow
```
open project ──▶ ConversationMetaStore::load(project_dir) ──▶ conversation_meta signal
sidebar render ──▶ effective_title/effective_icon(conv, meta) ──▶ row shows title + lucide icon
send first msg ──▶ derive title (≤60 chars) ──▶ store.set_title + persist ──▶ signal update ──▶ row retitles
right-click row → Rename ──▶ inline TextInput ──▶ commit ──▶ store.set_title + persist + signal
right-click row → Set icon ──▶ lucide grid popover ──▶ pick ──▶ store.set_icon + persist + signal
```

## Error / edge handling
- Missing/corrupt `conversation-meta.json` ⇒ empty map (logged, never fatal); first write recreates it.
- Empty `default_working_dir` (possible per the DTO) ⇒ fall back to
  `~/.config/oxidemx/projects/<project_id>/conversation-meta.json` (a deterministic per-project path) so
  persistence still works; document the chosen path. (Primary = project `.oxide/`.)
- Unknown/old icon name in the file ⇒ `icon_svg` returns the default; no panic.
- Auto-title never overwrites an existing `meta.title` (user rename wins) and never fires on an empty
  message.
- Atomic write (temp file + rename) so a crash mid-write can't corrupt the JSON.

## Testing
- **Unit** (`conversation_meta.rs`): `path()` builds `<dir>/.oxide/conversation-meta.json`;
  serialize→deserialize round-trip; `set_title`/`set_icon` update the map; load of missing file → empty;
  load of corrupt JSON → empty; the auto-title derivation (≤60 chars, trims, skips empty); `effective_title`
  fallback order; `effective_icon` validates against the curated set.
- **Dark snapshots** (render + read-back): a sidebar list with mixed (overridden title + icon) and
  (fallback) conversations; the icon-picker grid; a row in rename-edit mode.
- **Live**: send first message → title appears; right-click → Rename → persists; Set icon → glyph shows
  in expanded + collapsed; restart the app → title + icon survive (file round-trip).
- Headless can't drive the context menu / file across restart fully — those are live-verified; the store +
  merge logic is unit-tested.

## File structure
| File | Responsibility |
|------|----------------|
| `oxide-freya/src/conversation_meta.rs` (new) | `ConvMeta`, `ConversationMetaStore` (load/get/set/persist), `CURATED_ICONS`, `DEFAULT_ICON`, `icon_svg`, auto-title derive helper |
| `oxide-freya/src/state.rs` | `conversation_meta` signal; load on bootstrap/open_project; auto-title in `send`; `rename_conversation` + `set_conversation_icon` helpers |
| `oxide-freya/src/regions/sidebar.rs` | `effective_title`/`effective_icon` merge; render icons; right-click menu; inline rename; icon-picker popover |

## Decomposition / sequencing (~4 tasks)
1. `conversation_meta.rs` store + curated icons + `icon_svg` + unit tests.
2. `AppState` wiring (signal, load, auto-title in `send`) + sidebar display merge (titles + icons in expanded + collapsed). **← fixes the blank-title bug.**
3. Right-click context menu → **Rename** (inline editor).
4. **Set icon…** picker popover + final verify.

## Out of scope
- Delete/close conversation (the context menu is its future home).
- Persisting titles/icons to agentd (deliberately frontend-only for now; file format is lift-ready).
- AI-generated smart titles (first-message title is the chosen behavior).
- Slices B (menu animation) + C (menu interrupt-closure) of the broader program.
