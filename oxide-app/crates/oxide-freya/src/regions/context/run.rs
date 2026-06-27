//! Run tab — live flow-run stages. Slice 1: empty-state only (real conductor
//! run state lands in the Run-data follow-on slice).
use freya::prelude::*;
use oxide_ui::Theme;

use crate::state::AppState;

#[allow(dead_code)]
#[derive(PartialEq, Clone)]
pub struct RunTab {
    pub state: AppState,
}

impl Component for RunTab {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        rect()
            .direction(Direction::Vertical)
            .spacing(8.)
            .padding(Gaps::new_all(14.))
            .child(section_label(th, "Run"))
            .child(label().text("No active run").font_size(12.5).color(th.subtext()))
            .child(label().text("flow stages coming").font_size(10.5).color(th.faint()))
    }
}

/// Design's uppercase section label (faint, letter-spaced).
pub(super) fn section_label(th: Theme, t: &str) -> impl IntoElement {
    label().text(t.to_uppercase()).font_size(10.).color(th.faint())
}
