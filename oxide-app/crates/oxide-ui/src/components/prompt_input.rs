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
        let mut input = Input::new(self.value.clone())
            .width(Size::fill())
            .placeholder("Ask anything…");

        if let Some(handler) = self.on_submit.clone() {
            input = input.on_submit(handler);
        }

        rect()
            .width(Size::fill())
            .padding(Gaps::new_all(8.))
            .background(self.theme.surface())
            .child(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn prompt_input_renders() {
        fn app() -> impl IntoElement {
            let value = use_state(String::new);
            PromptInput::new(value.into_writable())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        // The built-in Input renders a rect; assert some rect exists.
        let found = t.find(|_, el| Rect::try_downcast(el));
        assert!(found.is_some(), "prompt input should render");
    }
}
