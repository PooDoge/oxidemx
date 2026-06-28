//! `ConfirmDialog` — a reusable modal dialog for destructive confirmation.
//!
//! Renders as a full-window dimmed backdrop (`Layer::Overlay`) with a centered
//! card containing a title, body, and a Cancel / Confirm button row.
//!
//! - Clicking the backdrop calls `on_cancel`.
//! - Pressing Escape calls `on_cancel`.
//! - The Confirm button is danger-tinted when `danger(true)`.
//!
//! No animation: plain mount/unmount. The caller controls visibility by only
//! rendering this component when a confirmation is needed.
//!
//! Builder usage:
//! ```ignore
//! ConfirmDialog::new(Theme::default())
//!     .title("Delete \"My chat\"?".to_string())
//!     .body("This can't be undone.".to_string())
//!     .confirm_label("Delete")
//!     .danger(true)
//!     .on_confirm((|()| {}).into())
//!     .on_cancel((|()| {}).into())
//! ```
use freya::prelude::*;

use crate::tokens::Theme;

#[derive(Clone, PartialEq)]
pub struct ConfirmDialog {
    theme:         Theme,
    title:         String,
    body:          String,
    confirm_label: String,
    danger:        bool,
    on_confirm:    Option<EventHandler<()>>,
    on_cancel:     Option<EventHandler<()>>,
}

impl ConfirmDialog {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            title:         String::new(),
            body:          String::new(),
            confirm_label: "Confirm".to_string(),
            danger:        false,
            on_confirm:    None,
            on_cancel:     None,
        }
    }

    pub fn title(mut self, title: String) -> Self {
        self.title = title;
        self
    }

    pub fn body(mut self, body: String) -> Self {
        self.body = body;
        self
    }

    pub fn confirm_label(mut self, label: impl Into<String>) -> Self {
        self.confirm_label = label.into();
        self
    }

    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    pub fn on_confirm(mut self, h: EventHandler<()>) -> Self {
        self.on_confirm = Some(h);
        self
    }

    pub fn on_cancel(mut self, h: EventHandler<()>) -> Self {
        self.on_cancel = Some(h);
        self
    }
}

impl Component for ConfirmDialog {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;

        // No stateful hooks needed — plain structural render.
        // (Freya rule: any hooks must be unconditional at the top of render;
        //  we have none here so nothing to declare.)

        let on_confirm = self.on_confirm.clone();
        let on_cancel  = self.on_cancel.clone();
        let on_cancel2 = self.on_cancel.clone();

        let confirm_bg = if self.danger { th.red() } else { th.accent() };
        let confirm_fg = th.text();

        // ── Button row ────────────────────────────────────────────────────────
        let cancel_btn: Element = {
            let h = on_cancel.clone();
            rect()
                .padding(Gaps::new(7., 18., 7., 18.))
                .corner_radius(CornerRadius::new_all(8.))
                .background(th.surface_hi())
                .on_press(move |_: Event<PressEventData>| {
                    if let Some(ref cb) = h { cb.call(()); }
                })
                .child(label().font_size(13.).color(th.text()).text("Cancel"))
                .into_element()
        };

        let confirm_label = self.confirm_label.clone();
        let confirm_btn: Element = {
            let h = on_confirm.clone();
            rect()
                .padding(Gaps::new(7., 18., 7., 18.))
                .corner_radius(CornerRadius::new_all(8.))
                .background(confirm_bg)
                .on_press(move |_: Event<PressEventData>| {
                    if let Some(ref cb) = h { cb.call(()); }
                })
                .child(label().font_size(13.).color(confirm_fg).text(confirm_label))
                .into_element()
        };

        let btn_row = rect()
            .direction(Direction::Horizontal)
            .main_align(Alignment::End)
            .spacing(8.)
            .child(cancel_btn)
            .child(confirm_btn);

        // ── Card ─────────────────────────────────────────────────────────────
        let card = rect()
            .direction(Direction::Vertical)
            .spacing(12.)
            .padding(Gaps::new_all(24.))
            .corner_radius(CornerRadius::new_all(14.))
            .background(th.panel())
            .border(Border::new().fill(th.hairline_strong()).width(1.))
            .shadow((0.0_f32, 12.0_f32, 40.0_f32, 0.0_f32, th.shadow_deep()))
            .min_width(Size::px(320.))
            .max_width(Size::px(420.))
            // Swallow presses inside the card so they don't bubble to the backdrop's
            // on_press (which cancels). Only the dim area outside the card cancels.
            .on_press(move |e: Event<PressEventData>| { e.stop_propagation(); })
            .child(
                label()
                    .font_size(15.)
                    .color(th.text())
                    .font_weight(FontWeight::BOLD)
                    .text(self.title.clone()),
            )
            .child(
                label()
                    .font_size(13.)
                    .color(th.subtext())
                    .text(self.body.clone()),
            )
            .child(btn_row);

        // ── Full-window dimmed backdrop + centered card ───────────────────────
        // Window-relative size (NOT `fill`, which would size to the parent — here the
        // narrow sidebar column — and push the card off-screen). `window_percent(100)`
        // + a global (0,0) origin makes this a true full-window backdrop regardless of
        // where the dialog is mounted, so `.center()` centers the card in the window.
        rect()
            .layer(Layer::Overlay)
            .position(Position::new_global().left(0.0).top(0.0))
            .width(Size::window_percent(100.))
            .height(Size::window_percent(100.))
            .background(Theme::with_alpha(th.bg_deep(), 0xb3)) // ~70% opacity
            .center()
            .on_press(move |_: Event<PressEventData>| {
                if let Some(ref cb) = on_cancel { cb.call(()); }
            })
            .on_global_key_down(move |e: Event<KeyboardEventData>| {
                if e.key == Key::Named(NamedKey::Escape) {
                    if let Some(ref cb) = on_cancel2 { cb.call(()); }
                }
            })
            .child(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn confirm_dialog_renders_title_body_buttons() {
        fn app() -> impl IntoElement {
            ConfirmDialog::new(Theme::default())
                .title("Delete \"My chat\"?".to_string())
                .body("This can't be undone.".to_string())
                .confirm_label("Delete")
                .danger(true)
                .on_confirm((|()| {}).into())
                .on_cancel((|()| {}).into())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        for needle in ["My chat", "can't be undone", "Delete", "Cancel"] {
            assert!(
                t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains(needle))).is_some(),
                "ConfirmDialog must render {needle:?}"
            );
        }
    }
}
