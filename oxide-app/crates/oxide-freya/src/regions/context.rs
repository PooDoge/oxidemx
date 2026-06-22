//! Right region: a status-rail placeholder (the 4 directions land in 2b).
use freya::prelude::*;
use oxide_ui::components::CollapsiblePanel;

#[derive(PartialEq, Clone)]
pub struct ContextRegion {
    pub collapsed: bool,
}

impl Component for ContextRegion {
    fn render(&self) -> impl IntoElement {
        CollapsiblePanel::new()
            .collapsed(self.collapsed)
            .full(label().text("Status"))
            .rail(label().text("◔"))
    }
}
