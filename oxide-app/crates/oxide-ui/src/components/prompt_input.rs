//! A text prompt box that wraps the built-in `Input` component.
//!
//! Call `.on_submit(|text: String| { ... })` to handle the user pressing Enter.
//! The `value` prop is a `Writable<String>` so it accepts either `State<String>`
//! (via `state.into_writable()`) or a radio slice.
use freya::prelude::*;

use crate::tokens::Theme;

/// A prompt text-entry box.
///
/// Builder usage:
/// ```ignore
/// fn app() -> impl IntoElement {
///     let value = use_state(String::new);
///     PromptInput::new(value.into_writable())
///         .on_submit(|text| println!("submitted: {text}"))
/// }
/// ```
///
/// Note: `Writable<String>` is constructed from a `State<String>` by calling
/// `state.into_writable()` (from `freya::prelude::IntoWritable`).
#[derive(PartialEq, Clone)]
pub struct PromptInput {
    value: Writable<String>,
    on_submit: Option<EventHandler<String>>,
    theme: Theme,
}

impl PromptInput {
    pub fn new(value: Writable<String>) -> Self {
        Self { value, on_submit: None, theme: Theme::default() }
    }

    pub fn on_submit(mut self, handler: impl Into<EventHandler<String>>) -> Self {
        self.on_submit = Some(handler.into());
        self
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }
}

impl Component for PromptInput {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let mut input = Input::new(self.value.clone())
            .width(Size::fill())
            .placeholder("Ask, or type / for a flow…")
            .theme_colors(InputColorsThemePartial {
                background: Some(Preference::Specific(th.bg_deep())),
                focus_background: Some(Preference::Specific(th.bg_deep())),
                border_fill: Some(Preference::Specific(th.surface_max())),
                focus_border_fill: Some(Preference::Specific(th.accent())),
                color: Some(Preference::Specific(th.text())),
                placeholder_color: Some(Preference::Specific(th.faint())),
            });

        if let Some(handler) = self.on_submit.clone() {
            input = input.on_submit(handler);
        }

        rect()
            .direction(Direction::Horizontal)
            // Content::Flex is REQUIRED for the input box's Size::flex(1.0) to be
            // honored — without it the box takes the full row width and the fixed
            // 42px send button overflows off the right edge. (FREYA-PATTERNS.md)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(9.)
            .width(Size::fill())
            .padding(Gaps::new(10., 16., 14., 16.))
            .background(th.bg())
            .border(
                Border::new()
                    .fill(th.hairline())
                    .width(BorderWidth { top: 1., ..Default::default() }),
            )
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .corner_radius(CornerRadius::new_all(12.))
                    .background(th.bg_deep())
                    .border(Border::new().fill(th.surface_max()).width(1.))
                    .padding(Gaps::new_all(4.))
                    .child(input),
            )
            .child(
                rect()
                    .width(Size::px(42.))
                    .height(Size::px(42.))
                    .corner_radius(CornerRadius::new_all(12.))
                    .main_align(Alignment::Center)
                    .cross_align(Alignment::Center)
                    .background(th.accent())
                    .child(
                        label().text("➤").font_size(17.).color(th.bg_deep()),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn composer_shows_send_glyph() {
        fn app() -> impl IntoElement {
            let value = use_state(String::new);
            PromptInput::new(value.into_writable())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        // The send glyph is a stable, composer-specific marker.
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "➤")
        });
        assert!(found.is_some(), "composer must render the ➤ send glyph");
    }
}
