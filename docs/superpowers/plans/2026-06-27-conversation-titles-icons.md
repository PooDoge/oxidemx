# Conversation Titles & Icons Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Frontend-owned, editable conversation titles + icons persisted to `<project>/.oxide/conversation-meta.json`; fixes the blank-title bug client-side; adds right-click Rename + lucide icon picker.

**Architecture:** A new `conversation_meta` store (per-project JSON file) loaded into an `AppState` signal; the sidebar merges overrides onto the agentd conversation list (title fallback + icon); `send()` auto-derives a title on the first message; right-click opens a context menu for Rename (inline editor) + Set-icon (lucide grid popover). NO agentd/HTTP/DTO/Transport changes.

**Tech Stack:** Rust, Freya blog/0.4 (`rect`/`svg`/`label`/`TextInput`/`Popover`/`Menu`/`open_context_menu`), `serde`/`serde_json`, `std::fs`, `freya_icons::lucide`, `freya_testing`.

## Global Constraints

- **No backend changes.** Titles + icons live only in `<project default_working_dir>/.oxide/conversation-meta.json` (flat JSON: `{ "<conv_id>": { "title": "...", "icon": "terminal" } }`). Liftable to agentd later.
- Empty `default_working_dir` ⇒ fall back to `~/.config/oxidemx/projects/<project_id>/conversation-meta.json` (use `std::env::var("HOME")`; if unavailable, `std::env::temp_dir()`). Primary path is the project `.oxide/`.
- Curated icons (all verified present in the lucide submodule): `["message-square","terminal","code","search","folder","bug","sparkles","git-branch","flask-conical","book","zap","pin","bot","wrench","file-text","globe"]`. `DEFAULT_ICON = "message-square"`. lucide fn names are snake_case of the kebab name (`git-branch` → `git_branch`).
- Title fallback order: `meta.title` → agentd `conversation.title` (if non-empty) → `"New conversation"`. Auto-title = first user message, `.trim()`, `.chars().take(60)`, skip if empty, never overwrites an existing `meta.title`.
- Persist = create `.oxide` dir if needed + atomic write (temp file + rename). All file errors logged (`eprintln!`, no `tracing` dep in oxide-freya) and **non-fatal**.
- `oxide-ui` must NOT depend on `oxide-freya`. The store + state live in `oxide-freya`; the sidebar already imports `oxide_ui` components.
- Rust quality (CLAUDE.md Rule 2): `cargo clippy` clean; hand-formatted (only added lines; no repo-wide fmt); borrow over clone; `?`/error-handling over unwrap in non-test code; no gold-plating.
- Spawn/signals: `State<T>` is `Copy`; `AppState` is `#[derive(Clone)]`. File writes are tiny — synchronous persist in the helper is acceptable.

**Run every cargo command as:**
```
distrobox enter claude_development -- bash -lc 'cd <WORKTREE>/oxide-app && CARGO_TARGET_DIR=<WARM_TARGET> LIBRARY_PATH=/tmp/oxidemx-lib-links cargo <args>'
```

---

### Task 1: `conversation_meta` store + curated icons + tests

**Files:**
- Create: `oxide-app/crates/oxide-freya/src/conversation_meta.rs`
- Modify: `oxide-app/crates/oxide-freya/src/main.rs` (add `pub mod conversation_meta;`) — or the crate's `lib.rs`/module root (grep `pub mod` in main.rs and mirror).

**Interfaces:**
- Produces: `ConvMeta { title: Option<String>, icon: Option<String> }`; `ConversationMetaStore` with `load(project_dir: &str, project_id: &str) -> Self`, `get(&self, id: &str) -> Option<&ConvMeta>`, `as_map(&self) -> &HashMap<String, ConvMeta>`, `set_title(&mut self, id: &str, title: String)`, `set_icon(&mut self, id: &str, icon: String)`; free fns `pub fn derive_title(msg: &str) -> Option<String>`, `pub fn effective_title(meta_title: Option<&str>, agentd_title: &str) -> String`, `pub fn effective_icon(meta_icon: Option<&str>) -> &'static str`, `pub fn icon_svg(name: &str) -> &'static [u8]`; consts `CURATED_ICONS: [&str; 16]`, `DEFAULT_ICON`.

