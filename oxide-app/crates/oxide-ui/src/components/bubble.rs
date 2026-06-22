//! A chat message bubble.
//!
//! User turns (`role == "user"`) align right with the accent tint.
//! Assistant turns align left on the surface color.
use freya::prelude::*;

use crate::tokens::Theme;

/// A chat message bubble that colors itself based on `role`.
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
        let is_user = self.role == "user";
        let bg = if is_user { self.theme.accent() } else { self.theme.surface() };
        let align = if is_user { Alignment::End } else { Alignment::Start };
        rect()
            .width(Size::fill())
            .main_align(align)
            .direction(Direction::Horizontal)
            .child(
                rect()
                    .padding(Gaps::new_all(10.))
                    .corner_radius(CornerRadius::new_all(10.))
                    .background(bg)
                    .child(label().text(self.text.clone()).color(self.theme.text())),
            )
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
}
