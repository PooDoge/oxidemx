//! Root shell: horizontal layout of three independently-navigable regions.
use std::sync::Arc;

use freya::prelude::*;
use oxide_client::{Transport, UdsTransport};

use crate::regions::{context::ContextRegion, main_region::MainRegion, sidebar::Sidebar};
use crate::state::{AppState, ConnState};

/// Root shell component — call from `fn app()`.
pub fn shell() -> impl IntoElement {
    use_init_theme(dark_theme);
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
    use freya_testing::TestingRunner;
    use freya_testing::prelude::*;
    use oxide_client::{
        AgentEvent, Conversation, ConversationId, ProjectId, Turn, Worktree, mock::MockTransport,
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
                worktree: None,
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
            // Bootstrap to populate conversations list; open the seeded conversation.
            let st = state.clone();
            use_side_effect(move || {
                st.bootstrap();
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

    // ── Test 1: sidebar renders conversation rows ─────────────────────────

    /// Assert the styled sidebar renders a seeded conversation title as a row.
    ///
    /// The per-region-nav independence invariant is covered by the pure seam
    /// test `nav::tests::region_nav_is_independent` which does not rely on
    /// any sidebar page enum.
    #[test]
    fn sidebar_renders_conversation_rows() {
        let mock = mock_with_history(); // seeds a "Chat 1" conversation
        let app = harness_with_history(mock);
        let mut runner = launch_test(app);
        runner.poll_n(Duration::from_millis(5), 8);
        assert!(
            runner
                .find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref() == "Chat 1"))
                .is_some(),
            "sidebar should render the conversation row"
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
                        worktree: None,
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

    // ── Snapshot: render the full shell to PNGs for visual review ──────────

    fn conv(id: &str, title: &str) -> Conversation {
        Conversation {
            id: ConversationId::from(id),
            project_id: ProjectId::from("personal"),
            title: title.into(),
            working_dir: String::new(),
            model: String::new(),
            created_at: 0,
            updated_at: 0,
            worktree: None,
        }
    }

    fn snapshot_shell_app() -> Element {
        use crate::regions::context::ContextRegion;
        let mock = Arc::new(MockTransport {
            conversations: vec![
                conv("c1", "hi, what can you do?"),
                conv("c2", "run the shell command git commit -m test"),
                conv("c3", "research-digest on AutoAge"),
                conv("c4", "Run research-digest on Microsoft Copilot Studio"),
            ],
            history: vec![
                Turn { role: "user".into(), text: "What can you do?".into(), ts: 0 },
                Turn {
                    role: "assistant".into(),
                    text: "I am Oxide, your desktop assistant. I can run shell commands, search the web, \
                           remember facts across conversations, configure your radial menu, and launch \
                           multi-agent flows like research-digest and system-doctor."
                        .into(),
                    ts: 0,
                },
            ],
            events: Mutex::new(Vec::new()),
            ..MockTransport::new()
        }) as Arc<dyn oxide_client::Transport>;
        let state = AppState::new(mock);
        let st = state.clone();
        use_side_effect(move || {
            st.bootstrap();
            st.open_conversation(ConversationId::from("c1"));
        });
        rect()
            .direction(Direction::Horizontal)
            .expanded()
            .background((5u8, 7u8, 11u8))
            .child(Sidebar { state: state.clone(), collapsed: false })
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .height(Size::fill())
                    .child(MainRegion { state: state.clone() }),
            )
            .child(ContextRegion { collapsed: true })
            .into()
    }

    /// Renders the shell to /tmp PNGs at 1200x800 for visual review.
    /// Run with: cargo test -p oxide-freya --lib snapshot_shell -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNGs to /tmp for visual review"]
    fn snapshot_shell() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_shell_app, (1200., 800.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 12);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-shell-expanded.png");

        // Click the "«" collapse button (bottom of the 274px sidebar) and re-render.
        runner.click_cursor((24., 770.));
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-shell-collapsed.png");
    }

    // ── Snapshot: styled thread (P1) ──────────────────────────────────────

    fn snapshot_thread_p1_app() -> Element {
        let mock = Arc::new(MockTransport {
            conversations: vec![conv("c1", "hi, what can you do?")],
            history: vec![
                Turn { role: "user".into(), text: "What can you do?".into(), ts: 0 },
                Turn {
                    role: "assistant".into(),
                    text: "I am Oxide, your desktop assistant. I can run shell commands, \
                           search the web, and launch flows."
                        .into(),
                    ts: 0,
                },
            ],
            events: Mutex::new(Vec::new()),
            ..MockTransport::new()
        }) as Arc<dyn oxide_client::Transport>;
        let state = AppState::new(mock);
        let st = state.clone();
        use_side_effect(move || {
            st.open_conversation(ConversationId::from("c1"));
        });
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .child(MainRegion { state })
            .into()
    }

    /// Renders the styled thread (ThreadHeader + bubbles + composer) to a PNG.
    /// Run with: cargo test -p oxide-freya --bin oxide-freya snapshot_thread_p1 -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_thread_p1() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_thread_p1_app, (820., 900.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 12);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-thread-p1.png");
    }

    // ── Snapshot: styled sidebar (P2) ─────────────────────────────────────

    fn snapshot_sidebar_p2_app() -> Element {
        let mock = Arc::new(MockTransport {
            conversations: vec![
                conv("c1", "hi, what can you do?"),
                conv("c2", "run the shell command git commit -m test"),
                Conversation {
                    id: ConversationId::from("c3"),
                    project_id: ProjectId::from("personal"),
                    title: "research-digest on AutoAge".into(),
                    working_dir: String::new(),
                    model: String::new(),
                    created_at: 0,
                    updated_at: 0,
                    worktree: Some(Worktree {
                        path: "/tmp/worktree-autoage".into(),
                        branch: "feat/autoage".into(),
                        base_ref: "main".into(),
                    }),
                },
                conv("c4", "Run research-digest on Microsoft Copilot Studio"),
            ],
            events: Mutex::new(Vec::new()),
            ..MockTransport::new()
        }) as Arc<dyn oxide_client::Transport>;
        let state = AppState::new(mock);
        let st = state.clone();
        use_side_effect(move || {
            st.bootstrap();
        });
        rect()
            .direction(Direction::Horizontal)
            .expanded()
            .background((5u8, 7u8, 11u8))
            .child(Sidebar { state: state.clone(), collapsed: false })
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .height(Size::fill())
                    .background((10u8, 13u8, 20u8)),
            )
            .into()
    }

    /// Renders the styled sidebar (P2: header + conversation rows + rail) to a PNG.
    /// Run with: cargo test -p oxide-freya --bin oxide-freya snapshot_sidebar_p2 -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_sidebar_p2() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_sidebar_p2_app, (900., 800.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 12);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-sidebar-p2.png");
    }
}
