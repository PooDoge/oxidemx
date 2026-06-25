# Text Interactions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use `- [ ]`.

**Goal:** A reusable themed single-line `TextInput` with a right-click Cut/Copy/Paste/Select-All menu baked in (migrate existing single-line inputs to it), the same right-click menu on the composer's multi-line editor, and selectable chat bubbles with a right-click "Copy message".

**Architecture:** A shared `text_menu` module holds the clipboard-op helpers (copy/cut/paste/select-all over a `use_editable` handle + a `value` Writable) and the Freya `Menu` builders. `TextInput` (new, on `use_editable`) and the composer editor both open that menu via `on_secondary_down` → `ContextMenu::open_from_event`. A `ContextMenuViewer` is mounted once per window root. Chat bubbles swap `label` → `SelectableText` and get a Copy-only menu.

**Tech Stack:** Rust, Freya 0.4.0-rc.23 (`ContextMenu`/`ContextMenuViewer`/`Menu`/`MenuButton`, `use_editable`, `SelectableText`, `Clipboard::get/set`), existing `arboard` image paste, `freya-testing`.

## Global Constraints

- Reuse Freya built-ins (Rule 0): `ContextMenu`/`ContextMenuViewer` (mount once; example `examples/feature_secondary_press.rs`), `Menu`/`MenuButton`, `SelectableText`, `Clipboard` (text, in `freya::prelude`). Confirm signatures against `/run/media/system/fastdrive/repos/freya/crates/freya-components/src/{context_menu.rs,menu.rs,selectable_text.rs}` + `freya-clipboard` and `docs/research/2026-06-25-freya-text-interactions-apis.md` before use.
- **Confirmed APIs:** `ContextMenu::open_from_event(&Event<PressEventData>, Menu)` (context_menu.rs:74), `ContextMenuViewer::new()` mounted once high; `Menu::new().child(MenuButton::new().on_press(..).child("Label"))`; `.on_secondary_down(move |e| ContextMenu::open_from_event(&e, menu))`. Editable: `editable.editor().read().get_selected_text()`, `.get_selection_range()`, `editable.editor_mut().write().remove(Range)`, `.insert(&str, idx)`, `.set_selection(..)` / `.clear_selection()` (indices = **utf-16 code units**). `Clipboard::get() -> Result<String,_>`, `Clipboard::set(String) -> Result<(),_>`. `SelectableText::new(impl Into<Cow<'static,str>>)` + `.max_lines()`/`.line_height()` + text-style builders (plain-text only, no markdown).
- **Theming/snapshots:** no hardcoded hex (`th.*` / `Theme::with_alpha`). Every snapshot MUST `use_init_theme(dark_theme)` at root AND wrap in `rect().background(Theme::default().bg_deep())` — the harness defaults to WHITE and near-white text/menus are invisible otherwise (this cost two cycles last slice). **Render AND read every snapshot before declaring a visual task done.**
- Build from `oxide-app/` with `LIBRARY_PATH=/tmp/oxidemx-lib-links`. Hand-formatted (no `cargo fmt`); `cargo clippy -p oxide-ui -p oxide-freya` clean. Reuse our existing `menu` theme (`components/menu/theme.rs`) where the Freya `Menu` accepts theming.
- Clipboard ops never panic: `Clipboard::get()/set()` `Result` → no-op on `Err`; empty selection → Copy/Cut no-op; empty clipboard → Paste no-op.

---

### Task 1: `text_menu` module — clipboard-op helpers + menu builders + mount `ContextMenuViewer`

**Files:** Create `oxide-app/crates/oxide-ui/src/components/menu/text_menu.rs`; `pub mod text_menu;` + re-export in `components/menu/mod.rs` (and surface via `components`/lib re-exports as siblings are). Modify the shell root in `oxide-app/crates/oxide-freya/src/app.rs` to mount `ContextMenuViewer`.

