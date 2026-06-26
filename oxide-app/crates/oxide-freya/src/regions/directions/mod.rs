//! The right-panel "directions" — Spec / Mission / Workbench / Ambient.
use freya::prelude::*;

use crate::state::{AppState, StatusDirection};

mod ambient;
mod mission;
mod spec;
mod workbench;

/// Renders the body for one direction. Each arm is its own component so the
/// later (2d) rich content drops into a single file.
#[derive(PartialEq, Clone)]
pub struct DirectionPanel {
    pub state: AppState,
    pub direction: StatusDirection,
}

impl Component for DirectionPanel {
    fn render(&self) -> impl IntoElement {
        let s = self.state.clone();
        match self.direction {
            StatusDirection::Spec => spec::SpecDirection { state: s }.into_element(),
            StatusDirection::Mission => mission::MissionDirection { state: s }.into_element(),
            StatusDirection::Workbench => workbench::WorkbenchDirection { state: s }.into_element(),
            StatusDirection::Ambient => ambient::AmbientDirection { state: s }.into_element(),
        }
    }
}
