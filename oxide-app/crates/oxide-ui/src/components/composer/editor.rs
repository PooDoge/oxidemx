//! `ComposerEditor` — the multiline chat editor built directly on freya's
//! low-level `use_editable` engine.
//!
//! The built-in `Input` component is single-line (`max_lines(1)` + Enter-as-submit)
//! and cannot host a growing, newline-bearing composer, so this builds on the same
//! `use_editable` + `paragraph()` shape that `Input` uses, but:
//!
//! - renders the paragraph WITHOUT `max_lines(1)` so embedded `\n` wraps onto real
//!   lines (the rope editor is inherently multiline; `EditorLine::SingleParagraph`
//!   just means "one `paragraph` element renders the whole rope");
//! - intercepts `Enter` BEFORE forwarding to `editable.process_event` so that, when
//!   `send_on_enter == true`, a bare `Enter` submits instead of inserting `\n`
//!   (`Shift+Enter`, or any `Enter` when `send_on_enter == false`, still inserts a
//!   newline — the rope editor inserts `\n` on `NamedKey::Enter` by default);
//! - auto-grows the body up to `config.cap_px()` and then scrolls
//!   (`ScrollView { show_scrollbar(false) }`), or honours a caller-supplied
//!   `manual_height` (clamped via [`clamp_height`]);
//! - keeps the external `value: Writable<String>` in sync on every edit and emits
//!   the measured content height via `on_height` so the Composer can show/hide its
//!   resize grip.
//!
//! Caret colour is themed to `theme.accent()` via `paragraph().cursor_color(..)`.
use freya::prelude::*;
use freya::text_edit::*;

use crate::components::composer::ComposerConfig;
use crate::components::resize_grip::clamp_height;
use crate::tokens::Theme;

const PLACEHOLDER: &str = "Ask, or type / for a flow…";

/// A multiline chat editor on freya's `use_editable` engine.
///
/// Builder usage:
/// ```ignore
/// fn app() -> impl IntoElement {
///     let value = use_state(String::new);
///     ComposerEditor::new(value.into_writable(), ComposerConfig::default(), Theme::default())
///         .send_on_enter(true)
///         .on_submit(|text| println!("submit: {text}"))
///         .on_height(|h| println!("content height: {h}"))
/// }
/// ```
#[derive(Clone, PartialEq)]
pub struct ComposerEditor {
    value: Writable<String>,
    config: ComposerConfig,
    theme: Theme,
    send_on_enter: bool,
    on_submit: Option<EventHandler<String>>,
    on_height: Option<EventHandler<f32>>,
    manual_height: Option<f32>,
    key: DiffKey,
}

impl KeyExt for ComposerEditor {
    fn write_key(&mut self) -> &mut DiffKey {
        &mut self.key
    }
}

impl ComposerEditor {
    pub fn new(value: Writable<String>, config: ComposerConfig, theme: Theme) -> Self {
        Self {
            value,
            config,
            theme,
            send_on_enter: true,
            on_submit: None,
            on_height: None,
            manual_height: None,
            key: DiffKey::default(),
        }
    }

    /// When `true` (default), a bare `Enter` submits and does NOT insert a newline.
    /// When `false`, `Enter` inserts a newline like `Shift+Enter`.
    pub fn send_on_enter(mut self, send_on_enter: bool) -> Self {
        self.send_on_enter = send_on_enter;
        self
    }

    pub fn on_submit(mut self, handler: impl Into<EventHandler<String>>) -> Self {
        self.on_submit = Some(handler.into());
        self
    }

    /// Emits the measured content height (px) on every edit so the Composer can
    /// show/hide the resize grip and clamp the panel.
    pub fn on_height(mut self, handler: impl Into<EventHandler<f32>>) -> Self {
        self.on_height = Some(handler.into());
        self
    }

