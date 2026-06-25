//! `TextInput` — a reusable themed single-line text field with a right-click
//! clipboard menu baked in.
//!
//! Built directly on freya's `use_editable` engine (same as `ComposerEditor`),
//! `max_lines(1)` enforces single-line behaviour.  Bare `Enter` fires `on_submit`
//! and does NOT insert a newline.  Every edit syncs `value` and optionally fires
//! `on_change`.
//!
//! Right-clicking opens a Cut / Copy / Paste / Select All context menu via
//! `ContextMenu::open_from_event`.  `ContextMenuViewer` must be mounted in an
//! ancestor scope (as in the real app shell).
//!
//! Builder usage:
//! ```ignore
//! fn app() -> impl IntoElement {
//!     let value = use_state(String::new);
//!     TextInput::new(value.into_writable(), Theme::default())
//!         .placeholder("Search…")
//!         .on_submit(|text| println!("submit: {text}"))
//!         .on_change(|text| println!("change: {text}"))
//! }
//! ```
use freya::prelude::*;
use freya::text_edit::*;

use crate::components::menu::text_menu::{
    copy_selection, cut_selection, editor_clipboard_menu, paste_text, select_all,
};
use crate::tokens::Theme;

/// A themed single-line text input with right-click clipboard menu.
#[derive(Clone, PartialEq)]
pub struct TextInput {
    value:       Writable<String>,
    theme:       Theme,
    placeholder: Option<String>,
    on_submit:   Option<EventHandler<String>>,
    on_change:   Option<EventHandler<String>>,
    key:         DiffKey,
}

impl KeyExt for TextInput {
    fn write_key(&mut self) -> &mut DiffKey {
        &mut self.key
    }
}

impl TextInput {
    pub fn new(value: Writable<String>, theme: Theme) -> Self {
        Self {
            value,
            theme,
            placeholder: None,
            on_submit:   None,
            on_change:   None,
            key:         DiffKey::default(),
        }
    }

    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    pub fn on_submit(mut self, handler: impl Into<EventHandler<String>>) -> Self {
        self.on_submit = Some(handler.into());
        self
    }

    pub fn on_change(mut self, handler: impl Into<EventHandler<String>>) -> Self {
        self.on_change = Some(handler.into());
        self
    }
}

impl Component for TextInput {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let placeholder_text = self.placeholder.clone();
        let on_submit  = self.on_submit.clone();
        let on_change  = self.on_change.clone();

        let a11y_id = use_hook(AccessibilityId::new_unique);
        let focus = use_focus(a11y_id);
        let holder = use_state(ParagraphHolder::default);
        let mut editable = use_editable(
            || self.value.read().to_string(),
            EditableConfig::new,
        );
        let mut is_dragging = use_state(|| false);

        // Each closure that captures value gets its own clone up front.
        // UseEditable is Copy so it can be shared across closures without cloning.
        let value_read   = self.value.clone();  // for the sync-check
        let mut value_kd = self.value.clone();  // for on_key_down
        let value_ctxmenu = self.value.clone(); // for the context menu closures

        // Pull external edits (e.g. cleared after submit) back into the editor.
        if *value_read.read() != editable.editor().read().to_string() {
            let mut editor = editable.editor_mut().write();
            editor.set(&value_read.read());
            editor.editor_history().clear();
            editor.clear_selection();
        }

        let display_placeholder =
            value_read.read().is_empty() && placeholder_text.is_some();

        // ── Key handling ──────────────────────────────────────────────────────
        let on_key_down = move |e: Event<KeyboardEventData>| {
            let key       = e.key.clone();
            let modifiers = e.modifiers;

            if let Key::Named(NamedKey::Enter) = &key {
                // Bare Enter → submit; do NOT forward to editor (no stray \n).
                e.prevent_default();
                e.stop_propagation();
                if let Some(h) = &on_submit {
                    h.call(editable.editor().peek().to_string());
                }
                return;
            }

            editable.process_event(EditableEvent::KeyDown {
                key: &key,
                modifiers,
            });

            // Sync the external Writable<String> and fire on_change.
            let text = editable.editor().read().to_string();
            if *value_kd.peek() != text {
                *value_kd.write() = text.clone();
                if let Some(h) = &on_change {
                    h.call(text);
                }
            }
        };

        let on_key_up = move |e: Event<KeyboardEventData>| {
            e.stop_propagation();
            editable.process_event(EditableEvent::KeyUp { key: &e.key });
        };

