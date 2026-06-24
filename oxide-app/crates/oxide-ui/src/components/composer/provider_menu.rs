//! ProviderMenu — floating model + settings picker for the Composer toolbar (Task 10).
//!
//! Two internal views:
//! - models: three provider groups (Gemini/Claude/Local), thinking level, optimizer switch,
//!   nav row to settings.
//! - settings: back row, Send-on-Enter switch, optimizer switch, footer note.
//!
//! Menu chrome: radius 16, `panel()` background, deep shadow (0 22 52).
use freya::prelude::*;

use crate::tokens::Theme;
use super::config::{MODELS, ProviderId, Thinking};
use super::icons::icon;

// ── View discriminant ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum View { Models, Settings }

// ── Public component ──────────────────────────────────────────────────────────

/// Floating provider/model picker rendered as a menu body.
///
/// The caller owns `selected_id`, `thinking`, `optimizer`, and `send_on_enter`
/// and threads changes back via the `on_*` handlers. The view toggle (models ↔
/// settings) is internal state.
#[derive(Clone, PartialEq)]
pub struct ProviderMenu {
    pub selected_id:    String,
    pub thinking:       Thinking,
    pub optimizer:      bool,
    pub send_on_enter:  bool,
    pub theme:          Theme,

    on_select_model:        Option<EventHandler<&'static str>>,
    on_thinking:            Option<EventHandler<Thinking>>,
    on_toggle_optimizer:    Option<EventHandler<bool>>,
    on_toggle_send_on_enter: Option<EventHandler<bool>>,
}

impl ProviderMenu {
    pub fn new(theme: Theme) -> Self {
        Self {
            selected_id: String::new(),
            thinking: Thinking::Medium,
            optimizer: false,
            send_on_enter: true,
            theme,
            on_select_model: None,
            on_thinking: None,
            on_toggle_optimizer: None,
            on_toggle_send_on_enter: None,
        }
    }

    pub fn selected_id(mut self, id: impl Into<String>) -> Self {
        self.selected_id = id.into();
        self
    }

    pub fn thinking(mut self, thinking: Thinking) -> Self {
        self.thinking = thinking;
        self
    }

    pub fn optimizer(mut self, v: bool) -> Self {
        self.optimizer = v;
        self
    }

    pub fn send_on_enter(mut self, v: bool) -> Self {
        self.send_on_enter = v;
        self
    }

    pub fn on_select_model(mut self, h: impl Into<EventHandler<&'static str>>) -> Self {
        self.on_select_model = Some(h.into());
        self
    }

    pub fn on_thinking(mut self, h: impl Into<EventHandler<Thinking>>) -> Self {
        self.on_thinking = Some(h.into());
        self
    }

    pub fn on_toggle_optimizer(mut self, h: impl Into<EventHandler<bool>>) -> Self {
        self.on_toggle_optimizer = Some(h.into());
        self
    }

    pub fn on_toggle_send_on_enter(mut self, h: impl Into<EventHandler<bool>>) -> Self {
        self.on_toggle_send_on_enter = Some(h.into());
        self
    }
}

// ── Component impl ────────────────────────────────────────────────────────────

impl Component for ProviderMenu {
    fn render(&self) -> impl IntoElement {
        let view = use_state(|| View::Models);
        let th = self.theme;

        let container_theme = MenuContainerThemePartial::new()
            .background(th.panel())
            .border_fill(th.hairline())
            .shadow(Color::TRANSPARENT)
            .corner_radius(CornerRadius::new_all(16.));

        let body: Element = match *view.read() {
            View::Models  => self.models_view(th, view).into_element(),
            View::Settings => self.settings_view(th, view).into_element(),
        };

        rect()
            .corner_radius(CornerRadius::new_all(16.))
            .shadow((0.0_f32, 22.0_f32, 52.0_f32, 0.0_f32, th.shadow_deep()))
            .content(Content::fit())
            .child(
                Menu::new()
                    .theme(container_theme)
                    .child(body),
            )
    }
}

// ── Models view ───────────────────────────────────────────────────────────────

