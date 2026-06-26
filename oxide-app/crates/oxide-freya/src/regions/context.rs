//! Right region: a 60px direction rail (collapsed) or a segmented expanded panel.
use freya::prelude::*;
use oxide_ui::{
    Theme,
    components::RailButton,
};

use crate::regions::directions::DirectionPanel;
use crate::state::{AppState, StatusDirection};

const CONTEXT_FULL_W: f32 = 300.0;
const CONTEXT_RAIL_W: f32 = 60.0;

#[derive(PartialEq, Clone)]
pub struct ContextRegion {
    pub state: AppState,
}

impl Component for ContextRegion {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let state = self.state.clone();
        let collapsed = *state.context_collapsed.read();
        let active = *state.active_direction.read();

        if collapsed {
            // Rail: 4 direction icons + expand toggle.
            let mut rail = rect()
                .direction(Direction::Vertical)
                .cross_align(Alignment::Center)
                .spacing(6.)
                .padding(Gaps::new_all(8.))
                .width(Size::px(CONTEXT_RAIL_W))
                .height(Size::fill())
                .background(th.panel());
            for d in StatusDirection::ALL {
                let mut act = state.active_direction;
                let mut coll = state.context_collapsed;
                let selected = d == active;
                rail = rail.child(
                    rect()
                        .width(Size::px(34.)).height(Size::px(34.))
                        .corner_radius(CornerRadius::new_all(9.))
                        .center()
                        .background(if selected { th.surface_hi() } else { th.surface() })
                        .on_press(move |_: Event<PressEventData>| {
                            act.set(d);
                            coll.set(false);
                        })
                        .child(
                            label().text(d.icon()).font_size(15.)
                                .color(if selected { th.accent() } else { th.faint() }),
                        ),
                );
            }
            rail.into_element()
        } else {
            // Expanded: segmented header + active direction body.
            let mut header = rect()
                .direction(Direction::Horizontal)
                .content(Content::Flex)
                .spacing(4.)
                .padding(Gaps::new_all(8.))
                .width(Size::fill());
            for d in StatusDirection::ALL {
                let mut act = state.active_direction;
                let selected = d == active;
                header = header.child(
                    rect()
                        .width(Size::flex(1.0))
                        .center()
                        .padding(Gaps::new(6., 4., 6., 4.))
                        .corner_radius(CornerRadius::new_all(8.))
                        .background(if selected { th.accent_dim() } else { th.surface() })
                        .on_press(move |_: Event<PressEventData>| act.set(d))
                        .child(
                            label().text(d.label()).font_size(11.5)
                                .color(if selected { th.text() } else { th.faint() }),
                        ),
                );
            }
            let mut coll = state.context_collapsed;
            rect()
                .direction(Direction::Vertical)
                .width(Size::px(CONTEXT_FULL_W))
                .height(Size::fill())
                .background(th.panel())
                .child(
                    rect()
                        .direction(Direction::Horizontal)
                        .cross_align(Alignment::Center)
                        .width(Size::fill())
                        .child(rect().width(Size::flex(1.0)).child(header))
                        .child(
                            RailButton::new("»".into())
                                .on_press(move |_: Event<PressEventData>| coll.set(true)),
                        ),
                )
                .child(DirectionPanel { state: state.clone(), direction: active })
                .into_element()
        }
    }
}

// ── Snapshots ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod context_snapshot_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use freya::prelude::*;
    use freya_testing::TestingRunner;
    use oxide_client::mock::MockTransport;

    use crate::state::{AppState, StatusDirection};
    use super::ContextRegion;

    fn make_state(collapsed: bool, direction: StatusDirection) -> AppState {
        let mock = Arc::new(MockTransport::new()) as Arc<dyn oxide_client::Transport>;
        let mut state = AppState::new(mock);
        state.context_collapsed.set(collapsed);
        state.active_direction.set(direction);
        state
    }

    fn snapshot_rail_app() -> Element {
        use_init_theme(dark_theme);
        let th = oxide_ui::tokens::Theme::default();
        let state = make_state(true, StatusDirection::Spec);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .child(ContextRegion { state })
            .into()
    }

    fn snapshot_spec_expanded_app() -> Element {
        use_init_theme(dark_theme);
        let th = oxide_ui::tokens::Theme::default();
        let state = make_state(false, StatusDirection::Spec);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .child(ContextRegion { state })
            .into()
    }

    /// Renders the collapsed direction rail (4 icons) to a PNG for visual review.
    /// Run with: cargo test -p oxide-freya context -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_context_rail() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_rail_app, (60., 400.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 8);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-context-rail.png");
    }

    /// Renders the expanded Spec panel (segmented header + body stub) to a PNG.
    /// Run with: cargo test -p oxide-freya context -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_context_spec_expanded() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_spec_expanded_app, (300., 600.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 8);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-context-spec-expanded.png");
    }
}
