//! The thread header: title + optional worktree chip + a status puck.
use freya::prelude::*;
use crate::components::{status_puck::StatusPuck, chip::WorktreeChip};
use crate::tokens::Theme;

#[derive(PartialEq, Clone)]
pub struct ThreadHeader {
    title: String,
    state: String,
    worktree: Option<String>,
    theme: Theme,
}

impl ThreadHeader {
    pub fn new(title: String, state: String, worktree: Option<String>) -> Self {
        Self { title, state, worktree, theme: Theme::default() }
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }
}

impl Component for ThreadHeader {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let title_col = rect()
            .direction(Direction::Vertical)
            .width(Size::flex(1.0))
            .spacing(3.)
            .child(
                label()
                    .text(self.title.clone())
                    .font_size(15.)
                    .font_weight(FontWeight::SEMI_BOLD)
                    .color(th.text()),
            )
            .maybe_child(self.worktree.clone().map(|w| WorktreeChip::new(w).theme(th)));
        rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .width(Size::fill())
            .spacing(12.)
            .padding(Gaps::new(12., 18., 12., 18.))
            .background(th.bg())
            .child(title_col)
            .child(StatusPuck::new(&self.state).theme(th))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn header_renders_title_and_state() {
        fn app() -> impl IntoElement {
            ThreadHeader::new("My chat".into(), "working".into(), Some("wt-x".into()))
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        assert!(
            t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "My chat"))
                .is_some(),
            "ThreadHeader should render the title label"
        );
        assert!(
            t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "working"))
                .is_some(),
            "ThreadHeader should render the state label via StatusPuck"
        );
    }
}