- [ ] **Step 1: Write the failing tests** (`conversation_meta.rs` `#[cfg(test)] mod tests`):
```rust
#[test]
fn derive_title_trims_caps_and_skips_empty() {
    assert_eq!(super::derive_title("  hello world  "), Some("hello world".to_string()));
    assert_eq!(super::derive_title("   "), None);
    let long = "x".repeat(80);
    assert_eq!(super::derive_title(&long).unwrap().chars().count(), 60);
}
#[test]
fn effective_title_fallback_order() {
    assert_eq!(super::effective_title(Some("Override"), "agentd"), "Override");
    assert_eq!(super::effective_title(None, "agentd"), "agentd");
    assert_eq!(super::effective_title(None, ""), "New conversation");
}
#[test]
fn effective_icon_validates_against_curated_set() {
    assert_eq!(super::effective_icon(Some("terminal")), "terminal");
    assert_eq!(super::effective_icon(Some("not-a-real-icon")), super::DEFAULT_ICON);
    assert_eq!(super::effective_icon(None), super::DEFAULT_ICON);
}
#[test]
fn store_roundtrip_in_tempdir() {
    let dir = std::env::temp_dir().join(format!("oxide-meta-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dirs = dir.to_string_lossy().to_string();
    let mut s = super::ConversationMetaStore::load(&dirs, "proj");
    s.set_title("c1", "My chat".into());
    s.set_icon("c1", "terminal".into());
    let reloaded = super::ConversationMetaStore::load(&dirs, "proj");
    let m = reloaded.get("c1").unwrap();
    assert_eq!(m.title.as_deref(), Some("My chat"));
    assert_eq!(m.icon.as_deref(), Some("terminal"));
    let _ = std::fs::remove_dir_all(&dir);
}
#[test]
fn load_missing_or_corrupt_is_empty_not_fatal() {
    let s = super::ConversationMetaStore::load("/nonexistent/path/xyz", "proj");
    assert!(s.get("any").is_none());
}
#[test]
fn icon_svg_never_panics_for_curated_or_unknown() {
    for n in super::CURATED_ICONS { assert!(!super::icon_svg(n).is_empty()); }
    assert!(!super::icon_svg("unknown").is_empty()); // falls back to default
}
```

- [ ] **Step 2: Run tests → fail** (`cargo test -p oxide-freya conversation_meta` → not-found).

