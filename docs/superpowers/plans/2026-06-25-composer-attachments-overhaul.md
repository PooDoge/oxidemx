# Composer Attachments Overhaul — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use `- [ ]`.

**Goal:** Relocate the composer's attachment chips into a horizontal scroll strip beside the model pill, make them minimal-width + clickable to a viewer (image lightbox / non-image info card), and support pasting images from the clipboard as real attachments.

**Architecture:** Extend the `Attachment` model with `kind` + `data` (real bytes for pasted images). Move chips from the full-width `AttachmentRow` into a horizontal `ScrollView` strip inside the Toolbar (replacing the flex spacer). A new `AttachmentViewer` built on Freya `Popup` renders an image lightbox or an info card. Clipboard paste reads the OS clipboard via `arboard` on Ctrl/Cmd+V (image-only intercept) and pushes an attachment. No agentd transport this round.

**Tech Stack:** Rust, Freya 0.4.0-rc.23 (`Popup`, `ScrollView` horizontal, `image()`+`dynamic_bytes`), `arboard` (clipboard), `image` (PNG-encode pasted RGBA), `freya-testing`.

## Global Constraints

- Reuse Freya built-ins (Rule 0/4): `Popup` for the viewer (its backdrop + Escape + click-outside dismissal — do NOT hand-roll), `ScrollView` (horizontal, `.show_scrollbar(false)`) for the strip, `image()` for the lightbox. Confirm uncertain builders against `/run/media/system/fastdrive/repos/freya/crates/` + `docs/reference/freya-components-catalog.md` before use.
- **Confirmed APIs:** `Popup::new().on_close_request(EventHandler<()>).maybe(show, |p| p.child(..))` (+ `PopupContent::new()`, `PopupButtons::new()`); `image(image_holder)` where the holder comes from `dynamic_bytes(Vec<u8>)` / `static_bytes(&[u8])` (confirm exact module path — catalog lists both); image scaling via `AspectRatio::{Fit,Max,Min,None}` + `ImageCover` (confirm the `image()` builder method names, e.g. `.aspect_ratio(..)` / `.sampling(..)`); `ScrollView::new().direction(Direction::Horizontal).show_scrollbar(false).spacing(f32)`. The compact-chip + click pattern follows `MenuRow`/`prompt_input.rs` idiom (`on_press` = `Event<PressEventData>`).
- **Scope:** UI/UX + real clipboard **image** bytes only. NO agentd transport. The `+`-menu sources stay mock (`sample_attachment`). Text paste is NOT intercepted (normal editor insertion).
- **New deps:** `arboard` (clipboard image+text) and `image` (encode arboard RGBA → PNG bytes for `dynamic_bytes`). Add to `oxide-ui/Cargo.toml` + workspace `[workspace.dependencies]`. Pin current stable versions (`arboard = "3"`, `image = "0.25"` — confirm at add time).
- No hardcoded hex (Theme accessors). Hand-formatted (no `cargo fmt`); `cargo clippy -p oxide-ui` clean. Build from `oxide-app/` with `LIBRARY_PATH=/tmp/oxidemx-lib-links`.
- **BINDING LESSON — verify visually:** for every task with a snapshot, after rendering it you MUST `Read` the PNG and confirm it looks correct; "renders without panic" is NOT sufficient. The controller will also read each snapshot. **After the final task, rebuild the binary** (`cargo build -p oxide-freya --bin oxide-freya`).

---

### Task 1: Attachment model (`kind`/`data`) + compact chip

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/composer/attachment.rs`.

**Interfaces:**
- Produces: `enum AttachKind { Image, Text, File }` (derive Clone, Copy, Debug, PartialEq, Eq); `enum AttachData { Image(Vec<u8>), Text(String), None }` (derive Clone, Debug, PartialEq); `Attachment { icon: &'static str, tone: Tone, name: String, meta: String, kind: AttachKind, data: AttachData }` (derive Clone, Debug, PartialEq). `pub fn attach_kind_for_icon(icon: &str) -> AttachKind` (`"camera"|"disk if image"`→? — map: `camera`→Image, `clipboard`|`terminal`→Text, else File). `sample_attachment` sets `kind: attach_kind_for_icon(icon)`, `data: AttachData::None`. Compact chip: `AttachmentChip::new(att, theme).compact(true)` OR a dedicated `AttachmentChip` already-compact form — add `.on_view(EventHandler<()>)` alongside existing `.on_remove(EventHandler<()>)`; compact render = `Content::Fit` row (icon 14 + name, NO subtitle), radius 8, tone-tint (`with_alpha(tone,0x16)`/`0x33`), body `on_press`→on_view, trailing `×` `on_press`→on_remove (stop_propagation on the × so it doesn't also fire view).