**Interfaces:**
- Produces — clipboard ops over a `use_editable` handle (type from `freya::prelude` / `freya::text_edit`; confirm the concrete `UseEditable` type name). Signatures (adjust to the real editable type):
  - `pub fn copy_selection(editable: &UseEditable)` — `let s = editable.editor().read().get_selected_text().unwrap_or_default(); if !s.is_empty() { let _ = Clipboard::set(s); }`
  - `pub fn cut_selection(editable: &mut UseEditable, value: &mut Writable<String>)` — copy, then remove the selection range, then sync `value`.
  - `pub fn paste_text(editable: &mut UseEditable, value: &mut Writable<String>)` — `if let Ok(t) = Clipboard::get() { insert at cursor; sync value }` (image-aware paste stays in the editor's own key path; this helper is text paste — the composer's menu Paste wraps it with the image check, see Task 5).
  - `pub fn select_all(editable: &mut UseEditable)` — set selection to the full rope range.
- Produces — menu builders:
  - `pub fn editor_clipboard_menu(theme: Theme, on_cut: EventHandler<()>, on_copy: EventHandler<()>, on_paste: EventHandler<()>, on_select_all: EventHandler<()>) -> Menu` — four `MenuButton`s labelled exactly "Cut", "Copy", "Paste", "Select All".
  - `pub fn copy_only_menu(theme: Theme, label: &str, on_copy: EventHandler<()>) -> Menu` — one `MenuButton` with the given label (callers pass "Copy message").
- `app.rs`: wrap the shell's root subtree so a `ContextMenuViewer::new()` is an ancestor of the composer + chat (mount once; it renders the floating menu and is layout-neutral when closed). If the standalone chat window and overlay have distinct roots, mount in each.

- [ ] **Step 1: failing tests** (menu builders are the testable surface; the editable ops need a live editor so are exercised via Task 2/5 snapshots):
```rust
#[test]
fn editor_menu_has_four_clipboard_actions() {
    use freya_testing::prelude::*;
    fn app() -> impl IntoElement {
        let noop = EventHandler::from(|_| {});
        editor_clipboard_menu(Theme::default(), noop, noop, noop, noop)
    }
    let mut t = launch_test(app);
    t.sync_and_update();
    for lbl in ["Cut", "Copy", "Paste", "Select All"] {
        assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == lbl)).is_some(), "missing {lbl}");
    }
}
#[test]
fn copy_only_menu_has_label() {
    use freya_testing::prelude::*;
    fn app() -> impl IntoElement { copy_only_menu(Theme::default(), "Copy message", EventHandler::from(|_| {})) }
    let mut t = launch_test(app);
    t.sync_and_update();
    assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Copy message"))).is_some());
}
```
- [ ] **Step 2: run → FAIL** (`LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui menu::text_menu`).
- [ ] **Step 3: implement** the helpers + builders. Confirm the `UseEditable` type + `Menu`/`MenuButton` builder shape against Freya source first; use the real names. Mount `ContextMenuViewer` in `app.rs` shell root.
- [ ] **Step 4: run → PASS**; `cargo build -p oxide-freya --bin oxide-freya` (ContextMenuViewer compiles in the tree); clippy clean.
- [ ] **Step 5: commit** `feat(oxide-ui): text_menu clipboard helpers + menus; mount ContextMenuViewer`

---

### Task 2: Reusable themed `TextInput` (single-line, right-click menu)

**Files:** Create `oxide-app/crates/oxide-ui/src/components/text_input.rs`; `pub mod text_input;` + re-export in `components/mod.rs` (or `lib.rs`, matching siblings). Reference the composer editor (`components/composer/editor.rs`) for the `use_editable` single-line pattern.

**Interfaces:**
- Consumes: `text_menu` (Task 1), `ContextMenu::open_from_event`.
- Produces: `TextInput { value: Writable<String>, theme: Theme, placeholder: Option<String>, on_submit: Option<EventHandler<String>>, on_change: Option<EventHandler<String>> }` + builders `.placeholder(impl Into<String>)`, `.on_submit(impl Into<EventHandler<String>>)`, `.on_change(impl Into<EventHandler<String>>)`, impl Component + `KeyExt`.
- Behavior: `use_editable` + single `paragraph()` with `max_lines(1)`; themed background (`th.surface*`), border, caret (`th.accent()`); placeholder overlay (`th.faint()`) when empty; focus ring; `Enter` fires `on_submit` (+ `prevent_default`/`stop_propagation`), every edit syncs `value` + fires `on_change`. `on_secondary_down` → `ContextMenu::open_from_event(&e, editor_clipboard_menu(...))` whose handlers call the Task-1 ops against THIS input's editable (paste = `paste_text`, no image path for single-line inputs). Keep it focused — NO auto-grow, NO send-on-enter toggle (that's the composer's).

