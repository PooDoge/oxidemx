use freya::prelude::*;
use oxide_ui::Theme;

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct SpecDirection {
    pub state: AppState,
}

impl Component for SpecDirection {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let s = &self.state;
        let cur = s.current_project.read().clone();
        let projects = s.projects.read().clone();
        let proj = cur.as_ref().and_then(|id| projects.iter().find(|p| &p.id == id));
        let (name, dir) = proj
            .map(|p| (p.name.clone(), p.default_working_dir.clone()))
            .unwrap_or_else(|| ("—".into(), "—".into()));
        rect().direction(Direction::Vertical).spacing(8.).padding(Gaps::new_all(12.))
            .child(label().text("Project").font_size(11.).color(th.faint()))
            .child(label().text(name).font_size(13.).color(th.text()))
            .child(label().text("Working dir").font_size(11.).color(th.faint()))
            .child(label().text(dir).font_size(12.).color(th.text()))
            .child(label().text("Spec viewer coming").font_size(10.5).color(th.faint()))
    }
}
