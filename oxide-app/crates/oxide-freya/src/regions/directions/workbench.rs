use freya::prelude::*;

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct WorkbenchDirection {
    pub state: AppState,
}

impl Component for WorkbenchDirection {
    fn render(&self) -> impl IntoElement {
        rect().padding(Gaps::new_all(12.)).child(label().text("Workbench"))
    }
}
