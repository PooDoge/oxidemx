//! Center region: the active chat thread + prompt. Renders ONLY
//! transport-delivered content (Rule 1): committed turns + the live streaming
//! assistant bubble; never fabricated text.
use freya::prelude::*;
use oxide_ui::Theme;
use oxide_ui::components::{Bubble, PromptInput, ThreadHeader};

use crate::state::AppState;

#[derive(PartialEq, Clone)]
pub struct MainRegion {
    pub state: AppState,
}

impl Component for MainRegion {
    fn render(&self) -> impl IntoElement {
        let state = self.state.clone();
        let tx = state.transcript.read().clone();
        let convs = state.conversations.read().clone();
        let active = state.active.read().clone();
        let (title, wt) = active
            .as_ref()
            .and_then(|id| convs.iter().find(|c| &c.id == id))
            .map(|c| (c.title.clone(), None))
            .unwrap_or_else(|| ("OxideMX".to_string(), None));

        let mut thread = rect().direction(Direction::Vertical).spacing(14.0).width(Size::fill());
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
            .content(Content::Flex)
            .width(Size::fill())
            .height(Size::fill())
            .background(Theme::default().bg())
            .child(ThreadHeader::new(title, "idle".into(), wt))
            .child(
                rect()
                    .width(Size::fill())
                    .height(Size::flex(1.0))
                    .padding(Gaps::new(16., 18., 16., 18.))
                    .child(ScrollView::new().child(thread)),
            )
            .child(
                PromptInput::new(input.into_writable())
                    .on_submit(move |text| send_state.send(text)),
            )
    }
}

#[cfg(test)]
mod editor_snapshot_tests {
    use std::time::Duration;

    use freya::prelude::*;
    use freya_testing::TestingRunner;
    use oxide_ui::Theme;
    use oxide_ui::components::composer::{ComposerConfig, ComposerEditor};

    fn snapshot_editor_app() -> Element {
        // Seed multi-line content so the snapshot exercises real newlines, the
        // accent caret, and the auto-grown body.
        let value = use_state(|| {
            "Summarize the changes in this branch,\n\
             then open a PR against main.\n\
             Use the conventional-commit style."
                .to_string()
        });
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(Theme::default().bg_deep())
            .padding(Gaps::new_all(12.))
            .child(
                ComposerEditor::new(
                    value.into_writable(),
                    ComposerConfig::default(),
                    Theme::default(),
                )
                .send_on_enter(true),
            )
            .into()
    }

    /// Renders the multiline `ComposerEditor` with seeded text to a PNG.
    /// Run with:
    ///   cargo test -p oxide-freya --bin oxide-freya snapshot_editor_multiline -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_editor_multiline() {
        let (mut runner, _) =
            TestingRunner::new(snapshot_editor_app, (760., 240.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 12);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-composer-editor.png");
    }
}

#[cfg(test)]
mod composer_snapshot_tests {
    use std::time::Duration;

    use freya::prelude::*;
    use freya_testing::TestingRunner;
    use oxide_ui::Theme;
    use oxide_ui::components::Composer;
    use oxide_ui::components::composer::ComposerConfig;

    /// Empty composer at 760 px — collapsed state (no grip, no strip-chips beyond
    /// the empty gate, no attachments, send button Disabled).
    fn collapsed_app() -> Element {
        let value = use_state(String::new);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(Theme::default().bg_deep())
            .padding(Gaps::new_all(16.))
            .child(Composer::new(value.into_writable(), ComposerConfig::default()).theme(Theme::default()))
            .into()
    }

    /// Seeded composer: two lines of text (PredictionStrip + multiline editor +
    /// Ready send button). The full variant additionally drives interaction in the
    /// test body to add two attachments and open the provider menu.
    fn full_app() -> Element {
        let value = use_state(|| "Refactor the run_bridge module,\nthen add focused tests.".to_string());
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(Theme::default().bg_deep())
            .padding(Gaps::new_all(16.))
            .child(Composer::new(value.into_writable(), ComposerConfig::default()).theme(Theme::default()))
            .into()
    }

    /// Center of the lowest node whose label text contains `needle`. "Lowest"
    /// (max Y) disambiguates the toolbar's provider pill from the identical model
    /// name shown in the ActivityLine above the card.
    fn lowest_label_center(runner: &TestingRunner, needle: &str) -> Option<(f64, f64)> {
        let hits = runner.find_many(|node, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains(needle))
                .map(|_| {
                    let c = node.layout().visible_area().center();
                    (c.x as f64, c.y as f64)
                })
        });
        hits.into_iter().max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Collapsed empty composer → /tmp/oxide-composer-collapsed.png.
    /// Run with:
    ///   cargo test -p oxide-freya --bin oxide-freya snapshot_composer_collapsed -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_composer_collapsed() {
        let (mut runner, _) =
            TestingRunner::new(collapsed_app, (760., 320.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 12);
        runner.sync_and_update();
        runner.render_to_file("/tmp/oxide-composer-collapsed.png");
    }

    /// Full composer: 2 lines + attachment(s) + provider menu open →
    /// /tmp/oxide-composer-full.png.
    ///
    /// Attachments and the menu-open flag live in `Composer`'s internal state, so
    /// the test drives them through the real interaction path: open the attach
    /// menu and pick a source (each pick closes the menu), then open the provider
    /// menu. Menu rows are located by label center (no hard-coded coords); the
    /// label-less "+" attach button uses a fixed left-padding X.
    ///
    /// LIMITATION: the second attachment pick can miss — once the first chip's
    /// row appears, the toolbar shifts down and the blind attach-button click no
    /// longer always lands on the (now-relocated) button, so the menu may not
    /// reopen for the second source. The snapshot reliably exercises the open
    /// provider menu floating above the toolbar, the prediction strip, a multiline
    /// editor, at least one attachment chip, and the Ready send button — which is
    /// the visual surface the controller inspects. It renders without panic.
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn snapshot_composer_full() {
        let (mut runner, _) =
            TestingRunner::new(full_app, (760., 560.).into(), |_| {}, 1.);
        runner.poll_n(Duration::from_millis(5), 12);
        runner.sync_and_update();

        // Add two attachments via the attach menu (open → pick → menu closes).
        // The "+" attach button carries no label, so it can't be located by text.
        // It is the toolbar's leftmost child — a fixed 36px square at the card's
        // left padding (outer 16 + toolbar 12 + half of 36 ≈ 46 px). Its Y tracks
        // the toolbar row, which we read each iteration from the provider pill.
        const ATTACH_BTN_X: f64 = 46.0;
        for source_label in ["Upload file", "Code snippet"] {
            if let Some((_pill_x, pill_y)) = lowest_label_center(&runner, "Sonnet 4.6") {
                runner.click_cursor((ATTACH_BTN_X, pill_y));
                runner.poll_n(Duration::from_millis(5), 6);
                runner.sync_and_update();
            }
            // Pick the source row in the now-open attach menu.
            if let Some(center) = lowest_label_center(&runner, source_label) {
                runner.click_cursor(center);
                runner.poll_n(Duration::from_millis(5), 6);
                runner.sync_and_update();
            }
        }

        // Open the provider menu by clicking the provider pill.
        if let Some(center) = lowest_label_center(&runner, "Sonnet 4.6") {
            runner.click_cursor(center);
            runner.poll_n(Duration::from_millis(5), 8);
            runner.sync_and_update();
        }

        runner.render_to_file("/tmp/oxide-composer-full.png");
    }
}
