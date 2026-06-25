//! Composer Toolbar — bottom control row (Task 11).
//!
//! Contains:
//! - Pure `send_state` logic + `SendState` enum (fully testable without a runtime).
//! - `Toolbar` component: a `Content::Flex` horizontal row with:
//!   AttachButton · ProviderPill · OptimizerChip · [spacer] · LineHint · SendButton.
//!
//! The `Content::Flex` on the row is mandatory — the spacer before LineHint uses
//! `Size::flex(1.0)` to push the send button to the right edge; omitting it would
//! cause the send button to overflow off-screen.
use freya::animation::*;
use freya::prelude::*;

use crate::tokens::Theme;
use crate::components::menu::{Placement, Popover};
use super::config::{Thinking, model_by_id};
use super::icons::icon;

// ── SendState ─────────────────────────────────────────────────────────────────

/// Represents the three send-button states the toolbar needs to distinguish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendState {
    /// No content — button is inert.
    Disabled,
    /// Content present — ready to send.
    Ready,
    /// A send is in flight — shows a stop affordance.
    Working,
}

/// Pure function: derive the send state from the composer's inputs.
///
/// Decision table:
/// - `working` ⇒ `Working` (highest priority).
/// - `text_empty && !has_attachments` ⇒ `Disabled`.
/// - otherwise ⇒ `Ready` (text present *or* at least one attachment).
pub fn send_state(text_empty: bool, has_attachments: bool, working: bool) -> SendState {
    if working {
        return SendState::Working;
    }
    if text_empty && !has_attachments {
        return SendState::Disabled;
    }
    SendState::Ready
}

// ── Toolbar ───────────────────────────────────────────────────────────────────

/// Bottom toolbar row for the Composer.
///
/// Builder usage:
/// ```ignore
/// Toolbar::new(theme)
///     .model_id("sonnet-4.6")
///     .thinking(Thinking::Medium)
///     .optimizer_on(false)
///     .line_count(3)
///     .send(SendState::Ready)
///     .attach_open(false)
///     .on_attach_toggle(|_| println!("attach toggled"))
///     .on_provider_toggle(|_| println!("provider toggled"))
///     .on_send(|_| println!("send!"))
/// ```
#[derive(Clone, PartialEq)]
pub struct Toolbar {
    pub model_id:      String,
    pub thinking:      Thinking,
    pub optimizer_on:  bool,
    pub line_count:    usize,
    pub send:          SendState,
    pub attach_open:   bool,
    pub provider_open: bool,
    pub theme:         Theme,

    // Menu content (built by the orchestrator) + per-trigger dismiss handlers.
    // The Toolbar wraps each trigger in a `Popover` so each menu anchors to ITS
    // own button and dismisses on outside-press/Escape.
    attach_menu:   Option<Element>,
    provider_menu: Option<Element>,

    on_attach_toggle:   Option<EventHandler<()>>,
    on_provider_toggle: Option<EventHandler<()>>,
    on_attach_dismiss:   Option<EventHandler<()>>,
    on_provider_dismiss: Option<EventHandler<()>>,
    on_send:            Option<EventHandler<()>>,
}

impl Toolbar {
    pub fn new(theme: Theme) -> Self {
        Self {
            model_id:           String::new(),
            thinking:           Thinking::Medium,
            optimizer_on:       false,
            line_count:         1,
            send:               SendState::Disabled,
            attach_open:        false,
            provider_open:      false,
            theme,
            attach_menu:        None,
            provider_menu:      None,
            on_attach_toggle:   None,
            on_provider_toggle: None,
            on_attach_dismiss:   None,
            on_provider_dismiss: None,
            on_send:            None,
        }
    }

    pub fn model_id(mut self, id: impl Into<String>) -> Self {
        self.model_id = id.into();
        self
    }

    pub fn thinking(mut self, t: Thinking) -> Self {
        self.thinking = t;
        self
    }

    pub fn optimizer_on(mut self, v: bool) -> Self {
        self.optimizer_on = v;
        self
    }

    pub fn line_count(mut self, n: usize) -> Self {
        self.line_count = n;
        self
    }

    pub fn send(mut self, s: SendState) -> Self {
        self.send = s;
        self
    }

    pub fn attach_open(mut self, v: bool) -> Self {
        self.attach_open = v;
        self
    }

    pub fn provider_open(mut self, v: bool) -> Self {
        self.provider_open = v;
        self
    }

    pub fn attach_menu(mut self, menu: Option<Element>) -> Self {
        self.attach_menu = menu;
        self
    }

    pub fn provider_menu(mut self, menu: Option<Element>) -> Self {
        self.provider_menu = menu;
        self
    }

