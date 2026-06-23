//! A chat message bubble styled per the Collapsible-Panels design.
//!
//! User turns (`role == "user"`) align right with an accent-tint fill and
//! per-corner tail `{14,14,4,14}`.
//! Assistant turns align left, preceded by an `Avatar`, with a surface bubble
//! and per-corner tail `{14,14,14,4}`.
use freya::prelude::*;

use crate::components::avatar::Avatar;
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
        if self.role == "user" {
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
                        .child(label().text(self.text.clone()).font_size(13.).color(th.text())),
                )
        } else {
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
                        .child(
                            label().text(self.text.clone()).font_size(13.).color(th.subtext_hi()),
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
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "hi there")
        });
        assert!(found.is_some(), "bubble should render its text");
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