    /// Overrides the auto-grown height (e.g. when the user has dragged the grip).
    /// The value is clamped via [`clamp_height`] against `config.cap_px()`.
    pub fn manual_height(mut self, manual_height: Option<f32>) -> Self {
        self.manual_height = manual_height;
        self
    }
}

impl Component for ComposerEditor {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let cap = self.config.cap_px();
        let send_on_enter = self.send_on_enter;
        let on_submit = self.on_submit.clone();
        let on_height = self.on_height.clone();

        let a11y_id = use_hook(AccessibilityId::new_unique);
        let focus = use_focus(a11y_id);
        let holder = use_state(ParagraphHolder::default);
        let mut editable = use_editable(
            || self.value.read().to_string(),
            EditableConfig::new,
        );
        let mut is_dragging = use_state(|| false);
        let mut content_h = use_state(|| 0.0_f32);
        let mut value = self.value.clone();

        // ScrollController for the inner body: drives scroll-to-End when the editor
        // content overflows the cap and the user is typing (follow-the-caret).
        // Default position is End so the initial render of a pre-filled editor
        // also shows the bottom of the content.
        let mut scroll_ctrl = use_scroll_controller(|| ScrollConfig {
            default_vertical_position: ScrollPosition::End,
            default_horizontal_position: ScrollPosition::Start,
        });

        // Pull external edits (e.g. the value was cleared after a submit) back into
        // the editor so the two stay in sync in both directions.
        if *value.read() != editable.editor().read().to_string() {
            let mut editor = editable.editor_mut().write();
            editor.set(&value.read());
            editor.editor_history().clear();
            editor.clear_selection();
        }

        let display_placeholder = value.read().is_empty();

