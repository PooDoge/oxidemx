//! `MenuSurface` — deep-shadow wrapper for all OxideMX floating menus.
//!
//! Fixes the full-width bug: width hugs content within `[min_w, max_w]`
//! (via `Content::fit()` on the wrapper + `Menu`'s own intrinsic sizing).
//! The deep shadow lives here so the `Menu` container's own shadow is suppressed
//! to `Color::TRANSPARENT` in `menu_theme()`.
use freya::prelude::*;

use crate::tokens::Theme;
use super::theme::menu_theme;

/// A floating surface that wraps a `Menu` with a deep drop-shadow and correct
/// width hugging.
///
/// Dismissal rides on the inner Freya [`Menu`]: when `on_close` is set, it is
/// threaded into `Menu::on_close`, so the menu dismisses on an outside
/// `on_global_pointer_press` + Escape — the proven Freya model. Because the
/// dismiss handler lives on the `Menu` node (which is only mounted while the
/// popover is open), the opening click can never reach it and self-close.
///
/// Builder usage:
/// ```ignore
/// MenuSurface::new(theme)
///     .min_w(180.)
///     .max_w(320.)
///     .on_close(move |_| open.set(false))
///     .child(body_element)
/// ```
#[derive(Clone, PartialEq)]
pub struct MenuSurface {
    pub theme: Theme,
    pub min_w: f32,
    pub max_w: f32,
    child: Option<Element>,
    on_close: Option<EventHandler<()>>,
}

impl MenuSurface {
    pub fn new(theme: Theme) -> Self {
        Self { theme, min_w: 180., max_w: 360., child: None, on_close: None }
    }

    pub fn min_w(mut self, v: f32) -> Self {
        self.min_w = v;
        self
    }

    pub fn max_w(mut self, v: f32) -> Self {
        self.max_w = v;
        self
    }

    pub fn child(mut self, el: impl IntoElement) -> Self {
        self.child = Some(el.into_element());
        self
    }

    /// Set the dismissal handler. Threaded into the inner Freya [`Menu`]'s
    /// `on_close`, which fires on outside-press + Escape.
    pub fn on_close(mut self, h: impl Into<EventHandler<()>>) -> Self {
        self.on_close = Some(h.into());
        self
    }
}

impl Component for MenuSurface {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let (container_theme, _) = menu_theme(th);

        let body: Element = self.child.clone().unwrap_or_else(|| {
            rect().into_element()
        });

        // Our `Popover` overlay is the sole positioner; tell the inner Freya `Menu`
        // to skip its own overflow-aware self-offset so it can't escape our box.
        let mut menu = Menu::new().theme(container_theme).host_positioned(true).child(body);
        if let Some(h) = self.on_close.clone() {
            menu = menu.on_close(h);
        }

        rect()
            .corner_radius(CornerRadius::new_all(12.))
            .shadow((0.0_f32, 18.0_f32, 44.0_f32, 0.0_f32, th.shadow_deep()))
            .content(Content::fit())
            .min_width(Size::px(self.min_w))
            .max_width(Size::px(self.max_w))
            .child(menu)
    }
}
