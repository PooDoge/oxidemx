use freya::prelude::*;
use oxide_ui::Theme;

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct WorkbenchDirection {
    pub state: AppState,
}

impl Component for WorkbenchDirection {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let s = &self.state;
        let active = s.active.read().clone();
        let convs = s.conversations.read().clone();
        let conv = active.as_ref().and_then(|id| convs.iter().find(|c| &c.id == id));
        let body = match conv {
            Some(c) => {
                let wt = c.worktree.as_ref()
                    .map(|w| format!("{} @ {}", w.branch, w.path))
                    .unwrap_or_else(|| "no worktree".into());
                rect().direction(Direction::Vertical).spacing(8.)
                    .child(label().text("Working dir").font_size(11.).color(th.faint()))
                    .child(label().text(if c.working_dir.is_empty() { "—".into() } else { c.working_dir.clone() })
                        .font_size(12.).color(th.text()))
                    .child(label().text("Worktree").font_size(11.).color(th.faint()))
                    .child(label().text(wt).font_size(12.).color(th.text()))
            }
            None => rect().child(label().text("No conversation selected").font_size(12.).color(th.faint())),
        };
        rect().direction(Direction::Vertical).spacing(8.).padding(Gaps::new_all(12.))
            .child(body)
            .child(label().text("Diffs + tools coming").font_size(10.5).color(th.faint()))
    }
}
