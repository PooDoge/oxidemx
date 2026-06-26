use freya::prelude::*;
use oxide_ui::Theme;

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct MissionDirection {
    pub state: AppState,
}

impl Component for MissionDirection {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let s = &self.state;
        let active = s.active.read().clone();
        let convs = s.conversations.read().clone();
        let conv = active.as_ref().and_then(|id| convs.iter().find(|c| &c.id == id));
        let body = match conv {
            Some(c) => rect().direction(Direction::Vertical).spacing(8.)
                .child(label().text("Conversation").font_size(11.).color(th.faint()))
                .child(label().text(if c.title.is_empty() { "untitled".into() } else { c.title.clone() })
                    .font_size(13.).color(th.text()))
                .child(label().text("Model").font_size(11.).color(th.faint()))
                .child(label().text(if c.model.is_empty() { "—".into() } else { c.model.clone() })
                    .font_size(12.).color(th.text())),
            None => rect().child(label().text("No conversation selected").font_size(12.).color(th.faint())),
        };
        rect().direction(Direction::Vertical).spacing(8.).padding(Gaps::new_all(12.))
            .child(body)
            .child(label().text("Mission tracker coming").font_size(10.5).color(th.faint()))
    }
}