    pub fn on_attach_dismiss(mut self, h: impl Into<EventHandler<()>>) -> Self {
        self.on_attach_dismiss = Some(h.into());
        self
    }

    pub fn on_provider_dismiss(mut self, h: impl Into<EventHandler<()>>) -> Self {
        self.on_provider_dismiss = Some(h.into());
        self
    }

    pub fn on_attach_toggle(mut self, h: impl Into<EventHandler<()>>) -> Self {
        self.on_attach_toggle = Some(h.into());
        self
    }

    pub fn on_provider_toggle(mut self, h: impl Into<EventHandler<()>>) -> Self {
        self.on_provider_toggle = Some(h.into());
        self
    }

    pub fn on_send(mut self, h: impl Into<EventHandler<()>>) -> Self {
        self.on_send = Some(h.into());
        self
    }
}

// ── Component impl ────────────────────────────────────────────────────────────

impl Component for Toolbar {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let attach_open = self.attach_open;

        // Animated rotation: plus glyph rotates 0° (closed) → 45° (open) so it
        // becomes an × shape.  `OnChange::Rerun` re-evaluates the factory whenever
        // `attach_open` flips; `into_reversed()` runs the tween backwards for close.
        let attach_anim = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            conf.on_creation(OnCreation::Finish);
            let tween = AnimNum::new(0.0_f32, 45.0_f32)
                .time(160)
                .ease(Ease::Out)
                .function(Function::Quart);
            if attach_open {
                tween
            } else {
                tween.into_reversed()
            }
        });
        let plus_rotation = attach_anim.get().value();

        // ── AttachButton ─────────────────────────────────────────────────────
        let on_attach = self.on_attach_toggle.clone();
        let attach_btn: Element = {
            let inner = rect()
                .width(Size::px(36.))
                .height(Size::px(36.))
                .main_align(Alignment::Center)
                .cross_align(Alignment::Center)
                .corner_radius(CornerRadius::new_all(10.))
                .background(th.surface())
                .border(Border::new().fill(th.hairline()).width(1.))
                .rotate(plus_rotation)
                .child(icon("plus", 18., th.subtext_hi()));
            if let Some(h) = on_attach {
                inner
                    .on_press(move |_: Event<PressEventData>| h.call(()))
                    .into_element()
            } else {
                inner.into_element()
            }
        };

        // Wrap the attach button in a Popover so the attach menu anchors to THIS
        // button and dismisses on outside-press/Escape. The Popover renders the
        // anchor (the button) always and shows `content` adjacent only when open.
        let on_attach_dismiss = self.on_attach_dismiss.clone();
        let attach_block: Element = {
            let mut pop = Popover::new(attach_btn)
                .open(self.attach_open)
                .placement(Placement::Above)
                .content(
                    self.attach_menu
                        .clone()
                        .unwrap_or_else(|| rect().into_element()),
                );
            if let Some(h) = on_attach_dismiss {
                pop = pop.on_dismiss(h);
            }
            pop.into_element()
        };

        // ── ProviderPill ─────────────────────────────────────────────────────
        // Resolve model from the registry (fall back to empty strings if unknown).
        let model_name = model_by_id(&self.model_id)
            .map(|m| m.name)
            .unwrap_or("Model");
        let provider_icon = model_by_id(&self.model_id)
            .map(|m| m.provider.icon())
            .unwrap_or("sparkle");
        let badge_text = self.thinking.badge().to_string();
        let on_provider = self.on_provider_toggle.clone();

        // Thinking badge: small accent-tinted box with the L/M/H letter.
        let badge: Element = rect()
            .main_align(Alignment::Center)
            .cross_align(Alignment::Center)
            .padding(Gaps::new(2., 5., 2., 5.))
            .corner_radius(CornerRadius::new_all(5.))
            .background(Theme::with_alpha(th.accent(), 0x22))
            .child(
                label()
                    .text(badge_text)
                    .font_size(10.)
                    .font_weight(FontWeight::BOLD)
                    .color(th.accent())
            )
            .into_element();

        let pill_inner = rect()
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .spacing(5.)
            .padding(Gaps::new(5., 10., 5., 8.))
            .corner_radius(CornerRadius::new_all(10.))
            .background(th.surface())
            .border(Border::new().fill(th.hairline()).width(1.))
            .child(icon(provider_icon, 14., th.subtext_hi()))
            .child(
                label()
                    .text(model_name)
                    .font_size(12.5)
                    .color(th.text())
                    .max_lines(1_usize)
            )
            .child(badge)
            .child(icon("chevronDown", 13., th.faint()));

        let provider_pill: Element = if let Some(h) = on_provider {
            pill_inner
                .on_press(move |_: Event<PressEventData>| h.call(()))
                .into_element()
        } else {
            pill_inner.into_element()
        };

        // Wrap the provider pill in a Popover so the provider menu anchors to THIS
        // pill and dismisses on outside-press/Escape.
        let on_provider_dismiss = self.on_provider_dismiss.clone();
        let provider_block: Element = {
            let mut pop = Popover::new(provider_pill)
                .open(self.provider_open)
                .placement(Placement::Above)
                .content(
                    self.provider_menu
                        .clone()
                        .unwrap_or_else(|| rect().into_element()),
                );
            if let Some(h) = on_provider_dismiss {
                pop = pop.on_dismiss(h);
            }
            pop.into_element()
        };

        // ── OptimizerChip (only when optimizer_on) ────────────────────────────
        let optimizer_chip: Option<Element> = self.optimizer_on.then(|| {
            rect()
                .direction(Direction::Horizontal)
                .cross_align(Alignment::Center)
                .spacing(4.)
                .padding(Gaps::new(4., 9., 4., 7.))
                .corner_radius(CornerRadius::new_all(10.))
                .background(Theme::with_alpha(th.mauve(), 0x16))
                .border(Border::new().fill(Theme::with_alpha(th.mauve(), 0x33)).width(1.))
                .child(icon("sparkle", 12., th.mauve()))
                .child(
                    label()
                        .text("Optimizer")
                        .font_size(11.)
                        .color(th.mauve())
                )
                .into_element()
        });

        // ── Spacer + LineHint (only when line_count > 1) ──────────────────────
        // The spacer MUST be Size::flex(1.0) — this is what pushes the send button
        // to the right.  The parent row MUST have Content::Flex for this to work.
        let spacer: Element = rect()
            .width(Size::flex(1.0))
            .height(Size::px(1.))
            .into_element();

        let line_hint: Option<Element> = (self.line_count > 1).then(|| {
            let text = format!("{} lines · ⇧⏎ newline", self.line_count);
            label()
                .text(text)
                .font_size(11.)
                .color(th.faint())
                .font_family(crate::tokens::FONT_MONO)
                .max_lines(1_usize)
                .into_element()
        });

        // ── SendButton ────────────────────────────────────────────────────────
        let on_send = self.on_send.clone();
        let send_state = self.send;

        let (send_bg, send_glyph_color, send_icon_name, send_shadow) = match send_state {
            SendState::Disabled => (
                th.surface_hi(),
                th.faint(),
                "send",
                None,
            ),
            SendState::Ready => (
                th.accent(),
                th.bg_deep(),
                "send",
                Some((0.0_f32, 4.0_f32, 12.0_f32, 0.0_f32, Theme::with_alpha(th.accent(), 0x50))),
            ),
            SendState::Working => (
                th.surface_max(),
                th.red(),
                "stop",
                None,
            ),
        };

        let send_inner = rect()
            .width(Size::px(42.))
            .height(Size::px(42.))
            .main_align(Alignment::Center)
            .cross_align(Alignment::Center)
            .corner_radius(CornerRadius::new_all(12.))
            .background(send_bg)
            .child(icon(send_icon_name, 20., send_glyph_color));

        // Apply shadow only for Ready state.
        let send_inner = if let Some(sh) = send_shadow {
            send_inner.shadow(sh)
        } else {
            send_inner
        };

        // Only wire on_send when the button is not Disabled.
        let send_btn: Element = if send_state != SendState::Disabled {
            if let Some(h) = on_send {
                send_inner
                    .on_press(move |_: Event<PressEventData>| h.call(()))
                    .into_element()
            } else {
                send_inner.into_element()
            }
        } else {
            send_inner.into_element()
        };

        // ── Row assembly ──────────────────────────────────────────────────────
        // Content::Flex is REQUIRED here — the spacer child uses Size::flex(1.0).
        // Without it the spacer collapses to zero and the send button is not pushed
        // to the right (or overflows off-screen).
        let mut row = rect()
            .direction(Direction::Horizontal)
            // ↓ THE critical line — Content::Flex must be present.
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .width(Size::fill())
            .padding(Gaps::new(6., 12., 6., 10.))
            .child(attach_block)
            .child(provider_block);

        if let Some(chip) = optimizer_chip {
            row = row.child(chip);
        }

        // Spacer always present — pushes right side to the edge.
        row = row.child(spacer);

        if let Some(hint) = line_hint {
            row = row.child(hint);
        }

        row.child(send_btn)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_state_resolves() {
        assert_eq!(send_state(true, false, false),  SendState::Disabled);
        assert_eq!(send_state(true, true, false),   SendState::Ready);   // attachment alone enables
        assert_eq!(send_state(false, false, false), SendState::Ready);
        assert_eq!(send_state(false, false, true),  SendState::Working);
    }
}
