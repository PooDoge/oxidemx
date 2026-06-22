use freya::prelude::*;
use crate::tokens::Theme;

#[derive(PartialEq, Clone)]
pub struct WorktreeChip {
    label: String,
    theme: Theme,
}

impl WorktreeChip {
    pub fn new(label: String) -> Self {
        Self { label, theme: Theme::default() }
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }
}

impl Component for WorktreeChip {
    fn render(&self) -> impl IntoElement {
        let a = self.theme.accent();
        rect()
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .padding(Gaps::new(1., 7., 1., 7.))
            .corner_radius(CornerRadius::new_all(999.))
            .background(Theme::with_alpha(a, 0x14))
            .border(Border::new().fill(Theme::with_alpha(a, 0x33)).width(1.))
            .child(label().text(self.label.clone()).font_size(10.5).color(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn worktree_chip_renders_branch() {
        fn app() -> impl IntoElement {
            WorktreeChip::new("main".to_string())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "main")
        });
        assert!(found.is_some(), "WorktreeChip should render the branch label");
    }
}
