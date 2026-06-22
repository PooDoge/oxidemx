//! Root shell: horizontal layout of three independently-navigable regions.
use std::sync::Arc;

use freya::prelude::*;
use oxide_client::{Transport, UdsTransport};

use crate::regions::{context::ContextRegion, main_region::MainRegion, sidebar::Sidebar};
use crate::state::{AppState, ConnState};

/// Root shell component — call from `fn app()`.
pub fn shell() -> impl IntoElement {
    let transport: Arc<dyn Transport> = Arc::new(UdsTransport::new(UdsTransport::default_socket()));
    let state = AppState::new(transport);
    state.bootstrap();
    let conn = *state.connection.read();

    rect()
        .direction(Direction::Horizontal)
        .expanded()
        .background((5u8, 7u8, 11u8))
        .maybe_child(connection_banner(conn))
        .child(Sidebar { state: state.clone(), collapsed: false })
        .child(
            rect()
                .width(Size::flex(1.0))
                .height(Size::fill())
                .child(MainRegion { state: state.clone() }),
        )
        .child(ContextRegion { collapsed: true })
}

fn connection_banner(conn: ConnState) -> Option<impl IntoElement> {
    match conn {
        ConnState::Unreachable => Some(
            label()
                .text("agentd unavailable — retry")
                .color(Color::from_rgb(255, 120, 120)),
        ),
        ConnState::Reconnecting => Some(
            label()
                .text("reconnecting…")
                .color(Color::from_rgb(255, 171, 64)),
        ),
        _ => None,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use freya::prelude::*;
    use freya_testing::prelude::*;
    use oxide_client::{
        AgentEvent, Conversation, ConversationId, ProjectId, Turn, mock::MockTransport,
    };

    use crate::regions::{main_region::MainRegion, sidebar::Sidebar};
    use crate::state::AppState;

    // ── Helpers ────────────────────────────────────────────────────────────

    fn mock_with_history() -> Arc<MockTransport> {
        Arc::new(MockTransport {
            conversations: vec![Conversation {
                id: ConversationId::from("c1"),
                project_id: ProjectId::from("personal"),
                title: "Chat 1".into(),
                working_dir: String::new(),
                model: String::new(),
                created_at: 0,
                updated_at: 0,
            }],
            history: vec![Turn { role: "user".into(), text: "seed-message".into(), ts: 0 }],
            events: Mutex::new(Vec::new()),
            ..MockTransport::new()
        })
    }

    // ── Test harness apps ─────────────────────────────────────────────────

    /// Harness for test 1: sidebar + center sharing one AppState.
    /// The mock has history so that `open_conversation` loads a turn.
    fn harness_with_history(mock: Arc<MockTransport>) -> impl Fn() -> Element {
        move || {
            let transport = mock.clone() as Arc<dyn oxide_client::Transport>;
            let state = AppState::new(transport);
            // Open the seeded conversation on mount to load history into transcript.
            let st = state.clone();
            use_side_effect(move || {
                st.open_conversation(ConversationId::from("c1"));
            });
            rect()
                .direction(Direction::Horizontal)
                .expanded()
                .child(Sidebar { state: state.clone(), collapsed: false })
                .child(
                    rect()
                        .width(Size::flex(1.0))
                        .height(Size::fill())
                        .child(MainRegion { state: state.clone() }),
                )
                .into()
        }
    }

    // ── Test 1: sidebar nav does not clear center thread ──────────────────

    /// Navigate the sidebar to its Placeholder page; assert the center thread's
    /// bubble text ("seed-message") is STILL present.
    ///
    /// This fails if the center remounts and clears its transcript state.
    #[test]
    fn sidebar_nav_does_not_clear_center_thread() {
        let mock = mock_with_history();
        let app = harness_with_history(mock);
        let mut runner = launch_test(app);

        // Drive async tasks so open_conversation + get_history complete.
        runner.poll_n(Duration::from_millis(5), 8);

        // Assert the seed turn is visible in the center.
        let before_nav = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("seed-message"))
        });
        assert!(
            before_nav.is_some(),
            "before sidebar nav: center should show 'seed-message' turn"
        );

        // Click the sidebar nav button (≡ Conversations header) to navigate to Placeholder.
        // The sidebar occupies the left portion; the header button is near the top-left.
        // In a 500×500 window the CollapsiblePanel is SIDEBAR_FULL_W=274px wide.
        runner.click_cursor((30., 15.));
        runner.sync_and_update();

        // Assert the sidebar navigated to its placeholder page.
        let placeholder = runner.find(|_, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("Sidebar placeholder page"))
        });
        assert!(
            placeholder.is_some(),
            "after click: sidebar should show placeholder page"
        );

        // INVARIANT: center still shows the seed turn — it was NOT remounted.
        let after_nav = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("seed-message"))
        });
        assert!(
            after_nav.is_some(),
            "after sidebar nav: center thread MUST still show 'seed-message' (independence invariant)"
        );
    }

    // ── Test 2: sending renders streamed reply ────────────────────────────

    /// End-to-end send path:
    ///   open_conversation (subscribes mock stream) → send("ping") → stream delivers
    ///   final{text:"pong"} → assert BOTH the user "ping" bubble AND the assistant
    ///   "pong" bubble are rendered by MainRegion.
    ///
    /// Harness approach: a nested component captures `AppState` and calls both
    /// `open_conversation` and `send` from `use_side_effect` on mount, giving the
    /// test full control without keyboard simulation.
    #[test]
    fn sending_renders_streamed_reply() {
        fn open_and_send_app() -> Element {
            let state = AppState::new(
                Arc::new(MockTransport {
                    events: Mutex::new(vec![AgentEvent {
                        seq: 1,
                        kind: "final".into(),
                        payload: serde_json::json!({"text": "pong"}),
                    }]),
                    conversations: vec![Conversation {
                        id: ConversationId::from("c1"),
                        project_id: ProjectId::from("personal"),
                        title: "Chat 1".into(),
                        working_dir: String::new(),
                        model: String::new(),
                        created_at: 0,
                        updated_at: 0,
                    }],
                    ..MockTransport::new()
                }) as Arc<dyn oxide_client::Transport>,
            );

            // Open conversation first (so the subscribe stream is live), then send.
            // `apply_user` is synchronous — "ping" appears in the transcript immediately.
            // The mock subscribe stream yields final{text:"pong"} once polled.
            let st = state.clone();
            use_side_effect(move || {
                st.open_conversation(ConversationId::from("c1"));
                st.send("ping".to_string());
            });

            rect()
                .width(Size::fill())
                .height(Size::fill())
                .child(MainRegion { state })
                .into()
        }

        let mut runner = launch_test(open_and_send_app);
        // Drive async tasks: open_conversation spawns get_history + subscribe stream;
        // send() spawns send_message (fire-and-forget); stream events flush transcript.
        runner.poll_n(Duration::from_millis(5), 12);

        // Assert the user "ping" turn is rendered (apply_user path).
        let ping_bubble = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("ping"))
        });
        assert!(
            ping_bubble.is_some(),
            "after send('ping'), MainRegion must render a user 'ping' bubble (apply_user path)"
        );

        // Assert the assistant "pong" reply is rendered (streamed final event path).
        let pong_bubble = runner.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("pong"))
        });
        assert!(
            pong_bubble.is_some(),
            "after subscribe stream delivers 'pong' final event, MainRegion must render an assistant 'pong' bubble"
        );
    }
}
