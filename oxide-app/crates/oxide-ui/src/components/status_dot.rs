//! A small connection-status indicator dot.
//!
//! Green when `ok`, red when not `ok`. Renders as a fixed-size square with
//! a rounded corner so it appears circular.
use freya::prelude::*;

/// A small circular status indicator.
///
/// Builder usage:
/// ```ignore
/// StatusDot::new(true)   // green = connected
/// StatusDot::new(false)  // red   = disconnected
/// ```
#[derive(PartialEq, Clone)]
pub struct StatusDot {
    ok: bool,
}

impl StatusDot {
    pub fn new(ok: bool) -> Self {
        Self { ok }
    }
}

impl Component for StatusDot {
    fn render(&self) -> impl IntoElement {
        let color = if self.ok {
            Color::from_rgb(123, 224, 106)  // green
        } else {
            Color::from_rgb(255, 80, 80)    // red
        };
        rect()
            .width(Size::px(10.))
            .height(Size::px(10.))
            .corner_radius(CornerRadius::new_all(5.))
            .background(color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn status_dot_renders() {
        fn app() -> impl IntoElement {
            StatusDot::new(true)
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        // StatusDot has no label — assert we got a Rect (the dot itself).
        let found = t.find(|_, el| Rect::try_downcast(el));
        assert!(found.is_some(), "status dot should render a rect");
    }
}
