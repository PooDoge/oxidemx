//! Right region: a 60px direction rail (collapsed) or a segmented expanded panel.
mod run;
mod worktree;
mod settings;

use freya::animation::*;
use freya::prelude::*;
use oxide_ui::{Theme, components::RailButton};

use crate::state::{AppState, RightTab};

const CONTEXT_FULL_W: f32 = 348.0;
const CONTEXT_RAIL_W: f32 = 60.0;

/// (RightTab, rail icon glyph, label) — drives the rail nav buttons + tab header.
const TABS: [(RightTab, &str, &str); 3] = [
    (RightTab::Run, "\u{2023}", "Run"),
    (RightTab::Worktree, "\u{2325}", "Worktree"),
    (RightTab::Settings, "\u{2699}", ".oxide"),
];

#[derive(PartialEq, Clone)]
pub struct ContextRegion {
    pub state: AppState,
    /// Effective collapsed value (user signal OR compact size class), from `shell()`.
    pub collapsed: bool,
}

impl Component for ContextRegion {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let state = self.state.clone();
        let collapsed = self.collapsed;
        let active_tab = *state.right_tab.read();

        // Width tween 348 <-> 60, scoped to the effective collapsed value,
        // settling on mount (OnCreation::Finish) so the collapsed rail doesn't
        // momentarily render at full width.
        let is_collapsed = collapsed;
        let width_anim = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            conf.on_creation(OnCreation::Finish);
            let w = AnimNum::new(CONTEXT_FULL_W, CONTEXT_RAIL_W)
                .time(340)
                .ease(Ease::Out)
                .function(Function::Expo);
            if is_collapsed { w } else { w.into_reversed() }
        });
        let anim_w = width_anim.get().value();

        if collapsed {
            // Rail: nav buttons that expand + select a tab.
            let mut rail = rect()
                .direction(Direction::Vertical)
                .cross_align(Alignment::Center)
                .spacing(8.)
                .padding(Gaps::new_all(8.))
                .width(Size::px(anim_w))
                .height(Size::fill())
                .background(th.panel())
                .border(Border::new().fill(th.hairline()).width(1.));
            for (tab, glyph, _lbl) in TABS {
                let mut rt = state.right_tab;
                let mut coll = state.context_collapsed;
                rail = rail.child(
                    RailButton::new(glyph.to_string())
                        .on_press(move |_: Event<PressEventData>| {
                            rt.set(tab);
                            coll.set(false);
                        }),
                );
            }
            rail.into_element()
        } else {
            // Full panel: SegmentedButton header + ScrollView body + collapse toggle.
            let mut seg = SegmentedButton::new();
            for (tab, _glyph, lbl) in TABS {
                let mut rt = state.right_tab;
                let selected = tab == active_tab;
                seg = seg.child(
                    ButtonSegment::new()
                        .selected(selected)
                        .on_press(move |_: Event<PressEventData>| rt.set(tab))
                        .child(label().text(lbl)),
                );
            }
            let mut coll = state.context_collapsed;
            let tab_body: Element = match active_tab {
                RightTab::Run => run::RunTab { state: state.clone() }.into_element(),
                RightTab::Worktree => worktree::WorktreeTab { state: state.clone() }.into_element(),
                RightTab::Settings => settings::SettingsTab { state: state.clone() }.into_element(),
            };
            rect()
                .direction(Direction::Vertical)
                .width(Size::px(anim_w))
                .height(Size::fill())
                .background(th.panel())
                .border(Border::new().fill(th.hairline()).width(1.))
                .child(
                    rect()
                        .direction(Direction::Horizontal)
                        .cross_align(Alignment::Center)
                        .spacing(8.)
                        .padding(Gaps::new(10., 12., 10., 12.))
                        .width(Size::fill())
                        .child(rect().width(Size::flex(1.0)).child(seg))
                        .child(
                            RailButton::new("\u{00bb}".to_string())
                                .on_press(move |_: Event<PressEventData>| coll.set(true)),
                        ),
                )
                .child(ScrollView::new().child(tab_body))
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
    use oxide_client::dto::{Conversation, ConversationId, Project, ProjectId, Worktree};

    use crate::state::{AppState, ConnState, RightTab};
    use super::ContextRegion;

    fn make_state(collapsed: bool, tab: RightTab) -> AppState {
        let mock = Arc::new(MockTransport::new()) as Arc<dyn oxide_client::Transport>;
        let mut state = AppState::new(mock);
        state.context_collapsed.set(collapsed);
        state.right_tab.set(tab);
        state
    }

    /// `make_state` with seeded projects + conversations so tab bodies show real data.
    fn make_rich_state(tab: RightTab) -> AppState {
        let pid = ProjectId::from("personal");
        let cid = ConversationId::from("conv-1");
        let mock = Arc::new(MockTransport::new()) as Arc<dyn oxide_client::Transport>;
        let mut state = AppState::new(mock);
        state.projects.set(vec![Project {
            id: pid.clone(),
            name: "OxideMX".into(),
            default_working_dir: "/home/jim/oxidemx".into(),
            created_at: 0,
        }]);
        state.conversations.set(vec![Conversation {
            id: cid.clone(),
            project_id: pid.clone(),
            title: "Build the shell".into(),
            working_dir: "/home/jim/oxidemx".into(),
            model: "claude-sonnet-4-6".into(),
            created_at: 0,
            updated_at: 0,
            worktree: Some(Worktree {
                path: "/home/jim/oxidemx".into(),
                branch: "feat/shell".into(),
                base_ref: "main".into(),
            }),
        }]);
        state.context_collapsed.set(false);
        state.right_tab.set(tab);
        state.current_project.set(Some(pid));
        state.active.set(Some(cid));
        state.connection.set(ConnState::Connected);
        state
    }

    fn snapshot_rail_app() -> Element {
        use_init_theme(dark_theme);
        let th = oxide_ui::tokens::Theme::default();
        let state = make_state(true, RightTab::Run);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .child(ContextRegion { state, collapsed: true })
            .into()
    }

    fn snapshot_run_app() -> Element {
        use_init_theme(dark_theme);
        let th = oxide_ui::tokens::Theme::default();
        let state = make_rich_state(RightTab::Run);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .child(ContextRegion { state, collapsed: false })
            .into()
    }

    fn snapshot_worktree_app() -> Element {
        use_init_theme(dark_theme);
        let th = oxide_ui::tokens::Theme::default();
        let state = make_rich_state(RightTab::Worktree);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .child(ContextRegion { state, collapsed: false })
            .into()
    }

    fn snapshot_settings_app() -> Element {
        use_init_theme(dark_theme);
        let th = oxide_ui::tokens::Theme::default();
        let state = make_rich_state(RightTab::Settings);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .child(ContextRegion { state, collapsed: false })
            .into()
    }

    // ── Legacy direction-panel snapshots (kept for reference, non-functional post-rewrite) ─

    fn right_tab_run_app() -> Element {
        use_init_theme(dark_theme);
        let th = oxide_ui::tokens::Theme::default();
        let state = make_rich_state(RightTab::Run);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .child(super::run::RunTab { state })
            .into()
    }

    fn right_tab_worktree_app() -> Element {
        use_init_theme(dark_theme);
        let th = oxide_ui::tokens::Theme::default();
        let state = make_rich_state(RightTab::Worktree);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .child(super::worktree::WorktreeTab { state })
            .into()
    }

    fn right_tab_settings_app() -> Element {
        use_init_theme(dark_theme);
        let th = oxide_ui::tokens::Theme::default();
        let state = make_rich_state(RightTab::Settings);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .child(super::settings::SettingsTab { state })
            .into()
    }

    /// Renders the collapsed rail (3 tab icons) to a PNG.
    /// Run with: cargo test -p oxide-freya context -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_context_rail() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_rail_app, (60., 400.).into(), |_| {}, 1.);
        runner.poll(Duration::from_millis(1), Duration::from_millis(350));
        runner.render_to_file("/tmp/right-rail.png");
    }

    /// Renders the full panel with Run tab active.
    /// Run with: cargo test -p oxide-freya context -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_context_run() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_run_app, (348., 600.).into(), |_| {}, 1.);
        runner.poll(Duration::from_millis(1), Duration::from_millis(350));
        runner.render_to_file("/tmp/right-run.png");
    }

    /// Renders the full panel with Worktree tab active.
    /// Run with: cargo test -p oxide-freya context -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_context_worktree() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_worktree_app, (348., 600.).into(), |_| {}, 1.);
        runner.poll(Duration::from_millis(1), Duration::from_millis(350));
        runner.render_to_file("/tmp/right-worktree.png");
    }

    /// Renders the full panel with Settings tab active.
    /// Run with: cargo test -p oxide-freya context -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_context_settings() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_settings_app, (348., 600.).into(), |_| {}, 1.);
        runner.poll(Duration::from_millis(1), Duration::from_millis(350));
        runner.render_to_file("/tmp/right-settings.png");
    }

    /// Renders the Run tab body to a PNG.
    /// Run with: cargo test -p oxide-freya right_tab -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn right_tab_run() {
        let (mut runner, _) =
            TestingRunner::new(right_tab_run_app, (348., 600.).into(), |_| {}, 1.);
        runner.poll(Duration::from_millis(1), Duration::from_millis(200));
        runner.render_to_file("/tmp/right-tab-run.png");
    }

    /// Renders the Worktree tab body to a PNG.
    /// Run with: cargo test -p oxide-freya right_tab -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn right_tab_worktree() {
        let (mut runner, _) =
            TestingRunner::new(right_tab_worktree_app, (348., 600.).into(), |_| {}, 1.);
        runner.poll(Duration::from_millis(1), Duration::from_millis(200));
        runner.render_to_file("/tmp/right-tab-worktree.png");
    }

    /// Renders the Settings tab body to a PNG.
    /// Run with: cargo test -p oxide-freya right_tab -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn right_tab_settings() {
        let (mut runner, _) =
            TestingRunner::new(right_tab_settings_app, (348., 600.).into(), |_| {}, 1.);
        runner.poll(Duration::from_millis(1), Duration::from_millis(200));
        runner.render_to_file("/tmp/right-tab-settings.png");
    }

    // ── Snapshot: old direction rail (historical) ────────────────────────────

    fn snapshot_spec_expanded_app() -> Element {
        use_init_theme(dark_theme);
        let th = oxide_ui::tokens::Theme::default();
        let state = make_state(false, RightTab::Run);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(th.bg_deep())
            .child(ContextRegion { state, collapsed: false })
            .into()
    }

    /// Renders the expanded Run panel (segmented header + body stub) to a PNG.
    /// Run with: cargo test -p oxide-freya context -- --ignored --nocapture
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_context_spec_expanded() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_spec_expanded_app, (348., 600.).into(), |_| {}, 1.);
        runner.poll(Duration::from_millis(1), Duration::from_millis(350));
        runner.render_to_file("/tmp/oxide-context-spec-expanded.png");
    }
}