- [ ] **Step 3: Implement `conversation_meta.rs`:**
```rust
//! Frontend-owned conversation titles + icons, persisted per project to
//! `<project>/.oxide/conversation-meta.json`. No backend involvement.
use std::collections::HashMap;
use std::path::PathBuf;

use freya_icons::lucide;
use serde::{Deserialize, Serialize};

pub const DEFAULT_ICON: &str = "message-square";
pub const CURATED_ICONS: [&str; 16] = [
    "message-square","terminal","code","search","folder","bug","sparkles","git-branch",
    "flask-conical","book","zap","pin","bot","wrench","file-text","globe",
];

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConvMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

pub struct ConversationMetaStore {
    path: PathBuf,
    map: HashMap<String, ConvMeta>,
}

impl ConversationMetaStore {
    pub fn load(project_dir: &str, project_id: &str) -> Self {
        let path = Self::path(project_dir, project_id);
        let map = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<String, ConvMeta>>(&s).ok())
            .unwrap_or_default();
        Self { path, map }
    }
    pub fn get(&self, id: &str) -> Option<&ConvMeta> { self.map.get(id) }
    pub fn as_map(&self) -> &HashMap<String, ConvMeta> { &self.map }
    pub fn set_title(&mut self, id: &str, title: String) {
        self.map.entry(id.to_string()).or_default().title = Some(title);
        self.persist();
    }
    pub fn set_icon(&mut self, id: &str, icon: String) {
        self.map.entry(id.to_string()).or_default().icon = Some(icon);
        self.persist();
    }
    fn path(project_dir: &str, project_id: &str) -> PathBuf {
        if !project_dir.trim().is_empty() {
            return PathBuf::from(project_dir).join(".oxide").join("conversation-meta.json");
        }
        let base = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
        base.join(".config").join("oxidemx").join("projects").join(project_id).join("conversation-meta.json")
    }
    fn persist(&self) {
        // prune empty entries
        let pruned: HashMap<_, _> = self.map.iter()
            .filter(|(_, m)| m.title.is_some() || m.icon.is_some())
            .map(|(k, v)| (k.clone(), v.clone())).collect();
        if let Some(parent) = self.path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) { eprintln!("conversation_meta: mkdir failed: {e}"); return; }
        }
        let tmp = self.path.with_extension("json.tmp");
        match serde_json::to_string_pretty(&pruned) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&tmp, json) { eprintln!("conversation_meta: write failed: {e}"); return; }
                if let Err(e) = std::fs::rename(&tmp, &self.path) { eprintln!("conversation_meta: rename failed: {e}"); }
            }
            Err(e) => eprintln!("conversation_meta: serialize failed: {e}"),
        }
    }
}

pub fn derive_title(msg: &str) -> Option<String> {
    let t = msg.trim();
    if t.is_empty() { return None; }
    Some(t.chars().take(60).collect())
}

pub fn effective_title(meta_title: Option<&str>, agentd_title: &str) -> String {
    if let Some(t) = meta_title { if !t.is_empty() { return t.to_string(); } }
    if !agentd_title.is_empty() { return agentd_title.to_string(); }
    "New conversation".to_string()
}

pub fn effective_icon(meta_icon: Option<&str>) -> &'static str {
    match meta_icon {
        Some(name) => CURATED_ICONS.iter().copied().find(|&c| c == name).unwrap_or(DEFAULT_ICON),
        None => DEFAULT_ICON,
    }
}

/// Curated-name → lucide SVG bytes; unknown → default. Never panics.
pub fn icon_svg(name: &str) -> &'static [u8] {
    match effective_icon(Some(name)) {
        "message-square" => lucide::message_square(),
        "terminal" => lucide::terminal(),
        "code" => lucide::code(),
        "search" => lucide::search(),
        "folder" => lucide::folder(),
        "bug" => lucide::bug(),
        "sparkles" => lucide::sparkles(),
        "git-branch" => lucide::git_branch(),
        "flask-conical" => lucide::flask_conical(),
        "book" => lucide::book(),
        "zap" => lucide::zap(),
        "pin" => lucide::pin(),
        "bot" => lucide::bot(),
        "wrench" => lucide::wrench(),
        "file-text" => lucide::file_text(),
        "globe" => lucide::globe(),
        _ => lucide::message_square(),
    }
}
```
(Verify the lucide fn return type — if it returns `&str`/`String`/`Vec<u8>` rather than `&'static [u8]`, adjust `icon_svg`'s return type to match what `svg(...)` accepts; grep `svg(lucide::` in regions/context/mod.rs for the exact type. `freya-icons` is already a dep of oxide-freya.)

- [ ] **Step 4: Register module** in `main.rs` (`pub mod conversation_meta;`, alphabetical with the others).

- [ ] **Step 5: Run tests → pass; clippy.**

Run: `cargo test -p oxide-freya conversation_meta` → all pass; `cargo clippy -p oxide-freya` → clean.

- [ ] **Step 6: Commit**
```bash
git add oxide-app/crates/oxide-freya/src/conversation_meta.rs oxide-app/crates/oxide-freya/src/main.rs
git commit -m "feat(conversation-meta): per-project .oxide title/icon store + curated lucide set + helpers"
```

---

### Task 2: `AppState` wiring — signal, load, auto-title, edit helpers

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/state.rs`

**Interfaces:**
- Consumes: `conversation_meta::{ConvMeta, ConversationMetaStore, derive_title}`.
- Produces: `AppState.conversation_meta: State<HashMap<String, ConvMeta>>`; `AppState::reload_conversation_meta(&self)` (loads the store for the current project into the signal); auto-title call in `send`; `AppState::rename_conversation(&self, id: &str, title: String)`; `AppState::set_conversation_icon(&self, id: &str, icon: String)`.

- [ ] **Step 1: Add the signal + field init.** In the `AppState` struct add `pub conversation_meta: State<HashMap<String, ConvMeta>>,`; in `AppState::new` initialise `conversation_meta: State::new_in_scope(HashMap::new(), ...)` — mirror EXACTLY how the other `State` fields are constructed in `new` (grep `AppState::new` / `use_state` there; use the same constructor the sibling signals use). Update the custom `PartialEq` for `AppState` if it enumerates fields (add `self.conversation_meta == other.conversation_meta`).

- [ ] **Step 2: Add a loader + call it on bootstrap/open_project.** Add:
```rust
    /// Load the current project's title/icon overrides into the signal.
    pub fn reload_conversation_meta(&self) {
        let (dir, pid) = {
            let cur = self.current_project.peek().clone();
            let projects = self.projects.peek().clone();
            match cur.and_then(|id| projects.iter().find(|p| p.id == id).cloned()) {
                Some(p) => (p.default_working_dir.clone(), p.id.0.clone()),
                None => return,
            }
        };
        let store = crate::conversation_meta::ConversationMetaStore::load(&dir, &pid);
        let mut sig = self.conversation_meta;
        sig.set(store.as_map().clone());
    }
```
Call `self.reload_conversation_meta()` at the end of `bootstrap` (after conversations load) and in `open_project` (after the conversation list refresh). (Match the exact spots where `list_conversations` results are applied; the project must be set first.)

- [ ] **Step 3: Auto-title in `send`.** `send` currently (state.rs:324):
```rust
    pub fn send(&self, text: String, attachments: Vec<AttachmentPayload>) {
        let Some(id) = self.active.peek().clone() else { return };
        let mut transcript = self.transcript;
        transcript.with_mut(|mut tx| tx.apply_user(text.clone()));
        ...
```
Insert BEFORE `apply_user`:
```rust
        let is_first_user_turn = !self.transcript.peek().turns.iter().any(|t| t.role == "user");
        let has_title = self.conversation_meta.peek().get(id.as_str()).and_then(|m| m.title.as_ref()).is_some();
        if is_first_user_turn && !has_title {
            if let Some(title) = crate::conversation_meta::derive_title(&text) {
                self.rename_conversation(id.as_str(), title);
            }
        }
```

- [ ] **Step 4: Add the edit helpers** (persist via the store, then refresh the signal):
```rust
    pub fn rename_conversation(&self, id: &str, title: String) {
        self.with_meta_store(|s| s.set_title(id, title));
    }
    pub fn set_conversation_icon(&self, id: &str, icon: String) {
        self.with_meta_store(|s| s.set_icon(id, icon));
    }
    fn with_meta_store(&self, f: impl FnOnce(&mut crate::conversation_meta::ConversationMetaStore)) {
        let (dir, pid) = {
            let cur = self.current_project.peek().clone();
            let projects = self.projects.peek().clone();
            match cur.and_then(|id| projects.iter().find(|p| p.id == id).cloned()) {
                Some(p) => (p.default_working_dir.clone(), p.id.0.clone()),
                None => return,
            }
        };
        let mut store = crate::conversation_meta::ConversationMetaStore::load(&dir, &pid);
        f(&mut store);
        let mut sig = self.conversation_meta;
        sig.set(store.as_map().clone());
    }
```
(Loading the store fresh each edit avoids holding a `ConversationMetaStore` in `AppState`; the file is tiny. If you prefer holding it, that's a larger refactor — keep this minimal version.)

- [ ] **Step 5: Test the auto-title path** (extend the `launch_test`+`poll_n` pattern; reuse the `create_conversation` test style from state.rs). A `MockTransport` with a project (`default_working_dir = a temp dir`) + a created conversation; drive `create_conversation` then `send("hello there")`; poll; assert `state.conversation_meta.read()` has a title `"hello there"` for the conv id. (If wiring a project's working_dir through the mock is heavy, instead unit-test the decision inline: assert that after `send` on a fresh transcript, `conversation_meta` gains the derived title — keep it a real assertion, not tautological.)

- [ ] **Step 6: clippy + commit**

Run: `cargo test -p oxide-freya` → pass; `cargo clippy -p oxide-freya` → clean.
```bash
git add oxide-app/crates/oxide-freya/src/state.rs
git commit -m "feat(state): conversation_meta signal + load on project + auto-title on first send + rename/set-icon helpers"
```

---

### Task 3: Sidebar display merge — titles + icons (expanded + collapsed)

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs`

**Interfaces:**
- Consumes: `conversation_meta::{effective_title, effective_icon, icon_svg}`, `state.conversation_meta`.

- [ ] **Step 1: Read the meta map once per render** (near `let convs = state.conversations.read().clone();`): `let meta = state.conversation_meta.read().clone();`.

- [ ] **Step 2: Expanded list rows** — replace each row's title text + leading visual with the merged values. For each conversation `c`:
```rust
let m = meta.get(c.id.0.as_str());
let title = crate::conversation_meta::effective_title(m.and_then(|x| x.title.as_deref()), &c.title);
let icon = crate::conversation_meta::effective_icon(m.and_then(|x| x.icon.as_deref()));
// render: svg(crate::conversation_meta::icon_svg(icon)).width(Size::px(16.)).height(Size::px(16.)).color(th.faint())  +  label().text(title)...
```
(The expanded rows are built via `ListItem` — check `ListItem`'s API in oxide-ui; if it takes a leading icon + title, pass them; if not, render the row inline as a `rect` row `[icon | title]`. Mirror the existing row construction; do not regress the existing selected/hover/on_press behavior.)

- [ ] **Step 3: Collapsed rail icons** — replace `StatusDot::new(true)` in the 28×28 conversation button with `svg(icon_svg(effective_icon(...))).width(16).height(16).color(if sel { th.text() } else { th.faint() })`, using the same `meta`/`c` merge. Keep the existing `OxideTooltip::detailed(...)` (its title line should also use `effective_title`).

- [ ] **Step 4: Build + snapshot + READ.** Reuse/extend the shell snapshot (`app.rs` `snapshot_shell_app`) — seed the `MockTransport` conversations + (if feasible) a `conversation_meta` entry — render to `/tmp/sidebar-titles-icons.png`. (If seeding meta through the mock is awkward, snapshot with agentd-title fallback + default icons, which still exercises the merge.) Controller reads it.

Run: `cargo build -p oxide-freya --bin oxide-freya` Finished; `cargo test -p oxide-freya` pass; `cargo clippy -p oxide-ui -p oxide-freya` clean.

- [ ] **Step 5: Commit**
```bash
git add oxide-app/crates/oxide-freya/src/regions/sidebar.rs
git commit -m "feat(sidebar): render effective conversation title + icon (meta override → agentd → default)"
```

---

### Task 4: Right-click context menu → Rename (inline editor)

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs`

**Interfaces:**
- Consumes: `oxide_ui::components::{open_context_menu}` + Freya `Menu`/`MenuItem`; `state.rename_conversation`.

- [ ] **Step 1: Add a per-render editing signal** at the top of `render`: `let mut editing = use_state::<Option<String>>(|| None);` (holds the conversation id being renamed).

- [ ] **Step 2: Right-click opens the menu.** On each expanded conversation row add:
```rust
.on_secondary_down({
    let id = c.id.0.clone();
    move |e: Event<PressEventData>| {
        let mut editing = editing;
        let id = id.clone();
        let menu = Menu::new().child(
            MenuItem::new()
                .on_press(move |_: Event<PressEventData>| editing.set(Some(id.clone())))
                .child(label().text("Rename")),
        );
        open_context_menu(&e, menu);
    }
})
```
(Grep `open_context_menu` usage in `bubble.rs`/`text_input.rs` and MIRROR the exact `Menu`/`MenuItem` construction + how the menu is closed on select — Task 5 adds the "Set icon…" item to this same menu.)

- [ ] **Step 3: Render the inline editor when this row is being edited.** When `editing.read().as_deref() == Some(c.id.0.as_str())`, render a `TextInput` seeded with the current effective title instead of the title label:
```rust
let mut edit_val = use_state(String::new); // see note
// on entering edit mode, seed edit_val with `title`; commit on Enter:
TextInput::new(edit_val.into_writable(), th)
    .on_key_down(move |e| if e.key == Key::Enter { state.rename_conversation(&id, edit_val.peek().clone()); editing.set(None); })
```
NOTE: seeding per-row edit state cleanly is fiddly. Simpler approach: keep ONE `edit_val: State<String>` at render top; when Rename is chosen, set both `editing = Some(id)` AND `edit_val = current title`. On commit (Enter) → `rename_conversation` + `editing=None`; on blur/Escape → `editing=None`. Verify `TextInput`'s available events (`on_key_down`/blur) by reading `text_input.rs`; use whatever it exposes (it already supports the clipboard menu). If `TextInput` lacks a key/commit hook, wrap a Freya `Input` for the edit field instead — choose the one that exposes an Enter/commit signal.

- [ ] **Step 4: Build + clippy + (live note).** Headless can't drive the context menu; verify it compiles + a snapshot of a row in forced-edit mode (set `editing` in a test harness) renders a `TextInput`. `cargo build -p oxide-freya` Finished; `cargo clippy -p oxide-ui -p oxide-freya` clean.

- [ ] **Step 5: Commit**
```bash
git add oxide-app/crates/oxide-freya/src/regions/sidebar.rs
git commit -m "feat(sidebar): right-click Rename conversation (inline editor → conversation_meta)"
```

---

### Task 5: Set-icon picker popover + final verify

**Files:**
- Modify: `oxide-app/crates/oxide-freya/src/regions/sidebar.rs`

**Interfaces:**
- Consumes: `Popover`, `conversation_meta::{CURATED_ICONS, icon_svg}`, `state.set_conversation_icon`.

- [ ] **Step 1: Add a "Set icon…" item to the row context menu** (the `Menu` from Task 4): a second `MenuItem` "Set icon…" that sets a per-render `icon_pick: State<Option<String>>` to the conversation id (toggles the picker open for that row).

- [ ] **Step 2: Render the icon-picker popover** anchored to the row when `icon_pick.read().as_deref() == Some(c.id.0.as_str())`:
```rust
Popover::new(<row element>)
    .open(true)
    .placement(Placement::Below)
    .content(
        MenuSurface::new(th).on_close({ let mut p = icon_pick; move |_| p.set(None) }).child(
            // a wrapping grid: rect (Horizontal, wrap) of CURATED_ICONS buttons:
            // each = rect 28x28 .on_press(set_conversation_icon(id, name); icon_pick=None)
            //        .child(svg(icon_svg(name)).width(18).height(18).color(th.text()))
        ))
```
Build the grid by folding `CURATED_ICONS` into rows of 4 (or a single wrapping row) — mirror how other multi-child grids are built in the codebase; each icon button captures its own `name`/`id`/`icon_pick` clones.

- [ ] **Step 3: Full verify + snapshot the picker.** Force `icon_pick = Some(id)` in a snapshot harness → render the grid → `/tmp/icon-picker.png`; controller reads it. Then:
```
cargo build -p oxide-freya --bin oxide-freya     # Finished
cargo test -p oxide-ui -p oxide-freya            # all pass
cargo clippy -p oxide-ui -p oxide-freya          # clean (note pre-existing main_region.rs test warnings as known)
```

- [ ] **Step 4: Commit**
```bash
git add oxide-app/crates/oxide-freya/src/regions/sidebar.rs
git commit -m "feat(sidebar): conversation icon picker (curated lucide grid → conversation_meta)"
```

---

## Self-Review notes

- **Spec coverage:** `.oxide` store + atomic write + fallback path (T1) ✓; curated icons + `icon_svg` + helpers + unit tests (T1) ✓; `conversation_meta` signal + load + auto-title bug-fix + edit helpers (T2) ✓; display merge titles+icons expanded+collapsed (T3) ✓; right-click Rename inline (T4) ✓; Set-icon lucide picker (T5) ✓; testing (unit + snapshots + live) ✓; no backend changes ✓.
- **Type consistency:** `ConvMeta{title,icon}`, `ConversationMetaStore::{load,get,as_map,set_title,set_icon}`, `effective_title(Option<&str>,&str)`, `effective_icon(Option<&str>)`, `icon_svg(&str)`, `derive_title(&str)->Option<String>`, `conversation_meta: State<HashMap<String,ConvMeta>>`, `rename_conversation`/`set_conversation_icon`/`reload_conversation_meta`/`with_meta_store` used consistently across tasks.
- **Flagged verify-against-source items:** lucide fn return type (`icon_svg` signature + `svg(...)` arg), `State::new` constructor in `AppState::new`, `AppState` `PartialEq` field list, `ListItem` leading-icon API, `open_context_menu`/`Menu`/`MenuItem` exact construction (mirror bubble.rs/text_input.rs), `TextInput` commit/key event (else use Freya `Input`), `Popover`/`MenuSurface` import paths. Each step says mirror/grep, not guess.
- **Deferred:** Delete/close (menu is its future home), agentd persistence, AI titles, slices B + C.