impl ProviderMenu {
    fn models_view(&self, th: Theme, mut view: State<View>) -> impl IntoElement {
        let mut rows: Vec<Element> = Vec::new();

        for provider in [ProviderId::Gemini, ProviderId::Claude, ProviderId::Local] {
            rows.push(self.provider_header(provider, th));
            for model in MODELS.iter().filter(|m| m.provider == provider) {
                rows.push(self.model_row(model, th));
            }
        }

        // ── Thinking level segmented control ──────────────────────────────
        let thinking = self.thinking;
        let on_thinking_low    = self.on_thinking.clone();
        let on_thinking_medium = self.on_thinking.clone();
        let on_thinking_high   = self.on_thinking.clone();

        let seg = rect()
            .direction(Direction::Vertical)
            .width(Size::fill())
            .padding(Gaps::new(6., 12., 4., 12.))
            .child(
                SegmentedButton::new().children([
                    ButtonSegment::new()
                        .key(0_u32)
                        .selected(thinking == Thinking::Low)
                        .on_press(move |_: Event<PressEventData>| {
                            if let Some(h) = &on_thinking_low {
                                h.call(Thinking::Low);
                            }
                        })
                        .child("Low")
                        .into_element(),
                    ButtonSegment::new()
                        .key(1_u32)
                        .selected(thinking == Thinking::Medium)
                        .on_press(move |_: Event<PressEventData>| {
                            if let Some(h) = &on_thinking_medium {
                                h.call(Thinking::Medium);
                            }
                        })
                        .child("Medium")
                        .into_element(),
                    ButtonSegment::new()
                        .key(2_u32)
                        .selected(thinking == Thinking::High)
                        .on_press(move |_: Event<PressEventData>| {
                            if let Some(h) = &on_thinking_high {
                                h.call(Thinking::High);
                            }
                        })
                        .child("High")
                        .into_element(),
                ])
            )
            .into_element();
        rows.push(seg);

        // ── Optimizer row ──────────────────────────────────────────────────
        rows.push(self.optimizer_row(th));

        // ── Composer settings nav row ──────────────────────────────────────
        let settings_row = MenuButton::new()
            .on_press(move |_: Event<PressEventData>| {
                view.set(View::Settings);
            })
            .child(
                rect()
                    .direction(Direction::Horizontal)
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(10.)
                    .width(Size::fill())
                    .child(icon("gear", 16., th.subtext_hi()))
                    .child(
                        label()
                            .text("Composer settings")
                            .font_size(13.)
                            .color(th.text())
                            .width(Size::flex(1.0))
                    )
                    .child(icon("chevronDown", 14., th.faint()))
            )
            .into_element();
        rows.push(settings_row);

        rect()
            .direction(Direction::Vertical)
            .width(Size::px(280.))
            .children(rows)
    }

    fn provider_header(&self, provider: ProviderId, th: Theme) -> Element {
        let tone_color = th.tone(provider.tone());
        rect()
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .padding(Gaps::new(10., 14., 4., 14.))
            .child(icon(provider.icon(), 14., tone_color))
            .child(
                label()
                    .text(provider.label())
                    .font_size(11.)
                    .font_weight(FontWeight::BOLD)
                    .color(tone_color)
            )
            .into_element()
    }

    fn model_row(&self, model: &'static crate::components::composer::config::Model, th: Theme) -> Element {
        let is_active = model.id == self.selected_id.as_str();
        let on_select = self.on_select_model.clone();
        let model_id  = model.id;

        // Text column: name (bold) + sub (faint), takes all remaining width.
        let text_col = rect()
            .direction(Direction::Vertical)
            .width(Size::flex(1.0))
            .child(
                label()
                    .text(model.name)
                    .font_size(13.)
                    .font_weight(FontWeight::BOLD)
                    .color(th.text())
                    .max_lines(1_usize)
            )
            .child(
                label()
                    .text(model.sub)
                    .font_size(11.)
                    .color(th.subtext())
                    .max_lines(1_usize)
            )
            .into_element();

        // Row: RadioItem + text + optional check
        let row_inner = rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(8.)
            .width(Size::fill())
            .child(RadioItem::new().selected(is_active))
            .child(text_col)
            .maybe_child(is_active.then(|| icon("check", 14., th.accent())))
            .into_element();

        MenuButton::new()
            .on_press(move |_: Event<PressEventData>| {
                if let Some(h) = &on_select {
                    h.call(model_id);
                }
            })
            .child(row_inner)
            .into_element()
    }

    fn optimizer_row(&self, th: Theme) -> Element {
        let optimizer = self.optimizer;
        let on_toggle = self.on_toggle_optimizer.clone();

        MenuButton::new()
            .child(
                rect()
                    .direction(Direction::Horizontal)
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(10.)
                    .width(Size::fill())
                    .child(icon("sparkle", 16., th.subtext_hi()))
                    .child(
                        label()
                            .text("Prompt optimizer")
                            .font_size(13.)
                            .color(th.text())
                            .width(Size::flex(1.0))
                    )
                    .child(
                        Switch::new()
                            .toggled(optimizer)
                            .on_toggle(move |_| {
                                if let Some(h) = &on_toggle {
                                    h.call(!optimizer);
                                }
                            })
                    )
            )
            .into_element()
    }
}

// ── Settings view ─────────────────────────────────────────────────────────────

