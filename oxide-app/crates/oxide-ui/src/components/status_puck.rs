use freya::prelude::*;
use crate::tokens::Theme;

#[derive(PartialEq, Clone)]
pub struct StatusPuck {
    state: String,
    theme: Theme,
}

impl StatusPuck {
    pub fn new(state: &str) -> Self {
        Self { state: state.to_string(), theme: Theme::default() }
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }

    fn tone(&self) -> Color {
        match self.state.as_str() {
            "working"   => self.theme.yellow(),
            "delivered" => self.theme.green(),
            "failed"    => self.theme.red(),
            _           => self.theme.overlay(),
        }
    }
}

impl Component for StatusPuck {
    fn render(&self) -> impl IntoElement {
        let tone = self.tone();
        rect()
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .padding(Gaps::new(5., 11., 5., 11.))
            .corner_radius(CornerRadius::new_all(999.))
            .background(Theme::with_alpha(tone, 0x14))
            .border(Border::new().fill(Theme::with_alpha(tone, 0x3a)).width(1.))
            .child(
                rect()
                    .width(Size::px(7.))
                    .height(Size::px(7.))
                    .corner_radius(CornerRadius::new_all(4.))
                    .background(tone),
            )
            .child(
                label()
                    .text(self.state.clone())
                    .font_size(11.5)
                    .font_weight(FontWeight::SEMI_BOLD)
                    .color(tone),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn status_puck_renders_state_label() {
        fn app() -> impl IntoElement {
            StatusPuck::new("working")
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "working")
        });
        assert!(found.is_some(), "StatusPuck should render its state label");
    }
}
