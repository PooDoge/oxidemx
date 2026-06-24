//! ActivityLine — the status line above the Composer card (Task 12).
//!
//! Renders a single horizontal row at ~11 px showing:
//!   [provider icon]  "{model.name} · {thinking_word} reasoning · {optimizer_status}"
//!   [flex spacer]  "/ for commands"
//!
//! The Composer only mounts this component when `config.activity == true`; there is
//! no visibility gate here.
use freya::prelude::*;

use crate::tokens::{Theme, FONT_MONO};
use super::config::{Model, Thinking};
use super::icons::icon;

// ── ActivityLine ───────────────────────────────────────────────────────────────

/// Status line displayed above the Composer card.
///
/// Builder usage:
/// ```ignore
/// ActivityLine::new(model, Thinking::Medium, false, theme)
/// ```
#[derive(Clone, PartialEq)]
pub struct ActivityLine {
    pub model:        Model,
    pub thinking:     Thinking,
    pub optimizer_on: bool,
    pub theme:        Theme,
}

impl ActivityLine {
    pub fn new(model: Model, thinking: Thinking, optimizer_on: bool, theme: Theme) -> Self {
        Self { model, thinking, optimizer_on, theme }
    }
}

// ── Component impl ─────────────────────────────────────────────────────────────

impl Component for ActivityLine {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let model = self.model;
        let tone_color = th.tone(model.tone);

        // ── Left label text ───────────────────────────────────────────────────
        let optimizer_status = if self.optimizer_on {
            "optimizer on"
        } else {
            "3 tools armed"
        };
        let thinking_word = self.thinking.word();
        let info_text = format!(
            "{} · {} reasoning · {}",
            model.name, thinking_word, optimizer_status
        );

        // ── Row: [icon] [info label] [spacer] [commands hint] ─────────────────
        // Content::Flex is required because the spacer uses Size::flex(1.0).
        rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(5.)
            .width(Size::fill())
            .child(icon(model.provider.icon(), 12., tone_color))
            .child(
                label()
                    .text(info_text)
                    .font_size(11.)
                    .color(th.subtext())
                    .max_lines(1_usize)
            )
            // Spacer — pushes "/ for commands" to the right edge.
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .height(Size::px(1.))
                    .into_element()
            )
            .child(
                label()
                    .text("/ for commands")
                    .font_size(10.5)
                    .color(th.faint())
                    .font_family(FONT_MONO)
                    .max_lines(1_usize)
            )
    }
}

// ── tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::composer::config::{model_by_id, DEFAULT_MODEL_ID};
    use freya_testing::prelude::*;

    #[test]
    fn activity_line_renders_model_name_and_commands_hint() {
        let model = *model_by_id(DEFAULT_MODEL_ID).expect("default model exists");

        fn app() -> impl IntoElement {
            let model = *model_by_id(DEFAULT_MODEL_ID).expect("default model exists");
            ActivityLine::new(model, Thinking::Medium, false, Theme::default())
        }

        let mut t = launch_test(app);
        t.sync_and_update();

        let found_model = t.find(|_, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains(model.name))
        });
        assert!(found_model.is_some(), "ActivityLine should render the model name");

        let found_hint = t.find(|_, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("/ for commands"))
        });
        assert!(found_hint.is_some(), "ActivityLine should render '/ for commands'");
    }
}