- [ ] **Step 1: failing tests**
```rust
#[test]
fn kind_inferred_from_icon() {
    assert_eq!(attach_kind_for_icon("camera"), AttachKind::Image);
    assert_eq!(attach_kind_for_icon("clipboard"), AttachKind::Text);
    assert_eq!(attach_kind_for_icon("folder"), AttachKind::File);
}
#[test]
fn sample_attachment_has_kind_and_no_data() {
    let a = sample_attachment("image").unwrap();
    assert_eq!(a.kind, AttachKind::Image);
    assert_eq!(a.data, AttachData::None);
}
#[test]
fn compact_chip_renders_name_and_fires_handlers() {
    use freya_testing::prelude::*;
    fn app() -> impl IntoElement {
        let att = sample_attachment("repo").unwrap();
        AttachmentChip::new(att, Theme::default()).compact(true)
    }
    let mut t = launch_test(app);
    t.sync_and_update();
    assert!(t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("run_bridge.rs"))).is_some());
}
```
- [ ] **Step 2: run → FAIL** (`LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui composer::attachment`).
- [ ] **Step 3: implement** the enums, `Attachment` field additions, `attach_kind_for_icon`, `sample_attachment` update, the `.compact(bool)` + `.on_view(..)` chip. Existing callers of `Attachment{..}` (in `sample_attachment`) must set the new fields; the `AttachmentRow` (if still referenced) keeps compiling.
- [ ] **Step 4: run → PASS**; `cargo clippy -p oxide-ui` clean.
- [ ] **Step 5: commit** `feat(oxide-ui): attachment kind/data model + compact clickable chip`

---

