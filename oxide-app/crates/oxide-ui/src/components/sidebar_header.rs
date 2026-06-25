//! Sidebar header: project switcher pill + search input + new-conversation button.
//!
//! The switcher is static in P2 (dropdown menu is future). The search input
//! is wired to local state only — search wiring is future.
use freya::prelude::*;

use crate::components::TextInput;
use crate::tokens::Theme;

/// Top-of-sidebar header: project switcher row + search + new conversation button.
///
/// Builder usage:
/// ```ignore
/// SidebarHeader::new("oxidemx-phase1".into())
///     .theme(theme)
/// ```
#[derive(PartialEq, Clone)]
pub struct SidebarHeader {
    project: String,
    theme: Theme,
}

impl SidebarHeader {
    pub fn new(project: String) -> Self {
        Self { project, theme: Theme::default() }
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }
}

impl Component for SidebarHeader {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let value = use_state(String::new);

        // Project switcher pill: surface bg + hairline_strong border + radius 9
        let switcher = rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.)
            .width(Size::fill())
            .padding(Gaps::new(8., 10., 8., 10.))
            .corner_radius(CornerRadius::new_all(9.))
            .background(th.surface())
            .border(Border::new().fill(th.hairline_strong()).width(1.))
            // small accent dot
            .child(
                rect()
                    .width(Size::px(6.))
                    .height(Size::px(6.))
                    .corner_radius(CornerRadius::new_all(3.))
                    .background(th.accent()),
            )
            // project name label
            .child(
                label()
                    .text(self.project.clone())
                    .font_size(12.5)
                    .color(th.text())
                    .width(Size::flex(1.0)),
            )
            // chevron
            .child(label().text("⌄").font_size(12.).color(th.faint()));

        // Search input — local state, non-functional search (wiring is future).
        // Migrated from Freya `Input` to our themed `TextInput` (Task 3) so it
        // gains design-token colouring and the right-click clipboard menu.
        let search = TextInput::new(value.into_writable(), th)
            .placeholder("Search…");

        // New conversation button — filled, accent background
        let new_btn = Button::new()
            .filled()
            .theme_colors(ButtonColorsThemePartial {
                background: Some(Preference::Specific(th.accent())),
                hover_background: Some(Preference::Specific(th.accent_hi())),
                border_fill: Some(Preference::Specific(Color::from_argb(0, 0, 0, 0))),
                focus_border_fill: Some(Preference::Specific(th.accent_hi())),
                color: Some(Preference::Specific(th.bg_deep())),
            })
            .child(label().text("+ New").font_size(12.5).color(th.bg_deep()));

        rect()
            .direction(Direction::Vertical)
            .spacing(8.)
            .padding(Gaps::new_all(8.))
            .width(Size::fill())
            .child(switcher)
            .child(search)
            .child(new_btn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn sidebar_header_renders_project_and_new() {
        fn app() -> impl IntoElement {
            SidebarHeader::new("oxidemx-phase1".into())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        assert!(
            t.find(|_, el| {
                Label::try_downcast(el)
                    .filter(|l| l.text.as_ref().contains("oxidemx-phase1"))
            })
            .is_some(),
            "SidebarHeader should render the project name"
        );
        assert!(
            t.find(|_, el| {
                Label::try_downcast(el).filter(|l| l.text.as_ref().contains("New"))
            })
            .is_some(),
            "SidebarHeader should render the '+ New' button label"
        );
        // Verify the migrated TextInput renders its placeholder label.
        assert!(
            t.find(|_, el| {
                Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Search"))
            })
            .is_some(),
            "SidebarHeader search TextInput should render the 'Search…' placeholder"
        );
    }
}
