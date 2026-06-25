# Freya v0.4.0-rc.23 — text interaction APIs (research)

Source clone: `/run/media/system/fastdrive/repos/freya`. Our repo: `oxidemx-2b`.
All citations are `file:line`. Plain Read/Grep only (no Serena).

---

## 1. ContextMenu / right-click menu

**Files:**
- `crates/freya-components/src/context_menu.rs` — `ContextMenu` state + `ContextMenuViewer` overlay.
- `crates/freya-components/src/menu.rs` — `Menu` / `MenuContainer` / `MenuItem` / `MenuButton` / `SubMenu`.
- Examples: `examples/component_context_menu.rs`, `examples/feature_secondary_press.rs`.

**Mechanism (three parts):**

(a) Mount `ContextMenuViewer::new()` once, high in the tree (sibling of content).
It provides the global `ContextMenu` state in `ScopeId::ROOT` and renders the floating
menu overlay. `context_menu.rs:114-179`. It tracks the global pointer location
(`on_global_pointer_move`, `context_menu.rs:156-158`) so the menu opens at the cursor.

(b) Build a menu as a value (not a component) — `Menu` is a plain builder:
```rust
Menu::new()
    .child(MenuItem::new().on_press(move |_| { /* … */ }).child("Copy"))
    .child(MenuButton::new().child("Close").on_press(|_| ContextMenu::close()))
```
`Menu::new()` `menu.rs:124-127`; `Menu::on_close(F: Into<EventHandler<()>>)` `menu.rs:129-135`;
`Menu::theme(MenuContainerThemePartial)` `menu.rs:137-140`. Items:
- `MenuItem::new()` `menu.rs:330`, `.on_press(impl Into<EventHandler<Event<PressEventData>>>)` `menu.rs:334`, `.on_pointer_enter(..)` `menu.rs:342`, `.selected(bool)` `menu.rs:350`, `.padding(impl Into<Gaps>)` `menu.rs:356`, `.theme(MenuItemThemePartial)` `menu.rs:372`. (`MenuItem` is the base; takes children via `ChildrenExt`.)
- `MenuButton::new()` `menu.rs:493`, `.on_press(..)` `menu.rs:497`, `.theme(..)` `menu.rs:503`. (Wraps `MenuItem`, closes sibling sub-menus on hover.)
- `SubMenu::new()` `menu.rs:553`, `.label(impl IntoElement)` `menu.rs:557`, `.theme(MenuContainerThemePartial)` `menu.rs:563`, children = sub-items.
- `MenuItem` vs `MenuButton`: use `MenuButton` for ordinary clickable rows (it also auto-closes open sub-menus on hover); `MenuItem` is the lower-level primitive. The `feature_secondary_press` example uses `MenuItem` directly; `component_context_menu` uses `MenuButton`.

