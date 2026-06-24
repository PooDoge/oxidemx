//! AttachMenu — the floating source-picker that appears when the user clicks the
//! attach (+) button in the Composer toolbar (Task 9).
//!
//! This component renders ONLY the menu body. The trigger/anchor is supplied by
//! the Toolbar (Task 11) via `Attached`; do not build the anchor here.
//!
//! Builder usage:
//! ```ignore
//! fn app() -> impl IntoElement {
//!     let mut open = use_state(|| true);
//!     AttachMenu::new(Theme::default())
//!         .on_pick(move |id: &'static str| println!("picked: {id}"))
//! }
//! ```
use freya::prelude::*;

use crate::tokens::Theme;
use super::attachment::ATTACH_SOURCES;
use super::icons::icon;

// ── AttachMenu ────────────────────────────────────────────────────────────────

/// Floating menu body listing the six attachment sources.
///
/// Renders a `Menu` styled to the design spec: radius 14, `panel()` background,
/// deep drop shadow. Each row shows the source icon, label, and faint hint.
/// Fires `on_pick(source.id)` when the user selects a row.
#[derive(Clone, PartialEq)]
pub struct AttachMenu {
    theme:   Theme,
    on_pick: Option<EventHandler<&'static str>>,
}

impl AttachMenu {
    pub fn new(theme: Theme) -> Self {
        Self { theme, on_pick: None }
    }

    pub fn on_pick(mut self, handler: impl Into<EventHandler<&'static str>>) -> Self {
        self.on_pick = Some(handler.into());
        self
    }
}

impl Component for AttachMenu {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;

        // Build menu items for each attach source.
        // Each row: icon (16 px) | label (text) + hint (subtext) stacked.
        // The label column uses Size::flex so the row rect needs Content::Flex.
        let items: Vec<Element> = ATTACH_SOURCES.iter().map(|source| {
            let on_pick = self.on_pick.clone();
            let id = source.id;

            // Label + hint stacked vertically
            let text_col = rect()
                .direction(Direction::Vertical)
                .width(Size::flex(1.0))
                .child(
                    label()
                        .text(source.label)
                        .font_size(13.)
                        .color(th.text())
                        .max_lines(1_usize),
                )
                .child(
                    label()
                        .text(source.hint)
                        .font_size(11.)
                        .color(th.subtext())
                        .max_lines(1_usize),
                )
                .into_element();

            // Row: icon + text_col side by side
            let row = rect()
                .direction(Direction::Horizontal)
                // Content::Flex required — text_col has Size::flex(1.0)
                .content(Content::Flex)
                .cross_align(Alignment::Center)
                .spacing(10.)
                .width(Size::fill())
                .child(icon(source.icon, 16., th.subtext_hi()))
                .child(text_col)
                .into_element();

            MenuButton::new()
                .on_press(move |_: Event<PressEventData>| {
                    if let Some(h) = &on_pick {
                        h.call(id);
                    }
                })
                .child(row)
                .into_element()
        }).collect();

        // The MenuContainer's theme only controls the shadow *color*; the
        // offsets (x, y, blur, spread) are hardcoded to (0, 4, 10, 0) inside
        // freya's MenuContainer render.  To achieve the spec's "0 18 44" deep
        // shadow we wrap the Menu in a rect that carries the full shadow, and
        // suppress the container's own shadow by setting its color to
        // transparent.  The corner_radius of 14 IS exposed via the container
        // theme, so it is threaded through the partial.
        let container_theme = MenuContainerThemePartial::new()
            .background(th.panel())
            .border_fill(th.hairline())
            .shadow(Color::TRANSPARENT)
            .corner_radius(CornerRadius::new_all(14.));

        // Wrapper provides the deep drop-shadow (spec: 0 18 44 shadow_deep).
        // corner_radius matches the menu so the shadow clips correctly.
        rect()
            .corner_radius(CornerRadius::new_all(14.))
            .shadow((0.0_f32, 18.0_f32, 44.0_f32, 0.0_f32, th.shadow_deep()))
            .content(Content::fit())
            .child(
                Menu::new()
                    .theme(container_theme)
                    .children(items),
            )
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    /// Step 1 (brief): mount AttachMenu, assert "Upload file" renders.
    #[test]
    fn attach_menu_shows_upload_file_label() {
        fn app() -> impl IntoElement {
            AttachMenu::new(Theme::default())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Upload file"))
        });
        assert!(found.is_some(), "AttachMenu should render the 'Upload file' label");
    }

    /// All six source labels must appear.
    #[test]
    fn attach_menu_renders_all_six_sources() {
        fn app() -> impl IntoElement {
            AttachMenu::new(Theme::default())
        }
        let mut t = launch_test(app);
        t.sync_and_update();

        for source in ATTACH_SOURCES.iter() {
            let label_text = source.label;
            let found = t.find(|_, el| {
                Label::try_downcast(el).filter(|l| l.text.as_ref().contains(label_text))
            });
            assert!(found.is_some(), "Missing label: {label_text}");
        }
    }
}
