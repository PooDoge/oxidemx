use freya::prelude::*;
use oxide_ui::Theme;

use crate::state::{AppState, ConnState};

#[derive(PartialEq, Clone)]
pub struct AmbientDirection {
    pub state: AppState,
}

impl Component for AmbientDirection {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let s = &self.state;
        let conn = *s.connection.read();
        let (txt, col) = match conn {
            ConnState::Connected => ("connected", th.accent()),
            ConnState::Reconnecting => ("reconnecting…", Color::from_rgb(255, 171, 64)),
            ConnState::Unreachable => ("unreachable", Color::from_rgb(255, 120, 120)),
            ConnState::Unknown => ("unknown", th.faint()),
        };
        rect().direction(Direction::Vertical).spacing(8.).padding(Gaps::new_all(12.))
            .child(label().text("Connection").font_size(11.).color(th.faint()))
            .child(label().text(txt).font_size(13.).color(col))
            .child(label().text("Activity feed coming").font_size(10.5).color(th.faint()))
    }
}
