//! Root shell: horizontal layout of three independently-navigable regions.
use std::sync::Arc;

use freya::prelude::*;
use oxide_client::{Transport, UdsTransport};
use oxide_ui::components::menu::OxideContextMenuViewer;

use crate::regions::{context::ContextRegion, main_region::MainRegion, sidebar::Sidebar};
use crate::state::{AppState, ConnState};

/// Root shell component — call from `fn app()`.
pub fn shell() -> impl IntoElement {
    use_init_theme(dark_theme);
    let transport: Arc<dyn Transport> = Arc::new(UdsTransport::new(UdsTransport::default_socket()));
    let state = AppState::new(transport);
    state.bootstrap();
    let conn = *state.connection.read();
    let mut size_class = state.size_class;

    let sc = *state.size_class.read();
    let force_rail = sc.is_compact_or_narrower();
    let sidebar_collapsed = *state.sidebar_collapsed.read() || force_rail;
    let context_collapsed = *state.context_collapsed.read() || force_rail;

    rect()
        .direction(Direction::Horizontal)
        .content(Content::Flex)
        .expanded()
        .background((5u8, 7u8, 11u8))
        // Invisible full-window probe: measures LOGICAL width → size_class signal.
        // Global-positioned overlay rect so it doesn't affect the horizontal layout.
        .child(
            rect()
                .layer(Layer::Overlay)
                .position(Position::new_global().left(0.0).top(0.0))
                .width(Size::fill())
                .height(Size::fill())
                .opacity(0.0_f32)
                .on_sized(move |e: Event<SizedEventData>| {
                    size_class.set_if_modified(crate::state::SizeClass::from_logical_width(e.area.size.width));
                }),
        )
        // Mount ContextMenuViewer once as the first child of the shell root.
        // It provides the global ContextMenu context (in ScopeId::ROOT) and
        // renders the floating overlay when open.  Being layout-neutral when
        // closed, it has no effect on the shell's horizontal flow.
        .child(OxideContextMenuViewer::new())
        .maybe_child(connection_banner(conn))
        .child(Sidebar { state: state.clone(), collapsed: sidebar_collapsed })
        .child(
            rect()
                .width(Size::flex(1.0))
                .height(Size::fill())
                .child(MainRegion { state: state.clone() }),
        )
        .child(ContextRegion { state: state.clone(), collapsed: context_collapsed })
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
                .child(Sidebar { state: state.clone(), collapsed: *state.sidebar_collapsed.read() })
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
                st.send("ping".to_string(), vec![]);
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
        // Bubble body is now SelectableText → emits a `paragraph` element.
        let ping_bubble = runner.find(|_, el| {
            Paragraph::try_downcast(el)
                .filter(|p| p.spans.iter().any(|s| s.text.contains("ping")))
        });
        assert!(
            ping_bubble.is_some(),
            "after send('ping'), MainRegion must render a user 'ping' bubble (apply_user path)"
        );

        // Assert the assistant "pong" reply is rendered (streamed final event path).
        let pong_bubble = runner.find(|_, el| {
            Paragraph::try_downcast(el)
                .filter(|p| p.spans.iter().any(|s| s.text.contains("pong")))
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
        let sc = *state.size_class.read();
        let force_rail = sc.is_compact_or_narrower();
        let sidebar_collapsed = *state.sidebar_collapsed.read() || force_rail;
        let context_collapsed = *state.context_collapsed.read() || force_rail;
        rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .expanded()
            .background((5u8, 7u8, 11u8))
            .child(Sidebar { state: state.clone(), collapsed: sidebar_collapsed })
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .height(Size::fill())
                    .child(MainRegion { state: state.clone() }),
            )
            .child(ContextRegion { state: state.clone(), collapsed: context_collapsed })
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

    // ── Snapshot: full shell – Compact layout ─────────────────────────────

    /// Harness identical to `snapshot_shell_app` except `size_class` is forced
    /// to `Compact` via an explicit `set()` so both side-panels collapse to rails
    /// (60 px each) regardless of the window size seen by the harness.
    fn snapshot_shell_compact_app() -> Element {
        use crate::regions::context::ContextRegion;
        use crate::state::SizeClass;
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
        let mut st = state.clone();
        use_side_effect(move || {
            st.bootstrap();
            st.open_conversation(ConversationId::from("c1"));
            // Force Compact: both side-panels collapse to 60 px rails.
            st.size_class.set(SizeClass::Compact);
        });
        let sc = *state.size_class.read();
        let force_rail = sc.is_compact_or_narrower();
        let sidebar_collapsed = *state.sidebar_collapsed.read() || force_rail;
        let context_collapsed = *state.context_collapsed.read() || force_rail;
        rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .expanded()
            .background((5u8, 7u8, 11u8))
            .child(Sidebar { state: state.clone(), collapsed: sidebar_collapsed })
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .height(Size::fill())
                    .child(MainRegion { state: state.clone() }),
            )
            .child(ContextRegion { state: state.clone(), collapsed: context_collapsed })
            .into()
    }

    /// Renders the shell in Compact mode (both side-panels at 60 px rails) to
    /// `/tmp/oxide-shell-compact.png` for visual review.
    /// Run with: cargo test -p oxide-freya snapshot_shell_compact -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_shell_compact() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_shell_compact_app, (1000., 800.).into(), |_| {}, 1.);
        runner.poll(Duration::from_millis(1), Duration::from_millis(400));
        runner.render_to_file("/tmp/oxide-shell-compact.png");
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
            .child(Sidebar { state: state.clone(), collapsed: *state.sidebar_collapsed.read() })
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

    // ── Snapshot: toolbar attachment strip (T2) ───────────────────────────────

    fn snapshot_toolbar_chips_app() -> Element {
        use oxide_ui::components::composer::{
            Toolbar, SendState, sample_attachment,
        };
        use oxide_ui::tokens::Theme;
        Toolbar::new(Theme::default())
            .attachments(vec![
                sample_attachment("repo").unwrap(),
                sample_attachment("image").unwrap(),
                sample_attachment("code").unwrap(),
            ])
            .send(SendState::Ready)
            .into()
    }

    /// Toolbar with 3 attachment chips beside the model pill, send button pinned right.
    /// Run with: cargo test -p oxide-freya snapshot_toolbar_chips -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_toolbar_chips() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_toolbar_chips_app, (760., 60.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 6);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-toolbar-chips.png");
    }

    fn snapshot_toolbar_overflow_app() -> Element {
        use oxide_ui::components::composer::{
            Toolbar, SendState, sample_attachment,
        };
        use oxide_ui::tokens::Theme;
        Toolbar::new(Theme::default())
            .attachments(vec![
                sample_attachment("repo").unwrap(),
                sample_attachment("image").unwrap(),
                sample_attachment("code").unwrap(),
                sample_attachment("upload").unwrap(),
                sample_attachment("paste").unwrap(),
                sample_attachment("camera").unwrap(),
                sample_attachment("repo").unwrap(),
                sample_attachment("image").unwrap(),
            ])
            .send(SendState::Ready)
            .into()
    }

    /// Toolbar with 8 chips — strip overflows horizontally, send button still visible.
    /// Run with: cargo test -p oxide-freya snapshot_toolbar_overflow -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_toolbar_overflow() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_toolbar_overflow_app, (760., 60.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 6);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-toolbar-overflow.png");
    }

    fn snapshot_chip_solo_app() -> Element {
        use oxide_ui::components::composer::{sample_attachment, AttachmentChip};
        use oxide_ui::tokens::Theme;
        let att = sample_attachment("repo").unwrap();
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(Color::from_rgb(5, 7, 11))
            .main_align(Alignment::Center)
            .cross_align(Alignment::Center)
            .child(
                AttachmentChip::new(att, Theme::default())
                    .compact(true),
            )
            .into()
    }

    /// A single compact chip rendered on dark background — confirms the filename
    /// "run_bridge.rs" is visibly painted.
    /// Run with: cargo test -p oxide-freya snapshot_chip_solo -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_chip_solo() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_chip_solo_app, (360., 90.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 12);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-chip-solo.png");
    }

    // ── Snapshot: AttachmentViewer info card (T3) ─────────────────────────────

    fn snapshot_viewer_infocard_app() -> Element {
        use oxide_ui::components::composer::{sample_attachment, AttachmentViewer};
        use oxide_ui::tokens::Theme;
        let th = Theme::default();
        let att = sample_attachment("repo").unwrap();
        rect()
            .background(th.bg_deep())
            .width(Size::fill())
            .height(Size::fill())
            .main_align(Alignment::Center)
            .cross_align(Alignment::Center)
            .child(
                AttachmentViewer::new(att, th)
                    .on_dismiss(|()| {}),
            )
            .into()
    }

    /// Info-card viewer for a File attachment on a dark backdrop.
    /// Run with: cargo test -p oxide-freya snapshot_viewer_infocard -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_viewer_infocard() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_viewer_infocard_app, (600., 400.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 12);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-viewer-infocard.png");
    }

    // ── Snapshot: image lightbox with real PNG bytes, dark theme (T4a) ──────────

    fn snapshot_viewer_lightbox_app() -> Element {
        use oxide_ui::components::composer::{
            attachment::{AttachData, AttachKind},
            clipboard::image_data_to_attachment,
            AttachmentViewer,
        };
        use oxide_ui::tokens::Theme;
        use_init_theme(dark_theme);
        let th = Theme::default();

        // 96×64 RGBA gradient: R varies across width, G varies across height.
        let width: usize = 96;
        let height: usize = 64;
        let mut rgba = Vec::with_capacity(width * height * 4);
        for row in 0..height {
            for col in 0..width {
                let r = ((col as f32 / (width - 1) as f32) * 255.0) as u8;
                let g = ((row as f32 / (height - 1) as f32) * 255.0) as u8;
                rgba.push(r);
                rgba.push(g);
                rgba.push(120u8);
                rgba.push(255u8);
            }
        }

        let mut att = image_data_to_attachment(width, height, &rgba)
            .expect("gradient image encodes to PNG");
        // Override the name so it reads as a recognizable test image in the caption.
        att.name = "gradient-test.png".into();
        att.kind = AttachKind::Image;
        // Confirm data came through as Image bytes (image_data_to_attachment sets this).
        debug_assert!(matches!(att.data, AttachData::Image(_)));

        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .main_align(Alignment::Center)
            .cross_align(Alignment::Center)
            .child(
                AttachmentViewer::new(att, th)
                    .on_dismiss(|()| {}),
            )
            .into()
    }

    /// Image lightbox with a real 96×64 RGBA gradient encoded as PNG, dark theme.
    /// Confirms: dark Popup, image renders, caption "gradient-test.png" legible.
    /// Run with:
    ///   LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya --bin oxide-freya snapshot_viewer_lightbox -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_viewer_lightbox() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_viewer_lightbox_app, (900., 640.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 12);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-viewer-lightbox.png");
    }

    // ── Snapshot: info-card on dark theme (T4b) ───────────────────────────────────

    fn snapshot_viewer_infocard_dark_app() -> Element {
        use oxide_ui::components::composer::{sample_attachment, AttachmentViewer};
        use oxide_ui::tokens::Theme;
        use_init_theme(dark_theme);
        let th = Theme::default();
        let att = sample_attachment("repo").unwrap();
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .main_align(Alignment::Center)
            .cross_align(Alignment::Center)
            .child(
                AttachmentViewer::new(att, th)
                    .on_dismiss(|()| {}),
            )
            .into()
    }

    /// Info-card viewer for a File attachment with dark theme properly initialized.
    /// The Popup card is dark and name/meta/type text is legible (white-on-dark).
    /// Run with:
    ///   LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya --bin oxide-freya snapshot_viewer_infocard_dark -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_viewer_infocard_dark() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_viewer_infocard_dark_app, (720., 400.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 12);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-viewer-infocard-dark.png");
    }

    // ── Snapshot: full composer with 2 seeded attachment chips (T5) ──────────────

    /// Renders the Toolbar seeded with 2 attachment chips on a dark card backdrop —
    /// confirms chips sit in the toolbar strip beside the model pill (NOT in a
    /// full-width row above the editor), send button is pinned right, and the dark
    /// backdrop makes the chips legible.
    ///
    /// Chips are seeded directly into the Toolbar builder (.attachments([…])) so
    /// the snapshot is deterministic regardless of freya_testing interaction timing.
    ///
    /// Run with:
    ///   LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya --bin oxide-freya snapshot_composer_full -- --ignored --nocapture
    fn snapshot_composer_full_app() -> Element {
        use oxide_ui::components::composer::{sample_attachment, Toolbar, SendState};
        use oxide_ui::tokens::Theme;
        use_init_theme(dark_theme);
        let th = Theme::default();
        rect()
            .direction(Direction::Vertical)
            .content(Content::Flex)
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .padding(Gaps::new_all(16.))
            // flex spacer pushes the toolbar to the bottom (real-app layout)
            .child(rect().width(Size::px(1.)).height(Size::flex(1.0)))
            // card wrapping the toolbar — NO Content::Flex, NO height:fill.
            // The card must hug the toolbar's natural height (same as the real
            // composer footer which uses a plain .child() that hugs content).
            // A fill/flex card causes the toolbar's attachment ScrollView to
            // expand vertically, floating chips to the top and making the
            // snapshot look like chips are in a separate row above the editor.
            .child(
                rect()
                    .direction(Direction::Vertical)
                    .width(Size::fill())
                    .corner_radius(CornerRadius::new_all(16.))
                    .background(th.bg_deep())
                    .border(Border::new().fill(th.surface_max()).width(1.))
                    .child(
                        Toolbar::new(th)
                            .attachments(vec![
                                sample_attachment("repo").unwrap(),
                                sample_attachment("image").unwrap(),
                            ])
                            .send(SendState::Ready),
                    ),
            )
            .into()
    }

    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_composer_full() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_composer_full_app, (760., 300.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 12);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-composer-full.png");
    }
    // ── Snapshot: TextInput with seeded text (Task 2) ────────────────────────

    fn snapshot_text_input_app() -> Element {
        use oxide_ui::components::TextInput;
        use oxide_ui::tokens::Theme;
        use_init_theme(dark_theme);
        let th = Theme::default();
        let v = use_state(|| "Hello, OxideMX".to_string());
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .main_align(Alignment::Center)
            .cross_align(Alignment::Center)
            .padding(Gaps::new_all(32.))
            .child(
                TextInput::new(v.into_writable(), th)
                    .placeholder("Search…"),
            )
            .into()
    }

    /// TextInput with seeded text "Hello, OxideMX" on dark backdrop.
    /// Confirms: themed surface border, cyan caret colour, readable text.
    /// Run with:
    ///   LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya --bin oxide-freya snapshot_text_input -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_text_input() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_text_input_app, (480., 120.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 8);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-textinput.png");
    }

    // ── Snapshot: editor_clipboard_menu on dark backdrop (Task 2) ────────────

    fn snapshot_text_input_menu_app() -> Element {
        use oxide_ui::components::{editor_clipboard_menu};
        use oxide_ui::tokens::Theme;
        use_init_theme(dark_theme);
        let th = Theme::default();
        let noop = EventHandler::from(|_: ()| {});
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .main_align(Alignment::Center)
            .cross_align(Alignment::Center)
            .child(
                editor_clipboard_menu(
                    th,
                    noop.clone(),
                    noop.clone(),
                    noop.clone(),
                    noop,
                ),
            )
            .into()
    }

    /// Renders the editor_clipboard_menu directly on a dark backdrop.
    /// Confirms: 4 items (Cut / Copy / Paste / Select All) are legible.
    /// Run with:
    ///   LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya --bin oxide-freya snapshot_text_input_menu -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_text_input_menu() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_text_input_menu_app, (240., 200.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 8);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-textinput-menu.png");
    }

    // ── Snapshot: migrated sidebar search box (Task 3) ───────────────────────

    fn snapshot_sidebar_search_app() -> Element {
        use oxide_ui::components::SidebarHeader;
        use_init_theme(dark_theme);
        let th = oxide_ui::tokens::Theme::default();
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .main_align(Alignment::Start)
            .child(SidebarHeader::new(
                vec![("personal".into(), "oxidemx-phase1".into())],
                "personal".into(),
            ).theme(th))
            .into()
    }

    /// Renders the SidebarHeader (with its migrated TextInput search box) on a dark
    /// backdrop so the placeholder text is visible.  Confirms theming is correct
    /// and the "Search…" placeholder appears.
    ///
    /// Run with:
    ///   LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya --bin oxide-freya snapshot_sidebar_search -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_sidebar_search() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_sidebar_search_app, (320., 160.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 8);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-sidebar-search.png");
    }

    // ── Snapshot: composer editor right-click menu (Task 4) ──────────────────

    /// Renders the `editor_clipboard_menu` over a dark backdrop with the
    /// `ComposerEditor`'s dark theme initialised.  The same menu appears when
    /// the user right-clicks the composer editor (`on_secondary_down`).
    ///
    /// Cut / Copy / Paste / Select All must be legible over the dark surface.
    ///
    /// Run with:
    ///   LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya --bin oxide-freya snapshot_composer_editor_menu -- --ignored
    fn snapshot_composer_editor_menu_app() -> Element {
        use oxide_ui::components::editor_clipboard_menu;
        use oxide_ui::tokens::Theme;
        use_init_theme(dark_theme);
        let th = Theme::default();
        let noop = EventHandler::from(|_: ()| {});
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .main_align(Alignment::Center)
            .cross_align(Alignment::Center)
            .child(
                editor_clipboard_menu(
                    th,
                    noop.clone(),
                    noop.clone(),
                    noop.clone(),
                    noop,
                ),
            )
            .into()
    }

    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_composer_editor_menu() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_composer_editor_menu_app, (240., 200.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 8);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-editor-menu.png");
    }

    // ── Snapshot: selectable bubble + Copy message menu (T5) ─────────────────

    fn snapshot_bubble_selectable_app() -> Element {
        use oxide_ui::components::Bubble;
        use oxide_ui::components::menu::copy_only_menu;
        use oxide_ui::tokens::Theme;
        use_init_theme(dark_theme);
        let th = Theme::default();
        let noop = EventHandler::from(|_: ()| {});

        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .direction(Direction::Vertical)
            .spacing(16.)
            .padding(Gaps::new_all(24.))
            // Mount the context-menu host so open_context_menu can find the context.
            .child(oxide_ui::components::menu::OxideContextMenuViewer::new())
            .child(
                Bubble::new(
                    "user".into(),
                    "What is the meaning of life?".into(),
                ),
            )
            .child(
                Bubble::new(
                    "assistant".into(),
                    "The answer is 42, of course. Right-click this bubble to copy it.".into(),
                ),
            )
            // Render the "Copy message" menu inline so the snapshot shows it open —
            // same pragmatic approach used in T4 (editor_clipboard_menu rendered directly).
            .child(copy_only_menu(th, "Copy message", noop))
            .into()
    }

    /// Renders two chat bubbles (user + assistant) on a dark background alongside
    /// an open "Copy message" context menu, to `/tmp/oxide-bubble-menu.png`.
    ///
    /// Verifies: body text legible, SelectableText paragraphs render correctly,
    /// and the copy_only_menu is mounted.
    ///
    /// Run with:
    ///   LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-freya --bin oxide-freya snapshot_bubble_selectable -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_bubble_selectable() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_bubble_selectable_app, (600., 400.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 8);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-bubble-menu.png");
    }

}
