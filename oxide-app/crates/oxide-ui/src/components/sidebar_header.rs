//! Sidebar header: project switcher (Freya `Select`) + search input + new-conversation icon button.
use freya::prelude::*;
use freya_icons::lucide;

use crate::components::TextInput;
use crate::tokens::Theme;

/// Top-of-sidebar header: project switcher (Freya `Select`) + search + "+" new conversation
/// icon button (34×34, accent-filled) inline on the same row as the search input.
///
/// Builder usage:
/// ```ignore
/// SidebarHeader::new(
///     vec![("personal".into(), "oxidemx-phase1".into())],
///     "personal".into(),
/// )
/// .on_select(|id| state.open_project(id.into()))
/// .on_new(|| state.new_conversation())
/// .theme(theme)
/// ```
#[derive(PartialEq, Clone)]
pub struct SidebarHeader {
    /// (project_id, project_name) options.
    projects: Vec<(String, String)>,
    /// The currently-selected project id (used to resolve the displayed name).
    current_id: String,
    on_select: Option<EventHandler<String>>,
    on_new: Option<EventHandler<()>>,
    theme: Theme,
}

impl SidebarHeader {
    pub fn new(projects: Vec<(String, String)>, current_id: String) -> Self {
        Self { projects, current_id, on_select: None, on_new: None, theme: Theme::default() }
    }

    pub fn on_select(mut self, h: impl Into<EventHandler<String>>) -> Self {
        self.on_select = Some(h.into());
        self
    }

    pub fn on_new(mut self, h: impl Into<EventHandler<()>>) -> Self {
        self.on_new = Some(h.into());
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
            );

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

        let switcher = Select::new()
            .theme(SelectThemePartial {
                width: Some(Preference::Specific(Size::fill())),
                margin: None,
                select_background: Some(Preference::Specific(th.surface())),
                background_button: Some(Preference::Specific(th.surface())),
                hover_background: Some(Preference::Specific(th.surface_hi())),
                border_fill: Some(Preference::Specific(th.hairline_strong())),
                focus_border_fill: Some(Preference::Specific(th.accent())),
                arrow_fill: Some(Preference::Specific(th.faint())),
                color: Some(Preference::Specific(th.text())),
            })
            .selected_item(pill)
            .children(options);

        // Search input — local state, non-functional search (wiring is future).
        // Wrapped in a flex rect so TextInput fills available width without a .width() builder.
        let search_wrap = rect()
            .width(Size::flex(1.0))
            .child(
                TextInput::new(value.into_writable(), th)
                    .placeholder("Search…"),
            );

        // "+" icon button — 34×34, accent bg, inline at the right of the search row.
        let on_new = self.on_new.clone();
        let plus = rect()
            .width(Size::px(34.))
            .height(Size::px(34.))
            .corner_radius(CornerRadius::new_all(9.))
            .background(th.accent())
            .center()
            .a11y_role(AccessibilityRole::Button)
            .a11y_alt("New conversation")
            .shadow((0.0_f32, 4.0_f32, 12.0_f32, 0.0_f32, Theme::with_alpha(th.accent(), 0x40)))
            .on_press(move |_: Event<PressEventData>| {
                if let Some(h) = &on_new { h.call(()); }
            })
            .child(
                svg(lucide::plus())
                    .color(th.bg_deep())
                    .width(Size::px(17.))
                    .height(Size::px(17.)),
            );

        // Search + "+" inline row.
        let search_row = rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.)
            .width(Size::fill())
            .child(search_wrap)
            .child(plus);

        rect()
            .direction(Direction::Vertical)
            .spacing(8.)
            .padding(Gaps::new_all(8.))
            .width(Size::fill())
            .child(switcher)
            .child(search_row)
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
            t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Search")))
                .is_some(),
            "search TextInput should render the 'Search…' placeholder"
        );
        assert!(
            t.find(|_, el| Svg::try_downcast(el).map(|_| ())).is_some(),
            "SidebarHeader should render the '+' new-conversation svg icon button"
        );
    }

    /// Snapshot: renders the header with dark theme; inline "+" icon button is visible
    /// on the same row as the search input.
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
