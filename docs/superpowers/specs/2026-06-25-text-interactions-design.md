# Text Interactions — design

**Date 2026-06-25.** From live-test feedback: (1) right-click in a text input should show a standard
Cut/Copy/Paste menu; (2) chat messages should be selectable to copy/paste. Freya v0.4.0-rc.23,
`oxide-ui` + `oxide-freya`. Grounded in `docs/research/2026-06-25-freya-text-interactions-apis.md`.

## Scope (locked, from user)

- **Build our own reusable themed `TextInput` component (single-line)** that bakes in our theming AND
  the right-click Cut/Copy/Paste/Select-All menu, so EVERY single-line input gets it for free. Migrate
  the existing single-line inputs to it (today: the sidebar search in `sidebar_header.rs`, and the
  `PromptInput` wrapper used by `main_region.rs`).
- **The composer input stays its own component** (it is multi-line / auto-grow / send-on-enter) — it
  reuses the shared right-click-menu helper but is NOT replaced by `TextInput`.
- **Chat bubbles → selectable text (drag-select + Ctrl+C) AND a right-click "Copy message" menu.**

### Why `TextInput` is built on `use_editable`, not Freya `Input`

To trigger Cut/Copy/Select-All from a context menu we must read the selection and mutate the rope —
i.e. own the editor handle. Freya's `Input` hides its internal editable, so a menu can't drive it.
`TextInput` therefore wraps `use_editable` + a single-line `paragraph()` (the same engine the composer
editor uses, with `max_lines(1)` + Enter-submits + no auto-grow), giving us full menu control and our
theming. The composer editor already proves this pattern.

## Key facts from research

- The composer editor (`use_editable`) **already** handles Ctrl/Cmd + C/X/V/A internally
  (`text_editor.rs:606`). So the right-click menu only needs to **trigger those same ops** — it adds
  no new clipboard logic, except Paste must keep our existing **image-aware** intercept.
- Freya `ContextMenu` model: mount `ContextMenuViewer::new()` once per window (root-scope state +
  renders the floating overlay at the cursor); build a `Menu::new().child(MenuButton::new()...)`;
  wire `.on_secondary_down(move |e| ContextMenu::open_from_event(&e, menu))`
  (`context_menu.rs:74`; example `examples/feature_secondary_press.rs`).
- `TextEditor` primitives on the editable: `get_selected_text()`, `get_selection_range()`,
  `remove(Range)`, `insert(&str, idx)`, `set_selection`/`clear_selection` (utf-16 code-unit indices).
- Freya `Clipboard::get() -> Result<String,_>` / `Clipboard::set(String)` (text, in `freya::prelude`);
  our `composer/clipboard.rs` already handles **images** via `arboard`.
- `SelectableText::new(impl Into<Cow<'static,str>>)` + `.max_lines()`/`.line_height()` — read-only
  `use_editable` + `paragraph()`, drag-select + Ctrl+C built-in, **plain-text only (no markdown)**.
- Chat bubble (`oxide-ui/src/components/bubble.rs`) renders the body with `label().text(...)`
  (lines ~58, 81); the chat renders **no markdown** → swapping to `SelectableText` loses nothing.
  The `Label::try_downcast` test (~bubble.rs:88) must be updated to match a paragraph/span.

## Components & changes

### 1. `ContextMenuViewer` mounted once per window — `oxide-freya`
`ContextMenu::open_from_event` requires a `ContextMenuViewer` ancestor providing root-scope state.
Mount `ContextMenuViewer::new()` once near the root of each window that hosts text surfaces — the
shell root in `app.rs` (covers composer + chat bubbles in the same tree). If the standalone chat
window and the overlay have separate roots, mount it in each. The viewer renders the floating menu;
it does not change layout when closed.

### 2. Reusable text clipboard menu — new `oxide-ui/src/components/menu/text_menu.rs`
A small helper module (reuses our existing `menu` theme / `MenuRow` styling where it fits, else
Freya `Menu`/`MenuButton`):
- `editor_clipboard_menu(theme, actions) -> Menu` — builds `MenuButton`s for **Cut, Copy, Paste,
  Select All**, each calling an `EventHandler` the caller supplies (one per action). Cut/Copy are
  shown always (no-op when no selection — matches OS behavior). Built with Freya `Menu`.
- `copy_only_menu(theme, on_copy) -> Menu` — a single **Copy** (labelled "Copy message" for bubbles).
- Each menu is opened by the caller via `on_secondary_down(move |e| ContextMenu::open_from_event(&e, menu))`.

