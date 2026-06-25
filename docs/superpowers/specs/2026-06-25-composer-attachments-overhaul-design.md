# Composer Attachments Overhaul — design

**Date 2026-06-25.** From live-test feedback. Relocate + restyle composer attachment chips, add
overflow handling, a click-to-view viewer (image lightbox + non-image info card), and clipboard
paste of images/files. Freya v0.4.0-rc.23, `oxide-ui` Composer.

## Scope (locked)

- **UI/UX with real clipboard bytes; NO agentd transport this round.** Attachments still live only
  in the Composer's local state and do not yet travel with a sent message (that transport work is a
  separate later slice). Clipboard paste captures **real** bytes (so a pasted image opens a real
  lightbox); the `+`-menu sources stay mock (`sample_attachment`).

## Decisions (from brainstorm)

- Overflow → **horizontal scroll strip** (Freya `ScrollView` horizontal, no scrollbar).
- Viewer → **image lightbox** (Freya `Popup`) + **info card** (compact `Popup`) for non-images.
- Clipboard → **`arboard`** crate (real text + image bytes; Freya's own clipboard is text-only).

## Components & changes

### 1. Attachment data model — `oxide-ui/src/components/composer/attachment.rs`
Extend `Attachment` so the viewer can render real content:
- `enum AttachKind { Image, Text, File }`
- `enum AttachData { Image(Vec<u8>), Text(String), None }` (stored on the attachment; `None` for mock
  entries without bytes).
- `Attachment { icon, tone, name, meta, kind: AttachKind, data: AttachData }`.
- `sample_attachment(id)` keeps its mock payloads, inferring `kind` from the icon (`camera`→Image,
  `clipboard`/`terminal`→Text, else File) with `data: None`.
- Keep `AttachmentChip`; add a **compact** rendering used in the toolbar strip: `Content::Fit` width
  (leading icon + `name` only, no subtitle), tone-tinted, radius ~8, with a trailing `×`. Two hit
  targets: the chip body fires `on_view`, the `×` fires `on_remove`. (`AttachData` is not `Eq`-cheap;
  derive `Clone`+`PartialEq` — `Vec<u8>` compares by value, fine for our small counts.)
- `AttachmentRow` (full-width) is no longer mounted by the orchestrator; it may stay in the file
  (unused) or be removed — remove it to avoid dead code if nothing references it.

### 2. Toolbar relayout — `oxide-ui/src/components/composer/toolbar.rs`
New row order: **`+` · provider pill · [attachment strip] · line-hint · send**. The strip *replaces
the existing flex spacer*: a horizontal `ScrollView` (`.direction(Direction::Horizontal)
.show_scrollbar(false)`, `width(Size::flex(1.0))`, `spacing(6.)`) holding the compact chips. It takes
the flexible middle and scrolls sideways on overflow; line-hint + send stay pinned right. The row
keeps `.content(Content::Flex)`. Toolbar gains: `attachments: Vec<Attachment>`,
`.on_attach_remove(EventHandler<usize>)`, `.on_attach_view(EventHandler<usize>)`. When `attachments`
is empty the strip still occupies the flex middle (acts as the spacer) so the send button stays right.

### 3. Viewer — new `oxide-ui/src/components/composer/attachment_viewer.rs`
`AttachmentViewer { attachment: Attachment, theme: Theme }` + `.on_dismiss(EventHandler<()>)`,
rendered by the Composer when an attachment is being viewed. Built on Freya **`Popup`** (centered,
backdrop + Escape + click-outside dismissal — reuse its built-in dismissal, do NOT hand-roll):
- `AttachKind::Image` with `AttachData::Image(bytes)` → a lightbox: `image(dynamic_bytes(bytes))`
  scaled to fit a max box, on a `Popup` with an `×` and backdrop-press dismiss.
- `AttachKind::Image` with no bytes (mock) → large placeholder (icon + filename) in the same Popup.
- Non-image → a compact info-card `Popup`: leading icon, `name`, `meta`, a "type" line. (Seam for a
  real text/code preview later when `AttachData::Text` carries content — render it in a mono pane.)
- Confirm in the plan: `Popup`/`PopupBackground` builder + `on_close_request`; `image()` +
  `dynamic_bytes()`; image scaling (`max_width`/`max_height` + aspect). Reference
  `freya-components/src/popup.rs` + `image_viewer.rs` + the catalog.

### 4. Clipboard paste — `oxide-ui/src/components/composer/editor.rs` (+ a small clipboard helper)
On **Ctrl/Cmd + V** in the editor's `on_key_down` (detect `Key::Character("v")` + `modifiers.ctrl()`
or `.meta()`): read the OS clipboard via **`arboard::Clipboard`**:
- image present → push `Attachment { kind: Image, data: Image(png_bytes), name: "pasted-image.png",
  meta: "<w>×<h>", icon: "camera", tone: Mauve }` (encode arboard's RGBA `ImageData` to PNG bytes via
  the `image` crate, or store raw + let the viewer build a holder — decide in plan; PNG is simplest
  for `dynamic_bytes`).