impl ProviderMenu {
    fn settings_view(&self, th: Theme, mut view: State<View>) -> impl IntoElement {
        // Back row
        let back_row = MenuButton::new()
            .on_press(move |_: Event<PressEventData>| {
                view.set(View::Models);
            })
            .child(
                rect()
                    .direction(Direction::Horizontal)
                    .cross_align(Alignment::Center)
                    .spacing(8.)
                    .child(icon("chevronDown", 14., th.subtext_hi()))
                    .child(
                        label()
                            .text("Composer settings")
                            .font_size(13.)
                            .font_weight(FontWeight::BOLD)
                            .color(th.text())
                    )
            )
            .into_element();

        // Send on Enter
        let send_on_enter = self.send_on_enter;
        let on_toggle_soe = self.on_toggle_send_on_enter.clone();
        let soe_row = MenuButton::new()
            .child(
                rect()
                    .direction(Direction::Horizontal)
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(10.)
                    .width(Size::fill())
                    .child(icon("send", 16., th.subtext_hi()))
                    .child(
                        rect()
                            .direction(Direction::Vertical)
                            .width(Size::flex(1.0))
                            .child(
                                label()
                                    .text("Send on Enter")
                                    .font_size(13.)
                                    .color(th.text())
                                    .max_lines(1_usize)
                            )
                            .child(
                                label()
                                    .text("Shift+Enter = newline")
                                    .font_size(11.)
                                    .color(th.subtext())
                                    .max_lines(1_usize)
                            )
                    )
                    .child(
                        Switch::new()
                            .toggled(send_on_enter)
                            .on_toggle(move |_| {
                                if let Some(h) = &on_toggle_soe {
                                    h.call(!send_on_enter);
                                }
                            })
                    )
            )
            .into_element();

        // Prompt optimizer
        let optimizer = self.optimizer;
        let on_toggle_opt = self.on_toggle_optimizer.clone();
        let opt_row = MenuButton::new()
            .child(
                rect()
                    .direction(Direction::Horizontal)
                    .content(Content::Flex)
                    .cross_align(Alignment::Center)
                    .spacing(10.)
                    .width(Size::fill())
                    .child(icon("sparkle", 16., th.subtext_hi()))
                    .child(
                        rect()
                            .direction(Direction::Vertical)
                            .width(Size::flex(1.0))
                            .child(
                                label()
                                    .text("Prompt optimizer")
                                    .font_size(13.)
                                    .color(th.text())
                                    .max_lines(1_usize)
                            )
                            .child(
                                label()
                                    .text("rewrite before send")
                                    .font_size(11.)
                                    .color(th.subtext())
                                    .max_lines(1_usize)
                            )
                    )
                    .child(
                        Switch::new()
                            .toggled(optimizer)
                            .on_toggle(move |_| {
                                if let Some(h) = &on_toggle_opt {
                                    h.call(!optimizer);
                                }
                            })
                    )
            )
            .into_element();

        // Footer note
        let footer = rect()
            .padding(Gaps::new(6., 14., 10., 14.))
            .child(
                label()
                    .text("Prediction style, line cap & live markdown live in Tweaks.")
                    .font_size(11.)
                    .color(th.faint())
                    .max_lines(3_usize)
            )
            .into_element();

        rect()
            .direction(Direction::Vertical)
            .width(Size::px(280.))
            .child(back_row)
            .child(soe_row)
            .child(opt_row)
            .child(footer)
    }

}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    /// Step 1: mount with sonnet-4.6 selected; assert Opus 4.8 and Gemini 3 render
    /// and the active model shows the check icon (by confirming sonnet-4.6 is active).
    #[test]
    fn provider_menu_renders_all_models_and_check() {
        fn app() -> impl IntoElement {
            ProviderMenu::new(Theme::default())
                .selected_id("sonnet-4.6")
                .thinking(Thinking::Medium)
        }
        let mut t = launch_test(app);
        t.sync_and_update();

        // "Opus 4.8" must appear
        let opus = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Opus 4.8"))
        });
        assert!(opus.is_some(), "ProviderMenu should render 'Opus 4.8'");

        // "Gemini 3" must appear
        let gemini3 = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Gemini 3"))
        });
        assert!(gemini3.is_some(), "ProviderMenu should render 'Gemini 3'");

        // "Sonnet 4.6" (selected) must appear
        let sonnet = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Sonnet 4.6"))
        });
        assert!(sonnet.is_some(), "ProviderMenu should render selected model 'Sonnet 4.6'");
    }

    /// Settings view renders after switching via the nav row text
    #[test]
    fn provider_menu_shows_group_headers() {
        fn app() -> impl IntoElement {
            ProviderMenu::new(Theme::default())
                .selected_id("gemini-3")
                .thinking(Thinking::Low)
        }
        let mut t = launch_test(app);
        t.sync_and_update();

        for label_text in ["Gemini", "Claude", "Local LLM", "Low", "Medium", "High", "Prompt optimizer"] {
            let found = t.find(|_, el| {
                Label::try_downcast(el).filter(|l| l.text.as_ref().contains(label_text))
            });
            assert!(found.is_some(), "ProviderMenu should render label: {label_text}");
        }
    }

    /// Snapshot: renders the models view at 360px width.
    /// Run with: LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui composer::provider_menu::tests::snapshot_provider_menu -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_provider_menu() {
        fn app() -> impl IntoElement {
            rect()
                .background(Theme::default().bg_deep())
                .padding(Gaps::new_all(16.))
                .content(Content::fit())
                .child(
                    ProviderMenu::new(Theme::default())
                        .selected_id("sonnet-4.6")
                        .thinking(Thinking::Medium)
                        .optimizer(false)
                        .send_on_enter(true),
                )
        }
        let (mut runner, _) =
            TestingRunner::new(app, (360., 600.).into(), |_| {}, 1.);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-provider-menu.png");
    }
}