### 3. Reusable themed `TextInput` — new `oxide-ui/src/components/text_input.rs`
A single-line input on `use_editable` + `paragraph()` with our theming + the right-click clipboard menu
baked in. Builder: `TextInput::new(value: Writable<String>)` + `.placeholder(&str)` + `.on_submit(EventHandler<String>)`
+ `.on_change(EventHandler<String>)` + theme. Behavior: `max_lines(1)`, Enter fires `on_submit`,
themed background/border/caret (`th.*`), placeholder overlay, focus ring. Right-click → `on_secondary_down`
opens `editor_clipboard_menu` (Cut/Copy/Paste/Select-All) driving its own editable (the clipboard-op
logic is the shared helper from §3.1 below). Keeps `value` synced on every edit. This is the canonical
single-line input used everywhere going forward.

#### 3.1 Shared clipboard-op helper
Both `TextInput` and the composer editor drive the same four ops against a `use_editable` handle. Factor
the logic into a small helper (free fns or a trait-ext) so it is written once: `copy`/`cut`/`paste`
(image-aware)/`select_all` taking the editable + the `value` Writable to sync. Both editors call it
from their menu handlers AND it is what the editor's existing Ctrl-key path already does.

### 4. Migrate existing single-line inputs — `sidebar_header.rs`, `prompt_input.rs`
- `sidebar_header.rs:69`: replace the inline Freya `Input` search box with `TextInput` (preserve
  placeholder + the value binding + any on_change).
- `prompt_input.rs`: `PromptInput` becomes a thin specialization that renders `TextInput` internally
  (so its existing callers — `main_region.rs:170` — keep working) OR is migrated to `TextInput` directly
  if it adds nothing beyond it. Decide in plan; either way no caller breaks and both gain theming + the menu.

### 5. Composer editor right-click — `oxide-ui/src/components/composer/editor.rs`
Add `.on_secondary_down` on the editor `paragraph()` that opens `editor_clipboard_menu` with handlers
that drive the editable engine:
- **Copy** → `let s = editor.get_selected_text(); if !s.is_empty() { Clipboard::set(s) }`.
- **Cut** → Copy, then `editor_mut().remove(get_selection_range())`, then sync `value` Writable.
- **Paste** → reuse the **image-aware** path: `read_clipboard_image()` → if `Some`, fire
  `on_paste_attachment`; else `Clipboard::get()` → `editor_mut().insert(&text, cursor)` + sync `value`.
- **Select All** → `editor_mut().set_selection(full range)`.
- Implementation note: prefer driving the `TextEditor` methods directly (above). If a method shape
  fights the borrow checker, fall back to re-dispatching `EditableEvent::KeyDown` with the matching
  Ctrl+key (the engine already handles them), keeping Paste on our image-aware path. Decide in plan.
- Keep `value` in sync after Cut/Paste exactly as `on_key_down` does (`*value.write() = text`).

### 6. Chat bubble selectable text + Copy menu — `oxide-ui/src/components/bubble.rs`
- Swap the body `label().text(body)` → `SelectableText::new(body.clone())` (keep font/color/line-height
  via its style builders; match current visual). Drag-select + Ctrl+C now work.
- Add `.on_secondary_down` opening `copy_only_menu` whose Copy → `Clipboard::set(body.clone())`
  ("Copy message" copies the whole message; partial copy is drag-select + Ctrl+C).
- Update the bubble test that downcasts `Label` to instead assert the body text via the new
  selectable element (paragraph/span match).

## Data flow
right-click editor → `on_secondary_down` → `ContextMenu::open_from_event` → menu → action handler
drives editable (`get_selected_text`/`remove`/`insert`/`set_selection`) + `Clipboard`/image-paste →
`value` synced. right-click bubble → Copy → `Clipboard::set(full body)`. drag-select + Ctrl+C handled
by `SelectableText`/editable built-ins.

## Error handling
- `Clipboard::get()`/`set()` return `Result`; on `Err` → no-op (never panic). Empty selection → Copy/Cut
  no-op. Paste with empty clipboard → no-op.
- Image paste path already returns `Option` (no panic); falls through to text paste.

## Testing
- Unit: `editor_clipboard_menu`/`copy_only_menu` build the expected MenuButtons (labels present);
  bubble renders the body text via `SelectableText` (updated downcast).
- **Headless dark-theme snapshots (render AND read — and ALWAYS `use_init_theme(dark_theme)` + a
  `bg_deep()` backdrop, or near-white text/menus are invisible on the harness's white default):**
  (a) composer editor with the Cut/Copy/Paste/Select-All menu open; (b) a chat bubble with selectable
  body + the "Copy message" menu open. Drive `on_secondary_down` (or force-open) in the test.
- `cargo clippy` clean; hand-formatted; reuse Freya `ContextMenu`/`Menu`/`SelectableText`.

## Non-goals (this round)
- Rich-text/markdown selection (the chat has none).
- New inputs beyond what exists today (sidebar search + PromptInput + composer). Future single-line
  inputs simply use `TextInput` and get the menu for free.
- Find/replace, spell-check, or any editing beyond the four standard clipboard ops.
