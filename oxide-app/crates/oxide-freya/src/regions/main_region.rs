//! Center region: the active chat thread + prompt. Renders ONLY
//! transport-delivered content (Rule 1): committed turns + the live streaming
//! assistant bubble; never fabricated text.
use freya::prelude::*;
use oxide_ui::components::{Bubble, PromptInput};

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct MainRegion {
    pub state: AppState,
}

impl Component for MainRegion {
    fn render(&self) -> impl IntoElement {
        let state = self.state.clone();
        let tx = state.transcript.read().clone();
        let mut thread = rect().direction(Direction::Vertical).spacing(8.0).width(Size::fill());
        for turn in &tx.turns {
            thread = thread.child(Bubble::new(turn.role.clone(), turn.text.clone()));
        }
        if !tx.live_assistant.is_empty() {
            thread = thread.child(Bubble::new("assistant".into(), tx.live_assistant.clone()));
        }
        let input = use_state(String::new);
        let send_state = state.clone();
        rect()
            .direction(Direction::Vertical)
            .width(Size::fill())
            .height(Size::fill())
            .child(
                rect()
                    .width(Size::fill())
                    .height(Size::flex(1.0))
                    .child(ScrollView::new().child(thread)),
            )
            .child(
                PromptInput::new(input.into_writable())
                    .on_submit(move |text| send_state.send(text)),
            )
    }
}
