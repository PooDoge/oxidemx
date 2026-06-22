//! A compact icon button for use in the navigation rail.
//!
//! Renders a glyph (unicode symbol or short text) inside a fixed-size
//! pressable square. Wire `on_press` for navigation or action.
use freya::prelude::*;

use crate::tokens::Theme;

/// A compact icon/glyph button for the rail navigation.
///
/// Builder usage:
/// ```ignore
/// RailButton::new("⚙".into())
///     .on_press(|_| println!("settings"))
/// ```
#[derive(PartialEq, Clone)]
pub struct RailButton {
    glyph: String,
    on_press: Option<EventHandler<Event<PressEventData>>>,
    theme: Theme,
}

impl RailButton {
    pub fn new(glyph: String) -> Self {
        Self { glyph, on_press: None, theme: Theme::default() }
    }

    pub fn on_press(mut self, handler: impl Into<EventHandler<Event<PressEventData>>>) -> Self {
        self.on_press = Some(handler.into());
        self
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }
}

impl Component for RailButton {
    fn render(&self) -> impl IntoElement {
        let base = rect()
            .width(Size::px(44.))
            .height(Size::px(44.))
            .corner_radius(CornerRadius::new_all(8.))
            .background(self.theme.surface())
            .center()
            .child(label().text(self.glyph.clone()).color(self.theme.text()));

        if let Some(handler) = self.on_press.clone() {
            base.on_press(handler)
        } else {
            base
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn rail_button_renders_glyph() {
        fn app() -> impl IntoElement {
            RailButton::new("⚙".into())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "⚙")
        });
        assert!(found.is_some(), "rail button should render its glyph");
    }
}
