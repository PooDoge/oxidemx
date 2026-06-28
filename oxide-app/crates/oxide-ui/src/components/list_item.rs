//! A conversation/project row for the sidebar list.
//!
//! Selected rows are highlighted with a tinted accent fill + border.
//! A leading tone dot indicates the conversation state. An optional trailing
//! `WorktreeChip` shows the active worktree branch.
//!
//! Press the row to fire the optional `on_press` handler.
use bytes::Bytes;
use freya::prelude::*;

use crate::tokens::Theme;

/// A single row in a conversation or project list.
///
/// Builder usage:
/// ```ignore
/// ListItem::new("My Project".into())
///     .selected(true)
///     .state("working".into())
///     .worktree(Some("main".into()))
///     .on_press(|_| println!("pressed"))
/// ```
#[derive(PartialEq, Clone)]
pub struct ListItem {
    label: String,
    icon: Option<Bytes>,
    selected: bool,
    state: String,
    worktree: Option<String>,
    on_press: Option<EventHandler<Event<PressEventData>>>,
    theme: Theme,
}

impl ListItem {
    pub fn new(label: String) -> Self {
        Self {
            label,
            icon: None,
            selected: false,
            state: "idle".into(),
            worktree: None,
            on_press: None,
            theme: Theme::default(),
        }
    }

    pub fn icon(mut self, icon: Option<Bytes>) -> Self {
        self.icon = icon;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn state(mut self, state: String) -> Self {
        self.state = state;
        self
    }

    pub fn worktree(mut self, worktree: Option<String>) -> Self {
        self.worktree = worktree;
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
        let th = self.theme;
        let tone = match self.state.as_str() {
            "working"   => th.yellow(),
            "delivered" => th.green(),
            "failed"    => th.red(),
            _           => th.overlay(),
        };
        let (bg, txt, border) = if self.selected {
            (
                Theme::with_alpha(th.accent(), 0x14),
                th.text(),
                Theme::with_alpha(th.accent(), 0x3a),
            )
        } else {
            (
                Color::from_argb(0, 0, 0, 0),
                th.subtext_hi(),
                Color::from_argb(0, 0, 0, 0),
            )
        };
        let row = rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.)
            .width(Size::fill())
            .padding(Gaps::new(8., 10., 8., 10.))
            .corner_radius(CornerRadius::new_all(9.))
            .background(bg)
            .border(Border::new().fill(border).width(1.))
            .child(
                rect()
                    .width(Size::px(6.))
                    .height(Size::px(6.))
                    .corner_radius(CornerRadius::new_all(3.))
                    .background(tone),
            )
            .maybe_child(self.icon.clone().map(|bytes| {
                svg(bytes)
                    .width(Size::px(16.))
                    .height(Size::px(16.))
                    .color(txt)
            }))
            .child(
                label()
                    .text(self.label.clone())
                    .font_size(12.5)
                    .color(txt)
                    .width(Size::flex(1.0)),
            )
            .maybe_child(
                self.worktree
                    .clone()
                    .map(|w| crate::components::chip::WorktreeChip::new(w).theme(th)),
            );
        if let Some(handler) = self.on_press.clone() {
            row.on_press(handler)
        } else {
            row
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

    #[test]
    fn list_item_with_worktree_renders_branch() {
        fn app() -> impl IntoElement {
            ListItem::new("Chat".into())
                .state("working".into())
                .worktree(Some("wt-x".into()))
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        assert!(
            t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "Chat"))
                .is_some(),
            "should render the title"
        );
        assert!(
            t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "wt-x"))
                .is_some(),
            "should render the worktree chip label"
        );
    }
}
