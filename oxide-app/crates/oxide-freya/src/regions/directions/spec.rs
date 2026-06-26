use freya::prelude::*;

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct SpecDirection {
    pub state: AppState,
}

impl Component for SpecDirection {
    fn render(&self) -> impl IntoElement {
        rect().padding(Gaps::new_all(12.)).child(label().text("Spec"))
    }
}
