//! AttachMenu — the floating source-picker that appears when the user clicks the
//! attach (+) button in the Composer toolbar (Task 9).
//!
//! This component renders ONLY the menu body. The trigger/anchor is supplied by
//! the Toolbar (Task 11) via `Attached`; do not build the anchor here.
//!
//! Refactored (Task 4) onto shared menu primitives:
//! - `MenuSurface` replaces the bespoke shadow-wrapper + `Menu::new().theme(...)`.
//!   Width now hugs content within `[min_w=200, max_w=280]` (bug #3 fixed).
//! - `MenuRow` replaces the bespoke `MenuButton` + `rect().content(Content::Flex)...`
//!   row layout (bug #1: dark hover from `menu_theme`).
//!
//! Builder usage:
//! ```ignore
//! fn app() -> impl IntoElement {
//!     let mut open = use_state(|| true);
//!     AttachMenu::new(Theme::default())
//!         .on_pick(move |id: &'static str| println!("picked: {id}"))
//! }
//! ```
use freya::prelude::*;

use crate::tokens::Theme;
use crate::components::menu::{MenuRow, MenuSurface};
use super::attachment::ATTACH_SOURCES;

// ── AttachMenu ────────────────────────────────────────────────────────────────

/// Floating menu body listing the six attachment sources.
///
/// Renders a `MenuSurface` styled to the design spec with deep drop shadow.
/// Each row shows the source icon, label, and faint hint via `MenuRow`.
/// Fires `on_pick(source.id)` when the user selects a row.
#[derive(Clone, PartialEq)]
pub struct AttachMenu {
    theme:    Theme,
    on_pick:  Option<EventHandler<&'static str>>,
    on_close: Option<EventHandler<()>>,
}

impl AttachMenu {
    pub fn new(theme: Theme) -> Self {
        Self { theme, on_pick: None, on_close: None }
    }

    pub fn on_pick(mut self, handler: impl Into<EventHandler<&'static str>>) -> Self {
        self.on_pick = Some(handler.into());
        self
    }

    /// Dismissal handler, threaded into the `MenuSurface` (Freya `Menu`'s
    /// `on_close`): fires on outside-press + Escape.
    pub fn on_close(mut self, handler: impl Into<EventHandler<()>>) -> Self {
        self.on_close = Some(handler.into());
        self
    }
}

impl Component for AttachMenu {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;

        // Build one MenuRow per attach source.
        let mut rows: Vec<Element> = Vec::new();
        for source in ATTACH_SOURCES.iter() {
            let on_pick = self.on_pick.clone();
            let id = source.id;

            let row = MenuRow::new(th)
                .icon(Some(source.icon))
                .title(source.label)
                .subtitle(Some(source.hint.to_string()))
                .on_press(move |_| {
                    if let Some(h) = &on_pick {
                        h.call(id);
                    }
                })
                .into_element();

            rows.push(row);
        }

        let body = rect()
            .direction(Direction::Vertical)
            .children(rows)
            .into_element();

        let mut surface = MenuSurface::new(th)
            .min_w(200.)
            .max_w(280.)
            .child(body);
        if let Some(h) = self.on_close.clone() {
            surface = surface.on_close(h);
        }
        surface
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    /// Step 1 (brief): mount AttachMenu, assert "Upload file" renders.
    #[test]
    fn attach_menu_shows_upload_file_label() {
        fn app() -> impl IntoElement {
            AttachMenu::new(Theme::default())
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Upload file"))
        });
        assert!(found.is_some(), "AttachMenu should render the 'Upload file' label");
    }

    /// All six source labels must appear.
    #[test]
    fn attach_menu_renders_all_six_sources() {
        fn app() -> impl IntoElement {
            AttachMenu::new(Theme::default())
        }
        let mut t = launch_test(app);
        t.sync_and_update();

        for source in ATTACH_SOURCES.iter() {
            let label_text = source.label;
            let found = t.find(|_, el| {
                Label::try_downcast(el).filter(|l| l.text.as_ref().contains(label_text))
            });
            assert!(found.is_some(), "Missing label: {label_text}");
        }
    }

    /// Snapshot: renders the attach menu at 760px canvas width (menu stays narrow).
    /// Run with: LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui composer::attach_menu::tests::snapshot_attach_menu -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_attach_menu() {
        fn app() -> impl IntoElement {
            rect()
                .background(Theme::default().bg_deep())
                .padding(Gaps::new_all(16.))
                .content(Content::fit())
                .child(
                    AttachMenu::new(Theme::default())
                        .on_pick(|id| { let _ = id; }),
                )
        }
        let (mut runner, _) =
            TestingRunner::new(app, (760., 500.).into(), |_| {}, 1.);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-attach-menu.png");
    }
}