- [ ] **Step 1: failing test** — mount `TextInput::new(value).placeholder("Search…")`; assert the placeholder Label renders when empty; type via `t.type_text`/key events and assert `value` updates (mirror `editor_mounts_with_placeholder` in editor.rs).
- [ ] **Step 2: run → FAIL.**
- [ ] **Step 3: implement.**
- [ ] **Step 4: run → PASS**; clippy clean.
- [ ] **Step 5: snapshot** — ignored in `oxide-freya`: `snapshot_text_input` rendering a `TextInput` with seeded text on the dark backdrop (`use_init_theme(dark_theme)` + `bg_deep()`), AND a variant with its right-click menu force-opened / opened via a driven `on_secondary_down` showing Cut/Copy/Paste/Select All → `/tmp/oxide-textinput.png` + `/tmp/oxide-textinput-menu.png`. Run `--ignored`; **controller reads** to confirm themed input + the 4-item menu legible on dark.
- [ ] **Step 6: commit** `feat(oxide-ui): reusable themed TextInput with right-click clipboard menu`

---

### Task 3: Migrate existing single-line inputs to `TextInput`

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/sidebar_header.rs` (the search `Input` at ~line 69); `oxide-app/crates/oxide-ui/src/components/prompt_input.rs`.

**Interfaces:** Consumes `TextInput` (Task 2). No public-API breakage for `PromptInput`'s callers (`main_region.rs:170`).

**Changes:**
- `sidebar_header.rs`: replace the inline Freya `Input::new(value.into_writable())...` search box with `TextInput::new(value.into_writable()).placeholder(<existing placeholder>)` + preserve any `on_change`/styling intent.
- `prompt_input.rs`: re-implement `PromptInput`'s `render` to delegate to `TextInput` (forward value + placeholder + on_submit/on_change), keeping the `PromptInput::new(...)` builder signature its callers use. If `PromptInput` adds nothing over `TextInput`, make it a thin wrapper; do NOT change its public builder.

- [ ] **Step 1:** keep existing `sidebar_header`/`prompt_input` tests green; if a test asserts a Freya `Input` element, update it to the `TextInput`'s element (paragraph/placeholder Label). Add an assertion that the migrated search renders its placeholder.
- [ ] **Step 2: run → confirm.**
- [ ] **Step 3: implement** the two migrations.
- [ ] **Step 4:** `cargo test -p oxide-ui` + `cargo test -p oxide-freya --bin oxide-freya` green; clippy clean.
- [ ] **Step 5: snapshot** — re-render the sidebar/header snapshot if one exists (dark bg); else add `snapshot_sidebar_search` → `/tmp/oxide-sidebar-search.png`. **Controller reads** to confirm the search box looks right (themed, placeholder visible).
- [ ] **Step 6: commit** `refactor(oxide-ui): migrate sidebar search + PromptInput to TextInput`

---

### Task 4: Composer editor right-click menu

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/composer/editor.rs`.

**Interfaces:** Consumes `text_menu` (Task 1) + its ops. The editor already owns `editable` + `value` + the image-aware paste (`read_clipboard_image` + `on_paste_attachment`).

