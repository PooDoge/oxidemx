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
    override_width: Option<f32>,
}

impl CollapsiblePanel {
    pub fn new() -> Self {
        Self {
            width: SIDEBAR_FULL_W,
            collapsed: false,
            full: None,
            rail: None,
            theme: Theme::default(),
            override_width: None,
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

    pub fn override_width(mut self, w: Option<f32>) -> Self {
        self.override_width = w;
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
        let (collapsed_w, body) = if self.collapsed {
            (SIDEBAR_RAIL_W, self.rail.clone())
        } else {
            (self.width, self.full.clone())
        };
        let w = self.override_width.unwrap_or(collapsed_w);
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
        let absent = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "RAIL")
        });
        assert!(absent.is_none(), "expanded panel must NOT show RAIL label");
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
        let absent = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "FULL")
        });
        assert!(absent.is_none(), "collapsed panel must NOT show FULL label");
    }

    #[test]
    fn override_width_sets_panel_width_but_body_follows_collapsed() {
        fn app() -> impl IntoElement {
            CollapsiblePanel::new()
                .collapsed(true)
                .override_width(Some(120.))
                .full(label().text("FULL"))
                .rail(label().text("RAIL"))
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        // collapsed=true still shows the RAIL body...
        let rail = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref() == "RAIL")
        });
        assert!(rail.is_some(), "override_width panel should still show RAIL when collapsed=true");
        // ...and the outer panel node measured 120px wide (the override), not 60.
        let node = t.find(|node, _| {
            let w = node.layout().area.width();
            if (w - 120.).abs() < 0.5 { Some(()) } else { None }
        });
        assert!(node.is_some(), "override_width should force a 120px panel width");
    }
}
