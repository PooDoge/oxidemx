//! A chat message bubble styled per the Collapsible-Panels design.
//!
//! User turns (`role == "user"`) align right with an accent-tint fill and
//! per-corner tail `{14,14,4,14}`.
//! Assistant turns align left, preceded by an `Avatar`, with a surface bubble
//! and per-corner tail `{14,14,14,4}`.
//!
//! Body text is rendered with `SelectableText` (drag-select + Ctrl+C built in).
//! Right-clicking the bubble body opens a "Copy message" context menu via
//! `ContextMenu::open_from_event`.  A `ContextMenuViewer` must be mounted in
//! an ancestor scope (the shell already mounts one at its root).
use freya::prelude::*;

use crate::components::avatar::Avatar;
use crate::components::menu::copy_only_menu;
use crate::tokens::Theme;

/// A chat message bubble that styles itself based on `role`.
///
/// Builder usage:
/// ```ignore
/// Bubble::new("assistant".into(), "Hello!".into())
///     .theme(custom_theme)
/// ```
#[derive(PartialEq, Clone)]
pub struct Bubble {
    role: String,
    text: String,
    theme: Theme,
}

impl Bubble {
    pub fn new(role: String, text: String) -> Self {
        Self { role, text, theme: Theme::default() }
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }
}

impl Component for Bubble {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let body = self.text.clone();
        if self.role == "user" {
            let body_for_menu = body.clone();
            rect()
                .width(Size::fill())
                .direction(Direction::Horizontal)
                .main_align(Alignment::End)
                .child(
                    rect()
                        .width(Size::percent(78.))
                        .padding(Gaps::new(10., 14., 10., 14.))
                        .corner_radius(CornerRadius {
                            top_left: 14.,
                            top_right: 14.,
                            bottom_right: 4.,
                            bottom_left: 14.,
                            smoothing: 0.,
                        })
                        .background(Theme::with_alpha(th.accent(), 0x1a))
                        .border(Border::new().fill(Theme::with_alpha(th.accent(), 0x33)).width(1.))
                        .on_secondary_down(move |e: Event<PressEventData>| {
                            let text = body_for_menu.clone();
                            crate::components::menu::open_context_menu(
                                &e,
                                copy_only_menu(
                                    th,
                                    "Copy message",
                                    EventHandler::from(move |_: ()| {
                                        let _ = Clipboard::set(text.clone());
                                    }),
                                ),
                            );
                        })
                        .child(
                            SelectableText::new().span(body.clone())
                                .font_size(13.)
                                .color(th.text()),
                        ),
                )
        } else {
            let body_for_menu = body.clone();
            rect()
                .width(Size::fill())
                .direction(Direction::Horizontal)
                .main_align(Alignment::Start)
                .spacing(10.)
                .child(Avatar::new().theme(th))
                .child(
                    rect()
                        .width(Size::percent(82.))
                        .padding(Gaps::new(10., 14., 10., 14.))
                        .corner_radius(CornerRadius {
                            top_left: 14.,
                            top_right: 14.,
                            bottom_right: 14.,
                            bottom_left: 4.,
                            smoothing: 0.,
                        })
                        .background(th.surface())
                        .border(Border::new().fill(th.hairline()).width(1.))
                        .on_secondary_down(move |e: Event<PressEventData>| {
                            let text = body_for_menu.clone();
                            crate::components::menu::open_context_menu(
                                &e,
                                copy_only_menu(
                                    th,
                                    "Copy message",
                                    EventHandler::from(move |_: ()| {
                                        let _ = Clipboard::set(text.clone());
                                    }),
                                ),
                            );
                        })
                        .child(
                            SelectableText::new().span(body.clone())
                                .font_size(13.)
                                .color(th.subtext_hi()),
                        ),
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn bubble_renders_text() {
        fn app() -> impl IntoElement {
            Bubble::new("assistant".into(), "hi there".into())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        // Body is now rendered by SelectableText, which emits a `paragraph` element
        // with the content in its `spans` list.
        let found = t.find(|_, el| {
            Paragraph::try_downcast(el)
                .filter(|p| p.spans.iter().any(|s| s.text == "hi there"))
        });
        assert!(found.is_some(), "bubble should render its text via SelectableText paragraph");
    }

    #[test]
    fn assistant_bubble_has_avatar_user_does_not() {
        fn ass() -> impl IntoElement {
            Bubble::new("assistant".into(), "hi".into())
        }
        fn usr() -> impl IntoElement {
            Bubble::new("user".into(), "hi".into())
        }
        let mut a = launch_test(ass);
        a.sync_and_update();
        assert!(
            a.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "✦")).is_some(),
            "assistant bubble should contain the Avatar sparkle glyph"
        );
        let mut u = launch_test(usr);
        u.sync_and_update();
        assert!(
            u.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "✦")).is_none(),
            "user bubble must NOT contain the Avatar sparkle glyph"
        );
    }
}
