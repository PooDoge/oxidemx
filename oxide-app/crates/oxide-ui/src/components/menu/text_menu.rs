//! Clipboard-op helpers and menu builders for text editing surfaces.
//!
//! # Clipboard helpers
//!
//! Each helper operates directly on a [`UseEditable`] handle, mirroring the
//! exact logic Freya's built-in key handler uses for Ctrl+C/X/V/A.  Indices
//! are **utf-16 code units** (same convention as the rope editor).
//!
//! # Menu builders
//!
//! [`editor_clipboard_menu`] — four-item Cut/Copy/Paste/Select All menu for a
//! writable editor.
//!
//! [`copy_only_menu`] — single-item copy menu for read-only displays (e.g.
//! "Copy message" on a chat bubble).
//!
//! Callers supply plain `EventHandler<()>` handlers; the builders wrap them
//! in the `EventHandler<Event<PressEventData>>` that `MenuButton::on_press`
//! requires.
use freya::prelude::*;
use freya::text_edit::{TextEditor, UseEditable};

use crate::components::menu::theme::menu_theme;
use crate::tokens::Theme;

// ── Clipboard-op helpers ──────────────────────────────────────────────────────

/// Copy the current selection to the system clipboard.
///
/// No-op when nothing is selected or the clipboard call fails.
pub fn copy_selection(editable: &UseEditable) {
    let selected = editable.editor().read().get_selected_text();
    if let Some(s) = selected {
        if !s.is_empty() {
            let _ = Clipboard::set(s);
        }
    }
}

/// Cut the current selection: copy it then delete it, syncing `value`.
///
/// No-op when nothing is selected.
pub fn cut_selection(editable: &mut UseEditable, value: &mut Writable<String>) {
    let range = editable.editor().read().get_selection_range();
    let text = editable.editor().read().get_selected_text();
    if let (Some((start, end)), Some(t)) = (range, text) {
        if !t.is_empty() {
            let _ = Clipboard::set(t);
            editable.editor_mut().write().remove(start..end);
            editable.editor_mut().write().move_cursor_to(start);
            let updated = editable.editor().read().to_string();
            *value.write() = updated;
        }
    }
}

/// Paste text from the system clipboard at the cursor, syncing `value`.
///
/// Replaces any current selection before inserting. No-op on clipboard error.
/// Note: image-aware paste (for the composer) wraps this helper with its own
/// image check and calls it only when the clipboard holds plain text.
pub fn paste_text(editable: &mut UseEditable, value: &mut Writable<String>) {
    if let Ok(t) = Clipboard::get() {
        // Delete current selection first (matches built-in Ctrl+V behaviour).
        if let Some((start, end)) = editable.editor().read().get_selection_range() {
            editable.editor_mut().write().remove(start..end);
            editable.editor_mut().write().move_cursor_to(start);
        }
        let pos = editable.editor().read().cursor_pos();
        let inserted = editable.editor_mut().write().insert(&t, pos);
        editable.editor_mut().write().move_cursor_to(pos + inserted);
        let updated = editable.editor().read().to_string();
        *value.write() = updated;
    }
}

/// Select all text in the editor.
pub fn select_all(editable: &mut UseEditable) {
    let len = editable.editor().read().len_utf16_cu();
    editable.editor_mut().write().set_selection((0, len));
}

// ── Menu builders ─────────────────────────────────────────────────────────────

/// Build a four-item clipboard menu (Cut / Copy / Paste / Select All).
///
/// Handlers are `EventHandler<()>`; the builder wraps each in the press
/// handler shape `MenuButton::on_press` expects.
pub fn editor_clipboard_menu(
    theme: Theme,
    on_cut: EventHandler<()>,
    on_copy: EventHandler<()>,
    on_paste: EventHandler<()>,
    on_select_all: EventHandler<()>,
) -> Menu {
    let (container_theme, item_theme) = menu_theme(theme);

    Menu::new()
        .theme(container_theme)
        .child(
            MenuButton::new()
                .theme(item_theme.clone())
                .on_press(move |_: Event<PressEventData>| on_cut.call(()))
                .child("Cut"),
        )
        .child(
            MenuButton::new()
                .theme(item_theme.clone())
                .on_press(move |_: Event<PressEventData>| on_copy.call(()))
                .child("Copy"),
        )
        .child(
            MenuButton::new()
                .theme(item_theme.clone())
                .on_press(move |_: Event<PressEventData>| on_paste.call(()))
                .child("Paste"),
        )
        .child(
            MenuButton::new()
                .theme(item_theme)
                .on_press(move |_: Event<PressEventData>| on_select_all.call(()))
                .child("Select All"),
        )
}

/// Build a single-item copy menu for read-only displays (e.g. chat bubbles).
///
/// `label` is typically `"Copy message"` or similar.
pub fn copy_only_menu(theme: Theme, label: &str, on_copy: EventHandler<()>) -> Menu {
    let (container_theme, item_theme) = menu_theme(theme);
    let label = label.to_string();

    Menu::new()
        .theme(container_theme)
        .child(
            MenuButton::new()
                .theme(item_theme)
                .on_press(move |_: Event<PressEventData>| on_copy.call(()))
                .child(label),
        )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn editor_menu_has_four_clipboard_actions() {
        fn app() -> impl IntoElement {
            let noop = EventHandler::from(|_: ()| {});
            editor_clipboard_menu(Theme::default(), noop.clone(), noop.clone(), noop.clone(), noop)
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        for lbl in ["Cut", "Copy", "Paste", "Select All"] {
            assert!(
                t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == lbl))
                    .is_some(),
                "missing label: {lbl}"
            );
        }
    }

    #[test]
    fn copy_only_menu_has_label() {
        fn app() -> impl IntoElement {
            copy_only_menu(Theme::default(), "Copy message", EventHandler::from(|_: ()| {}))
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        assert!(
            t.find(|_, el| {
                Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Copy message"))
            })
            .is_some(),
            "missing 'Copy message' label"
        );
    }
}