**Behavior (finalized):** ONLY intercept when the clipboard holds an **image** — create the image
attachment and `prevent_default()` so the editor does not also try to insert text. For **text**
clipboard content, do NOT intercept: let the editor's normal paste/insertion proceed (ordinary text
paste must keep working). So Ctrl/Cmd+V → if `arboard.get_image()` is `Ok` → make an image
attachment + consume the event; otherwise fall through to the editor's default paste. (No "Clipboard
text → attachment" path; that was the mock `+`-menu source's job, not paste.)
The editor exposes `.on_paste_attachment(EventHandler<Attachment>)`; the Composer pushes it into
`attachments`. `arboard` calls are synchronous + cheap; call inside the key handler.

### 5. Orchestrator wiring — `oxide-ui/src/components/composer/mod.rs`
- State: keep `attachments: Vec<Attachment>`; add `viewing: use_state(|| None::<usize>)`.
- Remove the `AttachmentRow` block above the editor.
- Pass `attachments` + `on_attach_remove(|i| remove)` + `on_attach_view(|i| viewing.set(Some(i)))`
  into the Toolbar.
- Editor `.on_paste_attachment(|a| attachments.push(a))`.
- When `viewing` is `Some(i)` (and `i < len`), render `AttachmentViewer::new(attachments[i].clone(),
  th).on_dismiss(|_| viewing.set(None))`. Submit/reset also clears `viewing`.

## Data flow
clipboard paste (editor Ctrl+V) → `on_paste_attachment` → Composer `attachments.push` → Toolbar strip
renders chip. chip body press → `on_attach_view(i)` → Composer `viewing=Some(i)` → `AttachmentViewer`
overlay. `×` → `on_attach_remove(i)`. viewer dismiss → `viewing=None`.

## Error handling
- `arboard::Clipboard::new()` / `.get_image()` / `.get_text()` return `Result`; on `Err` (no
  clipboard, wrong content) → no-op (never panic). Image encode failure → fall back to a mock image
  entry (`data: None`).
- Viewer guards `i < attachments.len()` (a removed-while-viewing race clears the viewer).

## Testing
- Unit: clipboard helper maps an `arboard` image/text to the right `Attachment{kind,data}`; compact
  `AttachmentChip` renders the name + fires `on_view`/`on_remove`; viewer picks lightbox vs info-card
  by `kind`.
- **Headless snapshots — rendered AND visually read before any relaunch** (the binding lesson from the
  prior round): toolbar with a few chips beside the pill (minimal width); overflow strip (many chips,
  send still pinned right); image lightbox (seeded PNG bytes); non-image info card.
- `cargo clippy` clean; hand-formatted; Rust + Freya idioms (reuse `Popup`/`ScrollView`/`image`).

## Dependencies
- **New:** `arboard` (clipboard text + image). Add to `oxide-ui` (or a tiny `oxide-platform` shim) —
  decide in plan; `oxide-ui` direct dep is fine. `image` crate may be needed to PNG-encode the pasted
  RGBA (confirm whether `dynamic_bytes` accepts raw RGBA or needs an encoded format).

## Non-goals (this round)
- Sending attachments to agentd (transport slice, later).
- Real bytes for the `+`-menu mock sources (only clipboard paste is real).
- Drag-and-drop file attach; multi-select; reordering.