### Task 2: Attachment strip in the Toolbar (relocate + overflow)

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/composer/toolbar.rs`.

**Interfaces:**
- Consumes: `Attachment`, compact `AttachmentChip` (Task 1).
- Produces: Toolbar builders `.attachments(Vec<Attachment>)`, `.on_attach_remove(EventHandler<usize>)`, `.on_attach_view(EventHandler<usize>)`.

**Changes:** In `Toolbar::render`, REPLACE the `Size::flex(1.0)` spacer with a horizontal attachment strip that occupies the flex middle: `ScrollView::new().direction(Direction::Horizontal).show_scrollbar(false).spacing(6.).width(Size::flex(1.0))` containing one compact `AttachmentChip` per `self.attachments` (built by index in a reassignment loop: `let mut strip = ScrollView...; for (i, att) in self.attachments.iter().enumerate() { strip = strip.child(AttachmentChip::new(att.clone(), th).compact(true).on_view(move |_| on_view.call(i)).on_remove(move |_| on_remove.call(i))); }`). Row order: AttachButton(Popover) · ProviderPill · **strip(flex)** · OptimizerChip(when on) · LineHint(when >1) · SendButton. Keep `.content(Content::Flex)` on the row. When `attachments` is empty the empty flex ScrollView still acts as the spacer (send stays right).

- [ ] **Step 1: failing test** — mount `Toolbar::new(Theme::default()).attachments(vec![sample_attachment("repo").unwrap(), sample_attachment("image").unwrap()])`; assert both chip names ("run_bridge.rs", "screenshot.png") render. (freya_testing.)
- [ ] **Step 2: run → FAIL.**
- [ ] **Step 3: implement** the strip + builders + handler wiring (per-index closures; the `×` and body wired to `on_attach_remove`/`on_attach_view`).
- [ ] **Step 4: run → PASS**; clippy clean.
- [ ] **Step 5: snapshots** — add ignored snapshots in `oxide-freya`: `snapshot_toolbar_chips` (2-3 chips beside the pill at ~760px → `/tmp/oxide-toolbar-chips.png`) and `snapshot_toolbar_overflow` (8 chips → `/tmp/oxide-toolbar-overflow.png`). Run `--ignored`; **Read both PNGs**: confirm chips sit right of the pill, minimal width, send button still pinned fully inside on the right, strip scrolls (overflow doesn't push send off).
- [ ] **Step 6: commit** `feat(oxide-ui): toolbar attachment scroll strip beside model pill`

---

### Task 3: `AttachmentViewer` (image lightbox + info card)

**Files:** Create `oxide-app/crates/oxide-ui/src/components/composer/attachment_viewer.rs`; modify `composer/mod.rs` (`pub mod attachment_viewer;` + re-export). Reference `freya-components/src/popup.rs`, `examples/component_popup.rs`, `freya-core/src/elements/image.rs`.

**Interfaces:**
- Consumes: `Attachment`, `AttachKind`, `AttachData`, `icons::icon`, `Theme`.
- Produces: `AttachmentViewer { attachment: Attachment, theme: Theme }` + `.on_dismiss(EventHandler<()>)`, impl Component. Always rendered by the caller only when something is being viewed (caller gates), so internally it shows immediately. Built on `Popup::new().on_close_request(move |_| on_dismiss.call(())).maybe(true, |p| p.child(body))`:
  - `kind == Image` && `data == Image(bytes)` → `body` = `PopupContent` holding `image(dynamic_bytes(bytes.clone())).aspect_ratio(AspectRatio::Fit)` in a max-size box (e.g. `max_width(Size::px(720.))`/`max_height(Size::px(540.))`) + a close `Button`/`×`. (Confirm `dynamic_bytes` path + the `image()` aspect builder name against source.)
  - `kind == Image` && no bytes → `body` = a large placeholder rect (icon 48 + `name`).
  - else (Text/File) → compact info card: `icon(att.icon, 20, tone)` + `name` (bold) + `meta` + a "type" line (`format!("{:?}", kind)`), in a small `PopupContent`.
- The Popup's own backdrop press + Escape fire `on_close_request` → `on_dismiss` (reuse; no bespoke dismissal).

- [ ] **Step 1: failing tests** — `viewer_image_renders` (mount `AttachmentViewer` with an Image attachment carrying a tiny seeded PNG `Vec<u8>`; assert it mounts without panic + the close affordance/label is found); `viewer_infocard_for_file` (File attachment → assert `name` + a type label render). (freya_testing; for the image, a 1×1 PNG byte literal is fine.)
- [ ] **Step 2: run → FAIL.**
- [ ] **Step 3: implement.** Confirm `dynamic_bytes`/`image().aspect_ratio()` builders against `freya-core/src/elements/image.rs` first; use the exact names.
- [ ] **Step 4: run → PASS**; clippy clean.
- [ ] **Step 5: snapshots** — ignored in `oxide-freya`: `snapshot_viewer_image` (seed a real small PNG → `/tmp/oxide-viewer-image.png`) + `snapshot_viewer_infocard` (a File attachment → `/tmp/oxide-viewer-infocard.png`). Run `--ignored`; **Read both**: image lightbox shows the picture centered on a dimmed backdrop with a close ×; info card shows icon+name+meta+type.
- [ ] **Step 6: commit** `feat(oxide-ui): AttachmentViewer (image lightbox + info card)`

---

### Task 4: Clipboard paste (arboard, image-only)

**Files:** Create `oxide-app/crates/oxide-ui/src/components/composer/clipboard.rs`; modify `editor.rs` (paste hook + `.on_paste_attachment` builder); modify `oxide-ui/Cargo.toml` + workspace `Cargo.toml` (deps).

**Interfaces:**
- Produces: `pub fn image_data_to_attachment(width: usize, height: usize, rgba: &[u8]) -> Option<Attachment>` (PURE, testable — PNG-encode the RGBA via the `image` crate; returns `Attachment { kind: Image, data: Image(png_bytes), name: "pasted-image.png", meta: format!("{width}×{height}"), icon: "camera", tone: Tone::Mauve }`; `None` on encode failure); `pub fn read_clipboard_image() -> Option<Attachment>` (impure: `arboard::Clipboard::new().ok()?.get_image().ok()` then `image_data_to_attachment`; never panics). `ComposerEditor` gains `.on_paste_attachment(EventHandler<Attachment>)`.
- In `editor.rs` `on_key_down`: detect paste (`(modifiers.ctrl() || modifiers.meta()) && matches!(&key, Key::Character(c) if c.eq_ignore_ascii_case("v"))`). On paste: `if let Some(att) = read_clipboard_image() { e.prevent_default(); e.stop_propagation(); on_paste_attachment.call(att); return; }` — otherwise fall through to the normal editor key handling (text paste inserts as usual). Place this check BEFORE forwarding to `editable.process_event`.

- [ ] **Step 1: failing test** (pure, no real clipboard):
```rust
#[test]
fn rgba_2x2_becomes_image_attachment() {
    let rgba = vec![255u8; 2 * 2 * 4]; // 2x2 opaque white
    let a = image_data_to_attachment(2, 2, &rgba).expect("encodes");
    assert_eq!(a.kind, AttachKind::Image);
    assert_eq!(a.meta, "2×2");
    match a.data { AttachData::Image(bytes) => assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G'])), _ => panic!("expected PNG image data") }
}
```
- [ ] **Step 2: run → FAIL** (`cargo test -p oxide-ui composer::clipboard`).
- [ ] **Step 3: implement** `clipboard.rs` (add `arboard` + `image` deps), the `image_data_to_attachment` PNG encode, `read_clipboard_image`, and the editor paste hook + `.on_paste_attachment` builder. (`arboard::ImageData` fields: `width: usize, height: usize, bytes: Cow<[u8]>` RGBA8 — confirm at add time.)
- [ ] **Step 4: run → PASS**; `cargo build -p oxide-ui` (links arboard/image); clippy clean.
- [ ] **Step 5: commit** `feat(oxide-ui): clipboard image paste → attachment (arboard)`

---

### Task 5: Orchestrator wiring + mount + live verify

**Files:** Modify `oxide-app/crates/oxide-ui/src/components/composer/mod.rs`.

**Interfaces:** Consumes everything above. Public `Composer` API unchanged.

**Changes:**
- Add `let mut viewing = use_state(|| None::<usize>);`.
- REMOVE the `attachment_row` block (the full-width `AttachmentRow` above the editor) and its `.maybe_child(attachment_row)` on the card.
- Editor: add `.on_paste_attachment(move |a: Attachment| { attachments.write().push(a); })`.
- Toolbar: `.attachments(attachments.read().clone()).on_attach_remove(move |i| { let mut w = attachments.write(); if i < w.len() { w.remove(i); } if viewing.peek().map_or(false,|v| v==i) { viewing.set(None); } }).on_attach_view(move |i| viewing.set(Some(i)))`.
- After the outer column, render the viewer when `viewing` is `Some(i)` and `i < attachments.len()`: `AttachmentViewer::new(attachments.read()[i].clone(), th).on_dismiss(move |_| viewing.set(None))` (mount it as an overlay sibling — e.g. a final `.maybe_child(viewer)` on the outer rect; the Popup paints on its own overlay layer).
- Submit/reset path also `viewing.set(None)`.

- [ ] **Step 1:** keep `composer_renders_send_button_and_activity_line` green. Add an orchestrator test that mounts `Composer`, and (if feasible) asserts no panic with attachments seeded via a test hook — otherwise rely on the existing test + snapshots.
- [ ] **Step 2: run → confirm gate.**
- [ ] **Step 3: implement** the wiring (remove AttachmentRow, add viewing + viewer + toolbar attachments + editor paste).
- [ ] **Step 4:** `cargo test -p oxide-ui` + `cargo test -p oxide-freya --bin oxide-freya` green; clippy clean.
- [ ] **Step 5: snapshot** — re-render `snapshot_composer_full` (seed 2 attachments via the composer's interaction or a seeded variant) → `/tmp/oxide-composer-full.png`; **Read it**: chips are in the toolbar strip beside the pill (NOT a full-width row above the editor), composer normal height, send pinned right. Re-render `snapshot_shell` → **Read**: whole shell intact.
- [ ] **Step 6:** **Rebuild the live binary** `LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build -p oxide-freya --bin oxide-freya`.
- [ ] **Step 7: commit** `feat(oxide-freya): wire attachment strip + viewer + clipboard paste into composer`

---

## Self-Review

- **Spec coverage:** (1) relocate beside pill → T2; (2) minimal width → T1 compact chip + T2 strip; (3) overflow → T2 horizontal ScrollView + `snapshot_toolbar_overflow`; (4) clickable viewer (image lightbox + info card) → T1 `on_view` + T3 `AttachmentViewer` + T5 `viewing`; (5) clipboard image paste → T4. Data model (kind/data) → T1. No-agentd-transport honored (no transport task). arboard+image deps → T4 + Global Constraints.
- **Placeholder scan:** the "confirm builder name" steps (dynamic_bytes, `image().aspect_ratio`, arboard ImageData fields/version) are concrete verify-against-source steps with the known shape — not vague TODOs. No bare "handle errors": `read_clipboard_image`/`image_data_to_attachment` return `Option` (no panic), viewer guards `i < len`.
- **Type consistency:** `AttachKind`, `AttachData`, `Attachment{kind,data}`, `attach_kind_for_icon`, `AttachmentChip::compact`/`.on_view`, Toolbar `.attachments`/`.on_attach_remove`/`.on_attach_view`, `AttachmentViewer::new(att,theme).on_dismiss`, `ComposerEditor::on_paste_attachment`, `image_data_to_attachment`/`read_clipboard_image`, `viewing` — used consistently across tasks.

## Notes for the executor
- Reuse Freya: `Popup` (dismissal), `ScrollView` horizontal (overflow), `image()`+`dynamic_bytes` (lightbox). Confirm the few uncertain builder names against `freya-core`/`freya-components` source first.
- Snapshots are the verification of record — **render AND Read every PNG before declaring a visual task done.** Don't relaunch the live app until the controller has read the snapshots.
