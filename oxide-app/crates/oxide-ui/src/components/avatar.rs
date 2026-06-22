use freya::prelude::*;
use crate::tokens::Theme;

#[derive(PartialEq, Clone)]
pub struct Avatar {
    theme: Theme,
}

impl Avatar {
    pub fn new() -> Self {
        Self { theme: Theme::default() }
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }
}

impl Default for Avatar {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Avatar {
    fn render(&self) -> impl IntoElement {
        let accent = self.theme.accent();
        let accent_dim = self.theme.accent_dim();
        let bg_deep = self.theme.bg_deep();
        rect()
            .width(Size::px(26.))
            .height(Size::px(26.))
            .corner_radius(CornerRadius::new_all(8.))
            .main_align(Alignment::Center)
            .cross_align(Alignment::Center)
            .background_linear_gradient(
                LinearGradient::new()
                    .angle(150.)
                    .stop((accent, 0.))
                    .stop((accent_dim, 100.)),
            )
            .child(label().text("✦").font_size(14.).color(bg_deep))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn avatar_renders_sparkle() {
        fn app() -> impl IntoElement {
            Avatar::new()
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "✦")
        });
        assert!(found.is_some(), "Avatar should render the sparkle glyph");
    }
}