**Changes:** Add `.on_secondary_down(move |e| ContextMenu::open_from_event(&e, editor_clipboard_menu(th, cut, copy, paste, select_all)))` on the editor `paragraph()`. Handlers:
- Cut → `cut_selection(&mut editable, &mut value)`; Copy → `copy_selection(&editable)`; Select All → `select_all(&mut editable)`.
- **Paste (image-aware)** → first `if let Some(att) = crate::components::composer::clipboard::read_clipboard_image() { on_paste_attachment.call(att) } else { paste_text(&mut editable, &mut value) }` — same precedence as the existing Ctrl+V intercept. (Factor the editor's existing paste decision so the key path and the menu path share it.)
- Capture the needed Copy state handles/clones for each `MenuButton` handler (the editor's `editable`/`value` are already in scope in `render`). Keep the existing `on_key_down` clipboard behavior intact.

- [ ] **Step 1:** keep `editor_mounts_with_placeholder` green; add a test that the editor mounts with `on_secondary_down` wired (smoke: renders without panic). The menu actions are exercised via the snapshot.
- [ ] **Step 2: run → confirm.**
- [ ] **Step 3: implement.**
- [ ] **Step 4:** tests green; clippy clean.
- [ ] **Step 5: snapshot** — ignored: `snapshot_composer_editor_menu` — render the composer editor (dark bg + `use_init_theme(dark_theme)`) with the right-click menu opened (drive `on_secondary_down` on the editor, or force-open) → `/tmp/oxide-editor-menu.png`. **Controller reads**: Cut/Copy/Paste/Select All legible over the editor.
- [ ] **Step 6: commit** `feat(oxide-ui): right-click clipboard menu on composer editor`

---

### Task 5: Chat bubble selectable text + right-click Copy

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/bubble.rs` (body text at ~lines 58, 81 + the downcast test at ~88).

**Interfaces:** Consumes `SelectableText`, `text_menu::copy_only_menu`, `ContextMenu::open_from_event`, `Clipboard::set`.

**Changes:**
- Swap the body `label().text(body)` → `SelectableText::new(body.clone())` with matching font size/color/line-height via its style builders (keep the current look). Both the user + assistant body render paths (~58, ~81).
- Add `.on_secondary_down(move |e| ContextMenu::open_from_event(&e, copy_only_menu(th, "Copy message", on_copy)))` where `on_copy` → `let _ = Clipboard::set(body.clone());`. Wrap the bubble body (the element that should catch right-click).
- Update the `Label::try_downcast` test (~88) to assert the body text via the `SelectableText`'s rendered element (it renders a `paragraph`/span — match the text through that, e.g. find the paragraph/span containing the body, or assert via `t.find` on the span text). If `SelectableText` content isn't a `Label`, switch the downcast accordingly.

- [ ] **Step 1: failing test** — update the bubble body test to find the body text through the new selectable element; run → FAIL (old `Label` downcast no longer matches).
- [ ] **Step 2: run → FAIL.**
- [ ] **Step 3: implement** the SelectableText swap + Copy menu.
- [ ] **Step 4: run → PASS**; `cargo test -p oxide-ui` + `oxide-freya` green; clippy clean.
- [ ] **Step 5: snapshot** — ignored: `snapshot_bubble_selectable` — render a chat bubble (dark bg) with body text + the "Copy message" menu opened → `/tmp/oxide-bubble-menu.png`. **Controller reads**: body text legible + the Copy menu shows. Also rebuild the live binary `cargo build -p oxide-freya --bin oxide-freya`.
- [ ] **Step 6: commit** `feat(oxide-ui): selectable chat bubbles + right-click Copy message`

---

## Self-Review

- **Spec coverage:** reusable `TextInput` (§3 → T2); shared clipboard-op helper + menu builders (§3.1/§2 → T1); `ContextMenuViewer` mount (§1 → T1); migrate sidebar + PromptInput (§4 → T3); composer editor menu (§5 → T4); selectable bubbles + Copy (§6 → T5). Image-aware paste preserved in T4. `use_editable`-based TextInput rationale honored (T2). No-markdown bubble swap (T5).
- **Placeholder scan:** the "confirm the `UseEditable` type / Menu builder shape against Freya source" steps are concrete verify-against-source steps with the known shape, not vague TODOs. Clipboard ops return-on-`Err`/empty (no panic) specified.
- **Type consistency:** `editor_clipboard_menu`/`copy_only_menu`, `copy_selection`/`cut_selection`/`paste_text`/`select_all`, `TextInput::new/.placeholder/.on_submit/.on_change`, `ContextMenu::open_from_event`, `SelectableText::new` — used consistently across tasks. `PromptInput` public builder unchanged (T3).

## Notes for the executor
- Confirm the concrete `use_editable` return type + `Menu`/`MenuButton`/`ContextMenuViewer`/`SelectableText` builder names against `/run/media/system/fastdrive/repos/freya` before writing each task; the research doc has file:line.
- Snapshots are the verification of record — dark theme + `bg_deep()` ALWAYS; render AND read every PNG before declaring a visual task done; don't relaunch the app until snapshots are read.
