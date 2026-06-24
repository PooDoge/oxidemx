//! A thin top-edge drag handle for resizing the composer panel.
//!
//! The grip emits a proposed new height (f32) via `.on_drag(handler)` as the
//! user drags vertically. The caller (Composer) is responsible for clamping
//! and visibility — call `clamp_height` to enforce min/max bounds.
//!
//! ## Usage
//! ```ignore
//! let height = use_signal(|| 200.0_f32);
//! ResizeGrip::new(height())
//!     .theme(theme)
//!     .on_drag(move |proposed: f32| {
//!         height.set(clamp_height(proposed, 121.0));
//!     })
//! ```
use freya::prelude::*;

use crate::tokens::Theme;

/// Clamp a proposed editor height between `cap_px` and the absolute maximum of
/// 520 px.
///
/// - below `cap_px`  → returns `cap_px`   (minimum floor set by the caller)
/// - above 520.0     → returns 520.0      (hard maximum)
/// - otherwise       → returns `proposed` unchanged
pub fn clamp_height(proposed: f32, cap_px: f32) -> f32 {
    proposed.clamp(cap_px, 520.0)
}

/// A 6 px tall, `surface_max`-coloured pill that sits at the top edge of the
/// composer panel and exposes a drag interface for vertical resizing.
///
/// The grip itself never gates its own visibility — the caller decides when to
/// show or hide it.
///
/// Builder usage:
/// ```ignore
/// ResizeGrip::new(current_height)
///     .theme(theme)
///     .on_drag(move |h| height.set(clamp_height(h, 121.0)))
/// ```
#[derive(PartialEq, Clone)]
pub struct ResizeGrip {
    /// Current height of the panel being resized. Stored so the drag delta can
    /// be added to produce the proposed new height.
    current_height: f32,
    theme: Theme,
    on_drag: Option<EventHandler<f32>>,
}

impl ResizeGrip {
    pub fn new(current_height: f32) -> Self {
        Self {
            current_height,
            theme: Theme::default(),
            on_drag: None,
        }
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }

    /// Register a handler that receives the *proposed* new height (current
    /// height minus the vertical drag delta — dragging up increases height).
    pub fn on_drag(mut self, handler: impl Into<EventHandler<f32>>) -> Self {
        self.on_drag = Some(handler.into());
        self
    }
}

impl Component for ResizeGrip {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let current_height = self.current_height;

        // `press_y` holds the global Y of the pointer-down event while the
        // user is dragging. None means "not dragging".
        let mut press_y: State<Option<f64>> = use_state(|| None);

        let on_drag = self.on_drag.clone();

        let on_pointer_down = move |e: Event<PointerEventData>| {
            if e.data().is_primary() {
                press_y.set(Some(e.data().global_location().y));
                e.stop_propagation();
            }
        };

        let on_drag_move = on_drag.clone();
        let on_global_pointer_move = move |e: Event<PointerEventData>| {
            if let Some(start_y) = press_y() {
                // Drag upward (negative delta_y) increases the panel height.
                let delta = e.data().global_location().y - start_y;
                let proposed = current_height - delta as f32;
                if let Some(ref handler) = on_drag_move {
                    handler.call(proposed);
                }
            }
        };

        // Any pointer release anywhere cancels the drag.
        let on_global_pointer_press = move |_: Event<PointerEventData>| {
            if press_y.read().is_some() {
                press_y.set(None);
            }
        };

        rect()
            .width(Size::fill())
            .height(Size::px(6.))
            // Centered pill — 48 px wide, 3 px tall.
            .main_align(Alignment::Center)
            .cross_align(Alignment::Center)
            .on_pointer_down(on_pointer_down)
            .on_global_pointer_move(on_global_pointer_move)
            .on_global_pointer_press(on_global_pointer_press)
            .child(
                rect()
                    .width(Size::px(48.))
                    .height(Size::px(3.))
                    .corner_radius(CornerRadius::new_all(2.))
                    .background(th.surface_max()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_respects_cap_and_max() {
        assert_eq!(clamp_height(50.0, 121.0), 121.0);   // below cap -> cap
        assert_eq!(clamp_height(300.0, 121.0), 300.0);  // in range
        assert_eq!(clamp_height(999.0, 121.0), 520.0);  // above max -> 520
    }
}
