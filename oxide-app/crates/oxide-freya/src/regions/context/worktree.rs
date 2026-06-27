//! Worktree tab — the active conversation's worktree + (later) changed files.
use freya::prelude::*;
use oxide_ui::Theme;

use crate::state::AppState;
use super::run::section_label;

#[derive(PartialEq, Clone)]
pub struct WorktreeTab {
    pub state: AppState,
}

impl Component for WorktreeTab {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let active = self.state.active.read().clone();
        let convs = self.state.conversations.read().clone();
        let conv = active.as_ref().and_then(|id| convs.iter().find(|c| &c.id == id));
        let wt = conv
            .and_then(|c| c.worktree.as_ref())
            .map(|w| format!("{} @ {}", w.branch, w.path))
            .unwrap_or_else(|| "no worktree".into());
        rect()
            .direction(Direction::Vertical)
            .spacing(8.)
            .padding(Gaps::new_all(14.))
            .child(section_label(th, "Worktree"))
            .child(label().text(wt).font_size(12.).color(th.text()))
            .child(label().text("changed files coming").font_size(10.5).color(th.faint()))
    }
}
