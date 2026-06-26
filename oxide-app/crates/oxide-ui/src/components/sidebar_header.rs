//! Sidebar header: project switcher (Freya `Select`) + search input + new-conversation button.
use freya::prelude::*;

use crate::components::TextInput;
use crate::tokens::Theme;

/// Top-of-sidebar header: project switcher (Freya `Select`) + search + new conversation button.
///
/// Builder usage:
/// ```ignore
/// SidebarHeader::new(
///     vec![("personal".into(), "oxidemx-phase1".into())],
///     "personal".into(),
/// )
/// .on_select(|id| state.open_project(id.into()))
/// .theme(theme)
/// ```
#[derive(PartialEq, Clone)]
pub struct SidebarHeader {
    /// (project_id, project_name) options.
    projects: Vec<(String, String)>,
    /// The currently-selected project id (used to resolve the displayed name).
    current_id: String,
    on_select: Option<EventHandler<String>>,
    theme: Theme,
}

impl SidebarHeader {
    pub fn new(projects: Vec<(String, String)>, current_id: String) -> Self {
        Self { projects, current_id, on_select: None, theme: Theme::default() }
    }

    pub fn on_select(mut self, h: impl Into<EventHandler<String>>) -> Self {
        self.on_select = Some(h.into());
        self
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

        // Resolve the displayed name from current_id (fallback: first project, else id).
        let current_name = self
            .projects
            .iter()
            .find(|(id, _)| id == &self.current_id)
            .map(|(_, name)| name.clone())
            .or_else(|| self.projects.first().map(|(_, n)| n.clone()))
            .unwrap_or_else(|| self.current_id.clone());

        let pill = rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.)
            .width(Size::fill())
            .padding(Gaps::new(8., 10., 8., 10.))
            .child(
                rect().width(Size::px(6.)).height(Size::px(6.))
                    .corner_radius(CornerRadius::new_all(3.)).background(th.accent()),
            )
            .child(
                label().text(current_name).font_size(12.5).color(th.text())
                    .width(Size::flex(1.0)),
            )
            .child(label().text("⌄").font_size(12.).color(th.faint()));

        let on_select = self.on_select.clone();
        let current_id = self.current_id.clone();
        let options: Vec<Element> = self
            .projects
            .iter()
            .map(|(id, name)| {
                let id = id.clone();
                let selected = id == current_id;
                let h = on_select.clone();
                MenuItem::new()
                    .selected(selected)
                    .on_press(move |_: Event<PressEventData>| {
                        if let Some(handler) = &h {
                            handler.call(id.clone());
                        }
                    })
                    .child(label().text(name.clone()).font_size(12.5))
                    .into()
            })
            .collect();

        let switcher = Select::new().selected_item(pill).children(options);

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
            SidebarHeader::new(
                vec![("personal".into(), "oxidemx-phase1".into())],
                "personal".into(),
            )
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        assert!(
            t.find(|_, el| Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("oxidemx-phase1")))
                .is_some(),
            "SidebarHeader should render the current project name"
        );
        assert!(
            t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("New")))
                .is_some(),
            "SidebarHeader should render the '+ New' button label"
        );
        assert!(
            t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Search")))
                .is_some(),
            "search TextInput should render the 'Search…' placeholder"
        );
    }

    /// Snapshot: renders the header with dark theme.
    /// Run with: LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui snapshot_switcher_dark -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_switcher_dark() {
        fn app() -> impl IntoElement {
            use_init_theme(dark_theme);
            rect().background(Theme::default().bg_deep()).padding(Gaps::new_all(16.)).child(
                SidebarHeader::new(
                    vec![
                        ("personal".into(), "oxidemx-phase1".into()),
                        ("work".into(), "client-app".into()),
                    ],
                    "personal".into(),
                ),
            )
        }
        let (mut runner, _) =
            TestingRunner::new(app, (300., 240.).into(), |_| {}, 1.);
        runner.sync_and_update();
        runner.render_to_file("/tmp/shell-switcher-closed.png");
    }
}
