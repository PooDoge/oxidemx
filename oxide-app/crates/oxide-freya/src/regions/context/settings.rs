//! .oxide settings tab — resolved config nav. Slice 1: placeholder category
//! rows with inheritance-source chips (real .oxide resolution + Hooks/MCP
//! editors land in the settings follow-on slice). Rows inert this slice.
use freya::prelude::*;
use oxide_ui::Theme;

use crate::state::AppState;
use super::run::section_label;

#[allow(dead_code)]
#[derive(PartialEq, Clone)]
pub struct SettingsTab {
    pub state: AppState,
}

/// (label, count text, source) — source: "inherited" | "local" | "merged".
const NAV: [(&str, &str, &str); 4] = [
    ("Hooks", "3 hooks", "local"),
    ("MCP servers", "2 servers", "inherited"),
    ("Permissions", "allow / ask / deny", "merged"),
    ("Environment", "4 vars", "inherited"),
];

fn source_color(th: Theme, source: &str) -> Color {
    match source {
        "local" => th.green(),
        "merged" => th.mauve(),
        _ => th.blue(),
    }
}

impl Component for SettingsTab {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let mut col = rect()
            .direction(Direction::Vertical)
            .spacing(6.)
            .padding(Gaps::new_all(14.))
            .child(section_label(th, "Resolved .oxide"));
        for (lbl, count, source) in NAV {
            let c = source_color(th, source);
            col = col.child(
                rect()
                    .direction(Direction::Horizontal)
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(11.)
                    .width(Size::fill())
                    .padding(Gaps::new(10., 11., 10., 11.))
                    .corner_radius(CornerRadius::new_all(9.))
                    .background(th.surface())
                    .border(Border::new().fill(th.hairline()).width(1.))
                    .child(
                        rect()
                            .direction(Direction::Vertical)
                            .width(Size::flex(1.0))
                            .child(label().text(lbl).font_size(13.).color(th.text()))
                            .child(label().text(count).font_size(10.5).color(th.subtext())),
                    )
                    .child(
                        rect()
                            .padding(Gaps::new(1., 6., 1., 6.))
                            .corner_radius(CornerRadius::new_all(999.))
                            .background(Theme::with_alpha(c, 0x16))
                            .border(Border::new().fill(Theme::with_alpha(c, 0x3a)).width(1.))
                            .child(label().text(source).font_size(9.).color(c)),
                    ),
            );
        }
        col
    }
}