        // ── Pointer / caret placement ─────────────────────────────────────────
        let on_focus_press = move |e: Event<FocusPressEventData>| {
            e.stop_propagation();
            e.prevent_default();
            is_dragging.set_if_modified(true);
            if !display_placeholder {
                editable.process_event(EditableEvent::Down {
                    location:    e.element_location(),
                    editor_line: EditorLine::SingleParagraph,
                    holder:      &holder.read(),
                });
            }
            a11y_id.request_focus();
        };

        let on_global_pointer_move = move |e: Event<PointerEventData>| {
            if a11y_id.is_focused() && *is_dragging.read() {
                editable.process_event(EditableEvent::Move {
                    location:    e.element_location(),
                    editor_line: EditorLine::SingleParagraph,
                    holder:      &holder.read(),
                });
            }
        };

        let on_global_pointer_press = move |_: Event<PointerEventData>| {
            editable.process_event(EditableEvent::Release);
            is_dragging.set_if_modified(false);
        };

        // ── Right-click context menu ──────────────────────────────────────────
        // UseEditable is Copy; each inner handler gets its own Writable clone.
        let on_secondary_down = {
            let v_cut   = value_ctxmenu.clone();
            let v_paste = value_ctxmenu.clone();
            move |e: Event<PressEventData>| {
                let mut ed_cut  = editable;
                let     ed_copy = editable;
                let mut ed_paste = editable;
                let mut ed_sel  = editable;
                let mut v_c = v_cut.clone();
                let mut v_p = v_paste.clone();

                ContextMenu::open_from_event(
                    &e,
                    editor_clipboard_menu(
                        th,
                        EventHandler::from(move |_: ()| { cut_selection(&mut ed_cut, &mut v_c); }),
                        EventHandler::from(move |_: ()| { copy_selection(&ed_copy); }),
                        EventHandler::from(move |_: ()| { paste_text(&mut ed_paste, &mut v_p); }),
                        EventHandler::from(move |_: ()| { select_all(&mut ed_sel); }),
                    ),
                );
            }
        };

        // ── Cursor / selection (only while focused) ───────────────────────────
        let (cursor_index, text_selection) = if focus() != Focus::Not {
            (
                Some(editable.editor().read().cursor_pos()),
                editable
                    .editor()
                    .read()
                    .get_visible_selection(EditorLine::SingleParagraph),
            )
        } else {
            (None, None)
        };

        // ── Paragraph (single-line editor) ────────────────────────────────────
        let editor_paragraph = paragraph()
            .a11y_id(a11y_id)
            .a11y_focusable(true)
            .a11y_role(AccessibilityRole::TextInput)
            .holder(holder.read().clone())
            .on_focus_press(on_focus_press)
            .on_key_down(on_key_down)
            .on_key_up(on_key_up)
            .on_global_pointer_press(on_global_pointer_press)
            .on_global_pointer_move(on_global_pointer_move)
            .max_lines(1)
            .min_width(Size::fill())
            .cursor_index(cursor_index)
            .cursor_color(th.accent())
            .color(th.text())
            .line_height(1.35)
            .highlights(text_selection.map(|h| vec![h]))
            .span(editable.editor().read().to_string());

        // ── Outer themed shell ────────────────────────────────────────────────
        rect()
            .width(Size::fill())
            .height(Size::px(36.))
            .corner_radius(CornerRadius::new_all(8.))
            .background(th.surface())
            .border(Border::new().fill(th.surface_max()).width(1.))
            .padding(Gaps::new(0., 10., 0., 10.))
            .main_align(Alignment::Center)
            .on_secondary_down(on_secondary_down)
            .child(
                rect()
                    .width(Size::fill())
                    .main_align(Alignment::Center)
                    .child(editor_paragraph)
                    .maybe_child(display_placeholder.then(|| {
                        placeholder_text.map(|ph| {
                            label()
                                .text(ph)
                                .color(th.faint())
                                .line_height(1.35)
                                .position(Position::new_absolute().top(0.).left(0.))
                        })
                    }).flatten()),
            )
    }

    fn render_key(&self) -> DiffKey {
        self.key.clone().or(self.default_key())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn text_input_mounts_with_placeholder() {
        fn app() -> impl IntoElement {
            let v = use_state(String::new);
            TextInput::new(v.into_writable(), Theme::default())
                .placeholder("Search…")
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Search"))
        });
        assert!(found.is_some(), "placeholder shows when value is empty");
    }

    #[test]
    fn text_input_no_placeholder_without_prop() {
        fn app() -> impl IntoElement {
            let v = use_state(String::new);
            TextInput::new(v.into_writable(), Theme::default())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        // No placeholder label should render when none is set.
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| !l.text.as_ref().is_empty())
        });
        assert!(found.is_none(), "no stray label when placeholder not set");
    }
}
