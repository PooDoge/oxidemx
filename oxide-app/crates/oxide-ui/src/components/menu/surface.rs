//! `MenuSurface` — deep-shadow wrapper for all OxideMX floating menus.
//!
//! Fixes the full-width bug: width hugs content within `[min_w, max_w]`
//! (via `Content::fit()` on the wrapper + `Menu`'s own intrinsic sizing).
//! The deep shadow lives here so the `Menu` container's own shadow is suppressed
//! to `Color::TRANSPARENT` in `menu_theme()`.
use freya::prelude::*;

use crate::tokens::Theme;
use super::theme::menu_theme;

/// Context that lets a descendant menu row ask the surface to dismiss.
/// `Some(handler)` only when the surface is in light-dismiss mode with an `on_close`.
///
/// Note: `EventHandler<T>` wraps `Rc<RefCell<...>>` so it is `Clone` but NOT `Copy`
/// in this Freya version.
#[derive(Clone)]
pub struct MenuDismiss(pub Option<EventHandler<()>>);

/// A floating surface that wraps a `Menu` with a deep drop-shadow and correct
/// width hugging.
///
/// When `light_dismiss` is false (the default), dismissal rides on the inner Freya
/// [`Menu`]: when `on_close` is set, it is threaded into `Menu::on_close`, so the menu
/// dismisses on an outside `on_global_pointer_press` + Escape — the proven Freya model.
/// Because the dismiss handler lives on the `Menu` node (which is only mounted while the
/// popover is open), the opening click can never reach it and self-close.
///
/// When `light_dismiss(true)` is set, the surface instead owns the dismissal:
/// outside press + Escape close the menu (NOT inside clicks), and a
/// [`MenuDismiss`] context is provided so descendant rows can dismiss
/// declaratively.
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
    light_dismiss: bool,
}

impl MenuSurface {
    pub fn new(theme: Theme) -> Self {
        Self { theme, min_w: 180., max_w: 360., child: None, on_close: None, light_dismiss: false }
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

    /// Opt into light-dismiss: the menu closes only on an outside press or Escape
    /// (not on inside clicks), and provides a `MenuDismiss` context so rows can
    /// dismiss declaratively. Default false keeps Freya's any-click-close.
    pub fn light_dismiss(mut self, v: bool) -> Self {
        self.light_dismiss = v;
        self
    }
}

impl Component for MenuSurface {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let (container_theme, _) = menu_theme(th);

        // Hooks first, unconditionally.
        let mut area = use_state(|| None::<Area>);
        let dismiss = if self.light_dismiss { self.on_close.clone() } else { None };
        use_provide_context(move || MenuDismiss(dismiss.clone()));

        let body: Element = self.child.clone().unwrap_or_else(|| rect().into_element());

        // Our `Popover` overlay positions the menu; tell the inner Freya `Menu` to
        // skip its own overflow self-offset. In light-dismiss mode do NOT thread
        // `on_close` into the `Menu` (its global press fires on every inside click);
        // dismissal is handled below. Otherwise keep today's Freya-driven dismissal.
        let mut menu = Menu::new().theme(container_theme).host_positioned(true);
        if !self.light_dismiss {
            if let Some(h) = self.on_close.clone() {
                menu = menu.on_close(h);
            }
        }
        let menu = menu.child(body);

        let mut surface = rect()
            .corner_radius(CornerRadius::new_all(12.))
            .shadow((0.0_f32, 18.0_f32, 44.0_f32, 0.0_f32, th.shadow_deep()))
            .content(Content::fit())
            .min_width(Size::px(self.min_w))
            .max_width(Size::px(self.max_w));

        if self.light_dismiss {
            let on_close = self.on_close.clone();
            let on_close2 = self.on_close.clone();
            surface = surface
                .on_sized(move |e: Event<SizedEventData>| {
                    area.set_if_modified(Some(e.area));
                })
                .on_global_pointer_press(move |e: Event<PointerEventData>| {
                    if let Some(a) = *area.peek() {
                        let p = e.global_location();
                        let inside = point_in_rect(
                            a.origin.x as f64, a.origin.y as f64,
                            a.size.width as f64, a.size.height as f64,
                            p.x, p.y,
                        );
                        if !inside {
                            if let Some(h) = &on_close {
                                h.call(());
                            }
                        }
                    }
                })
                .on_global_key_down(move |e: Event<KeyboardEventData>| {
                    if e.key == Key::Named(NamedKey::Escape) {
                        if let Some(h) = &on_close2 {
                            h.call(());
                        }
                    }
                });
        }

        surface.child(menu)
    }
}

/// True if `(px,py)` lies within the rect at `(x,y)` of size `(w,h)`.
/// Pure geometry so the light-dismiss bounds-check is unit-testable without
/// Freya event types (and explicit about which coord fields feed it).
pub(crate) fn point_in_rect(x: f64, y: f64, w: f64, h: f64, px: f64, py: f64) -> bool {
    px >= x && px <= x + w && py >= y && py <= y + h
}

#[cfg(test)]
mod tests {
    use super::point_in_rect;

    #[test]
    fn point_in_rect_inside_edge_outside() {
        // rect at (100,200) size 80x40 -> spans x[100,180], y[200,240]
        assert!(point_in_rect(100., 200., 80., 40., 140., 220.), "center is inside");
        assert!(point_in_rect(100., 200., 80., 40., 100., 200.), "top-left corner is inside (inclusive)");
        assert!(point_in_rect(100., 200., 80., 40., 180., 240.), "bottom-right corner is inside (inclusive)");
        assert!(!point_in_rect(100., 200., 80., 40., 99., 220.), "left of rect is outside");
        assert!(!point_in_rect(100., 200., 80., 40., 140., 241.), "below rect is outside");
    }
}
