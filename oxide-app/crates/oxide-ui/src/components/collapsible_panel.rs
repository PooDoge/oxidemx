//! A panel that toggles between a full-width body and a narrow rail.
//!
//! `Element` in freya rc.23 is `Clone` but not `Default`, so `full` and `rail`
//! are stored as `Option<Element>` and emitted via `.maybe_child(option)`.
use freya::prelude::*;

use crate::tokens::{SIDEBAR_FULL_W, SIDEBAR_RAIL_W, Theme};

/// A panel that shows `full` content at `width` when expanded, and `rail`
/// content at [`SIDEBAR_RAIL_W`] when collapsed.
///
/// Builder usage:
/// ```ignore
/// CollapsiblePanel::new()
///     .collapsed(false)
///     .full(label().text("Sidebar"))
///     .rail(label().text("≡"))
/// ```
#[derive(PartialEq, Clone)]
pub struct CollapsiblePanel {
    width: f32,
    collapsed: bool,
    full: Option<Element>,
    rail: Option<Element>,
    theme: Theme,
}

impl CollapsiblePanel {
    pub fn new() -> Self {
        Self {
            width: SIDEBAR_FULL_W,
            collapsed: false,
            full: None,
            rail: None,
            theme: Theme::default(),
        }
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    pub fn collapsed(mut self, c: bool) -> Self {
        self.collapsed = c;
        self
    }

    pub fn full(mut self, e: impl IntoElement) -> Self {
        self.full = Some(e.into_element());
        self
    }

    pub fn rail(mut self, e: impl IntoElement) -> Self {
        self.rail = Some(e.into_element());
        self
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }
}

impl Default for CollapsiblePanel {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for CollapsiblePanel {
    fn render(&self) -> impl IntoElement {
        let (w, body) = if self.collapsed {
            (SIDEBAR_RAIL_W, self.rail.clone())
        } else {
            (self.width, self.full.clone())
        };
        rect()
            .width(Size::px(w))
            .height(Size::fill())
            .background(self.theme.surface())
            .maybe_child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn shows_full_content_when_expanded() {
        fn app() -> impl IntoElement {
            CollapsiblePanel::new()
                .collapsed(false)
                .full(label().text("FULL"))
                .rail(label().text("RAIL"))
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "FULL")
        });
        assert!(found.is_some(), "expanded panel should show FULL label");
    }

    #[test]
    fn shows_rail_content_when_collapsed() {
        fn app() -> impl IntoElement {
            CollapsiblePanel::new()
                .collapsed(true)
                .full(label().text("FULL"))
                .rail(label().text("RAIL"))
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "RAIL")
        });
        assert!(found.is_some(), "collapsed panel should show RAIL label");
    }
}
