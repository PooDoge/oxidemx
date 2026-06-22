//! A conversation/project row for the sidebar list.
//!
//! Selected rows are highlighted with the accent color. Press the row to fire
//! the optional `on_press` handler.
use freya::prelude::*;

use crate::tokens::Theme;

/// A single row in a conversation or project list.
///
/// Builder usage:
/// ```ignore
/// ListItem::new("My Project".into())
///     .selected(true)
///     .on_press(|_| println!("pressed"))
/// ```
#[derive(PartialEq, Clone)]
pub struct ListItem {
    label: String,
    selected: bool,
    on_press: Option<EventHandler<Event<PressEventData>>>,
    theme: Theme,
}

impl ListItem {
    pub fn new(label: String) -> Self {
        Self { label, selected: false, on_press: None, theme: Theme::default() }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
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

impl Component for ListItem {
    fn render(&self) -> impl IntoElement {
        let bg = if self.selected {
            self.theme.accent()
        } else {
            self.theme.surface()
        };
        let text_color = if self.selected {
            self.theme.bg()
        } else {
            self.theme.text()
        };
        let base = rect()
            .width(Size::fill())
            .padding(Gaps::new_all(10.))
            .background(bg)
            .child(label().text(self.label.clone()).color(text_color));

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
    fn list_item_renders_label() {
        fn app() -> impl IntoElement {
            ListItem::new("Conversation A".into())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "Conversation A")
        });
        assert!(found.is_some(), "list item should render its label");
    }
}