        // ── Edit / key handling ───────────────────────────────────────────────
        // Enter is intercepted BEFORE `editable.process_event` so a bare Enter can
        // submit (and NOT leave a stray `\n`) when `send_on_enter` is set.
        let on_key_down = move |e: Event<KeyboardEventData>| {
            let key = e.key.clone();
            let modifiers = e.modifiers;

            if let Key::Named(NamedKey::Enter) = &key {
                let insert_newline = modifiers.shift() || !send_on_enter;
                if !insert_newline {
                    // Submit: do NOT forward to the editor (prevents the `\n`).
                    e.stop_propagation();
                    e.prevent_default();
                    if let Some(on_submit) = &on_submit {
                        on_submit.call(editable.editor().peek().to_string());
                    }
                    return;
                }
                // else: fall through and let the editor insert a newline.
            }

            editable.process_event(EditableEvent::KeyDown {
                key: &key,
                modifiers,
            });

            // Keep the external Writable<String> in sync on every edit.
            let text = editable.editor().read().to_string();
            if *value.peek() != text {
                *value.write() = text;
            }

            // Follow the caret: when the body is scrollable (content taller than
            // cap), scroll to End so the current line stays visible after every
            // keystroke.  We only do this when scrollable to avoid disturbing
            // short editors that never overflow.
            if *content_h.peek() > cap {
                scroll_ctrl.scroll_to(ScrollPosition::End, Direction::Vertical);
            }

            // SEAM (Slice 2): live markdown-on-space rendering + ghost inline
            // completion + Tab-accept of predictions hook in here, after the edit
            // is applied and `value` is synced. No logic yet (deferred).
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
                    location: e.element_location(),
                    editor_line: EditorLine::SingleParagraph,
                    holder: &holder.read(),
                });
            }
            a11y_id.request_focus();
        };

        let on_global_pointer_move = move |e: Event<PointerEventData>| {
            if a11y_id.is_focused() && *is_dragging.read() {
                editable.process_event(EditableEvent::Move {
                    location: e.element_location(),
                    editor_line: EditorLine::SingleParagraph,
                    holder: &holder.read(),
                });
            }
        };

        let on_global_pointer_press = move |_: Event<PointerEventData>| {
            editable.process_event(EditableEvent::Release);
            is_dragging.set_if_modified(false);
        };

        // ── Height measurement + auto-grow ────────────────────────────────────
        let on_sized = move |e: Event<SizedEventData>| {
            let measured = e.area.height();
            if (*content_h.peek() - measured).abs() > 0.5 {
                content_h.set(measured);
                if let Some(on_height) = &on_height {
                    on_height.call(measured);
                }
            }
        };

        // The body grows with content up to `cap`, then `ScrollView` scrolls it.
        // `manual_height` (if set) overrides, clamped against the cap.
        //
        // V_PAD accounts for the 8px top + 8px bottom padding (`Gaps::new(8., 12.,
        // 8., 12.)`) around the paragraph inside the ScrollView.  Without it the outer
        // rect is 16px too short and descenders on the bottom line are clipped.
        //
        // Strategy:
        // - While the padded content (paragraph + V_PAD) still fits within the cap,
        //   use `Size::Inner` — the box sizes itself to exactly the paragraph + padding,
        //   no clipping and no fixed height needed.
        // - Once the paragraph alone would exceed the cap, pin the outer rect to `cap`
        //   and let the inner ScrollView scroll the overflowing content.
        const V_PAD: f32 = 16.0; // 8px top + 8px bottom
        let body_height: Option<f32> = match self.manual_height {
            Some(h) => Some(clamp_height(h, cap)),
            None => {
                let c = content_h();
                if c <= 0.0 || c + V_PAD <= cap {
                    None // → Size::Inner: box sizes to padding + paragraph, no clip
                } else {
                    Some(cap) // text alone exceeds the cap → fixed cap, ScrollView scrolls
                }
            }
        };

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

        // The paragraph that renders the whole rope. NOTE: no `max_lines(1)` — this
        // is what makes the editor multiline (embedded `\n` becomes real lines).
        let editor_paragraph = paragraph()
            .a11y_id(a11y_id)
            .a11y_focusable(true)
            .a11y_role(AccessibilityRole::TextInput)
            .holder(holder.read().clone())
            .on_sized(on_sized)
            .on_focus_press(on_focus_press)
            .on_key_down(on_key_down)
            .on_key_up(on_key_up)
            .on_global_pointer_press(on_global_pointer_press)
            .on_global_pointer_move(on_global_pointer_move)
            .min_width(Size::fill())
            .cursor_index(cursor_index)
            .cursor_color(th.accent())
            .color(th.text())
            .line_height(1.35)
            .highlights(text_selection.map(|h| vec![h]))
            .span(editable.editor().read().to_string());

        // Placeholder overlay: only the placeholder label is shown when empty; it
        // sits in the same box (a Stack) so the caret still renders over it.
        rect()
            .width(Size::fill())
            .height(match body_height {
                Some(h) => Size::px(h),
                None    => Size::Inner,
            })
            .child(
                ScrollView::new_controlled(scroll_ctrl)
                    .width(Size::fill())
                    .height(Size::fill())
                    .show_scrollbar(false)
                    .child(
                        rect()
                            .width(Size::fill())
                            .padding(Gaps::new(8., 12., 8., 12.))
                            .child(editor_paragraph)
                            .maybe_child(display_placeholder.then(|| {
                                label()
                                    .text(PLACEHOLDER)
                                    .color(th.faint())
                                    .line_height(1.35)
                                    .position(Position::new_absolute().top(8.).left(12.))
                            })),
                    ),
            )
    }

    fn render_key(&self) -> DiffKey {
        self.key.clone().or(self.default_key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::composer::ComposerConfig;
    use crate::tokens::Theme;
    use freya_testing::prelude::*;

    #[test]
    fn editor_mounts_with_placeholder() {
        fn app() -> impl IntoElement {
            let v = use_state(String::new);
            ComposerEditor::new(v.into_writable(), ComposerConfig::default(), Theme::default())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Ask, or type"))
        });
        assert!(found.is_some(), "placeholder shows when empty");
    }
}