(c) Open it from an event. `ContextMenu` (`context_menu.rs:40-95`):
- `ContextMenu::open_from_event(event: &Event<PressEventData>, menu: Menu)` `context_menu.rs:74` — **preferred**. Left-click-press defers the first close request (so the menu doesn't immediately close); right-click-down closes on a single subsequent click.
- `ContextMenu::open(menu: Menu)` `context_menu.rs:63`, `ContextMenu::is_open() -> bool` `context_menu.rs:56`, `ContextMenu::close()` `context_menu.rs:90`.
- `ContextMenu::get()` **panics** if no `ContextMenuViewer` ancestor `context_menu.rs:51-54`.

**Wiring to right-click — `on_secondary_down`:** any element supports
`.on_secondary_down(move |e: Event<PressEventData>| ContextMenu::open_from_event(&e, menu))`.
Minimal working example (`examples/feature_secondary_press.rs:13-34`):
```rust
let on_secondary_down = move |e: Event<PressEventData>| {
    ContextMenu::open_from_event(&e,
        Menu::new().child(
            MenuItem::new()
                .on_press(move |_| { let _ = Clipboard::set("Right Click Me".to_string()); })
                .child("Copy")));
};
rect().child(ContextMenuViewer::new())
      .child(label().text("Right click to copy").on_secondary_down(on_secondary_down))
```
(`Clipboard` comes from `freya_edit::Clipboard` in that example, but is also re-exported in `freya::prelude` — see §5.)

`component_context_menu.rs` shows opening from a `Button::on_press` (left-click) with `is_open()`/`close()` toggle, and a `SubMenu` tree.

---

## 2. Editor clipboard ops (our composer is `use_editable` + `paragraph()`)

The clipboard ops live **inside the editor's key handler** — Freya's built-in `Input`/editable
handles Ctrl/Cmd+C/X/V/A itself in `process_key()` (`crates/freya-edit/src/text_editor.rs:606-666`):
- **Select all** `"a"` ctrl_or_meta → `self.set_selection((0, len))` `:627-630`.
- **Copy** `"c"` (gated on `allow_write_clipboard`) → `Clipboard::set(self.get_selected_text())` `:633-638`.
- **Cut** `"x"` (gated `allow_changes && allow_write_clipboard`) → get range, `self.remove(start..end)`, `Clipboard::set(text)`, `move_cursor_to(start)` `:641-650`.
- **Paste** `"v"` (gated `allow_changes && allow_read_clipboard`) → `Clipboard::get()`, delete current selection, `self.insert(&copied, cursor_pos)`, move cursor past `:653-666`.

So **we get C/X/V for free** as long as the editor receives the key events (our composer
forwards them via `editable.process_event(EditableEvent::KeyDown {..})`, `editor.rs:196`). A
right-click menu only needs to *re-trigger* these — two options:

**Option A (drive the editor directly).** The `TextEditor` trait (`text_editor.rs`) exposes
all the primitives a menu handler needs:
- `get_selected_text(&self) -> Option<String>` `:731` (copy source)
- `get_selection_range(&self) -> Option<(usize,usize)>` `:739`
- `get_selection(&self) -> Option<(usize,usize)>` `:375`; `set_selection((usize,usize))` `:447`; `clear_selection()` `:444`
- `remove(&mut self, range: Range<usize>) -> usize` `:145` (cut/replace)
- `insert(&mut self, text: &str, char_idx: usize) -> usize` `:142` (paste)
- `move_cursor_to(&mut self, pos)` `:367`; `cursor_pos()` `:363`
Reach them via `editable.editor_mut().write()` (our composer already does this for `set`/
`clear_selection`, `editor.rs:144-149`). NOTE indices are **utf-16 code units** (the built-in
copy/paste uses `len_utf16_cu()` / `encode_utf16().count()`).

**Option B (synthesize the keystroke).** Build the `Menu` items to call
`editable.process_event(EditableEvent::KeyDown { key: &Key::Character("c".into()), modifiers: Modifiers::CONTROL })`
— reuses the exact built-in logic. Cleaner but indirect.

**Clipboard primitive:** `freya_edit::Clipboard` → `Clipboard::set(String) -> Result<(),ClipboardError>`
and `Clipboard::get() -> Result<String,ClipboardError>` (`crates/freya-clipboard/src/clipboard.rs:31-55`;
re-exported from `freya_edit`). Backed by `copypasta` (NOT arboard) and a root-context provider.

**EditableConfig clipboard gates** (`crates/freya-edit/src/config.rs:4-54`): `with_allow_changes`,
`with_allow_read_clipboard`, `with_allow_write_clipboard`, `with_allow_tabs`, `with_indentation`
— all default `true` except `allow_tabs` (false). Our composer uses bare `EditableConfig::new`
(`editor.rs:127`), so copy/cut/paste are already enabled.

---

## 3. SelectableText

**File:** `crates/freya-components/src/selectable_text.rs` (exported `crates/freya-components/src/lib.rs:39`).
Example: `examples/component_selectable_text.rs`.

**Constructor + builder:**
- `SelectableText::new(value: impl Into<Cow<'static, str>>)` `selectable_text.rs:54`.
- `.max_lines(impl Into<Option<usize>>)` `:66`; `.line_height(impl Into<Option<f32>>)` `:71`.
- Implements `LayoutExt`, `AccessibilityExt`, `TextStyleExt`, `ContainerExt`, `KeyExt` — so the
  standard layout/`font_size`/`color`/padding/a11y builder methods apply (it's a `Component`).

**How it works** (`render`, `:77-200`): internally builds a **read-only** editable
(`use_editable(.., || EditableConfig::new().with_allow_changes(false))`, `:80-83`) and renders a
`paragraph()` with `.highlights(..)` from `editor.get_visible_selection(EditorLine::SingleParagraph)`.
Drag-select via pointer-down/global-move/release (`:98-143`); Ctrl/Cmd+C copy is handled by the
read-only editor's own key handler (`on_key_down → EditableEvent::KeyDown`, `:145-150`) — i.e. the
same `text_editor.rs:633` copy path, allowed because `allow_write_clipboard` defaults true and copy
isn't gated on `allow_changes`. Has a `SelectableTextStatus` (Idle/Hovering) `:7-14`.

**Multi-line / rich text:** it renders **one** `paragraph` over the whole string with
`EditorLine::SingleParagraph`, so it handles wrapped + `\n` multi-line plain text. It takes a single
`Cow<str>` `value` and emits a single `Span` (`:194`) — **no rich-text / multi-span / markdown
support**. Theming = the generic `TextStyleExt` props (font_size/color/etc.); cursor color is
hard-coded `Color::BLACK` (`:183`).

**Can it replace a `label()`/`paragraph()` for read-only display?** Yes for plain text — it *is* a
themed read-only paragraph. Drop-in for `label().text(s)`. Example
(`component_selectable_text.rs:18-21`): `SelectableText::new("You can select this long text")`.

---

## 4. Our chat bubble rendering

**File:** `oxide-app/crates/oxide-ui/src/components/bubble.rs`. Rendered from
`oxide-app/crates/oxide-freya/src/regions/main_region.rs:110` (`Bubble::new(turn.role, turn.text)`)
and `:113` (live assistant stream).

**Today:** the message body is rendered with **`label().text(self.text.clone())`** —
user turn `bubble.rs:58`, assistant turn `bubble.rs:81`. `label()` is **not selectable**.

**Markdown:** the chat does **NOT** render markdown. The only "markdown" hits in oxide-ui are
*comments* in `composer/editor.rs:215` (a deferred SEAM note); no `pulldown-cmark`/`comrak`
dependency in `oxide-ui` or `oxide-freya` Cargo.toml. So message bodies are plain text.

**Cost of swapping to `SelectableText`:** Low. Replace `label().text(t).font_size(13.).color(c)`
with `SelectableText::new(t).font_size(13.).color(c)` in both branches of `bubble.rs`. Since the
chat is plain text, **no markdown is lost** (there's none). Caveats:
- `SelectableText::new` wants `impl Into<Cow<'static,str>>`; `self.text.clone()` (a `String`) works.
- It mounts a `use_editable` per bubble (one read-only rope each) — slightly heavier than a `label`
  per message; fine for typical thread sizes, worth a perf glance for very long threads.
- The unit test in `bubble.rs:88-104` asserts `Label::try_downcast`; swapping to `SelectableText`
  (which renders a `Paragraph`/`Span`, not a `Label`) will break that downcast assertion — update
  the test to match on the paragraph/span text.
- If we ever add markdown rendering, `SelectableText` can't carry it (single-span only) — we'd need
  a custom selectable-paragraph with spans, or keep markdown rendering separate.

---

## 5. `use_clipboard` hook

There is **no `use_clipboard()` hook**. Freya exposes a **`Clipboard` unit-struct** with static
methods instead (`crates/freya-clipboard/src/clipboard.rs:28-55`):
- `Clipboard::get() -> Result<String, ClipboardError>` `:37`
- `Clipboard::set(contents: String) -> Result<(), ClipboardError>` `:47`
- `ClipboardError` = `FailedToRead | FailedToSet | NotAvailable` `:6-11`.

**Re-exported in `freya::prelude`** — `crates/freya/src/lib.rs:99-102` re-exports `Clipboard` and
`ClipboardError` from `freya_edit`. Also available via `freya::text_edit::*`
(`crates/freya/src/lib.rs:238-240`, which our composer already imports). Backed by the `copypasta`
crate, text-only. (Our existing `composer/clipboard.rs` uses `arboard` directly for **image**
paste — a separate concern; text clipboard should use Freya's `Clipboard`.)

---

## Flags / gaps

- Could NOT find a `use_clipboard` hook — none exists; use `Clipboard::{get,set}` (§5). Confirmed.
- `SelectableText` is single-span / plain-text only — no markdown, no per-span styling (§3).
- Clipboard indices in the editor are **utf-16 code units**, not byte/char (§2) — important for
  any direct `remove`/`insert` we drive.
- For a right-click cut/copy/paste menu over the composer, simplest is Option B (re-dispatch the
  ctrl-key `EditableEvent::KeyDown`) since the editor already implements all three; Option A gives
  finer control if needed.
