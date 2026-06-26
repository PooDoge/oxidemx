use freya::prelude::*;

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct AmbientDirection {
    pub state: AppState,
}

impl Component for AmbientDirection {
    fn render(&self) -> impl IntoElement {
        rect().padding(Gaps::new_all(12.)).child(label().text("Ambient"))
    }
}
