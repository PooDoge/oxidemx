//! `OxideTooltip` — a hover tooltip modeled on Freya's `TooltipContainer`
//! (`freya-components/src/tooltip.rs`): hover-delay Timer + `Attached` placement +
//! a scale/opacity entrance animation. Adds a detailed-content variant, a
//! configurable offset, and (via `TooltipGroup`) "instant after the first".
use std::borrow::Cow;
use std::time::Duration;

use freya::animation::*;
use freya::prelude::*;

use crate::tokens::Theme;

pub use freya::prelude::AttachedPosition;

/// How long after the cursor leaves the last tooltip in a group before the
/// warm state is reset (i.e. the next hover incurs the full delay again).
const TOOLTIP_COOLDOWN: Duration = Duration::from_millis(400);

/// Shared warm/cold state provided by `TooltipGroup` via context.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct TooltipGroupState {
    pub warm: State<bool>,
    pub cold_task: State<Option<TaskHandle>>,
}

/// Pure delay rule: instant only when a group is present AND warm.
pub(crate) fn group_effective_delay_value(has_group: bool, warm: bool, base: Duration) -> Duration {
    if has_group && warm { Duration::ZERO } else { base }
}

/// Groups a set of `OxideTooltip`s so that after the first shows, moving to
/// another is instant; leaving the group for `TOOLTIP_COOLDOWN` resets it.
#[derive(Default, Clone, PartialEq)]
pub struct TooltipGroup {
    children: Vec<Element>,
    key: DiffKey,
}

impl TooltipGroup {
    pub fn new() -> Self { Self::default() }
}

impl KeyExt for TooltipGroup {
    fn write_key(&mut self) -> &mut DiffKey { &mut self.key }
}

impl ChildrenExt for TooltipGroup {
    fn get_children(&mut self) -> &mut Vec<Element> { &mut self.children }
}

impl Component for TooltipGroup {
    fn render(&self) -> impl IntoElement {
        use_provide_context(|| TooltipGroupState {
            warm: State::create(false),
            cold_task: State::create(None),
        });
        rect().children(self.children.clone())
    }
}

/// Tooltip body: a simple themed text label, or arbitrary rich content.
enum TooltipBody {
    Text(Cow<'static, str>),
    Detailed(Element),
}

impl Clone for TooltipBody {
    fn clone(&self) -> Self {
        match self {
            Self::Text(t) => Self::Text(t.clone()),
            Self::Detailed(e) => Self::Detailed(e.clone()),
        }
    }
}

#[derive(Clone)]
pub struct OxideTooltip {
    body: TooltipBody,
    position: AttachedPosition,
    offset: f32,
    delay: Duration,
    children: Vec<Element>,
    key: DiffKey,
}

impl OxideTooltip {
    pub fn text(text: impl Into<Cow<'static, str>>) -> Self {
        Self::with_body(TooltipBody::Text(text.into()))
    }
    pub fn detailed(content: impl IntoElement) -> Self {
        Self::with_body(TooltipBody::Detailed(content.into_element()))
    }
    fn with_body(body: TooltipBody) -> Self {
        Self {
            body,
            position: AttachedPosition::Bottom,
            offset: 0.0,
            delay: Duration::from_millis(250),
            children: vec![],
            key: DiffKey::None,
        }
    }
    pub fn placement(mut self, position: AttachedPosition) -> Self {
        self.position = position;
        self
    }
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

impl PartialEq for OxideTooltip {
    fn eq(&self, _: &Self) -> bool { false } // re-render on each render of the parent (content is owned)
}
impl KeyExt for OxideTooltip {
    fn write_key(&mut self) -> &mut DiffKey { &mut self.key }
}
impl ChildrenExt for OxideTooltip {
    fn get_children(&mut self) -> &mut Vec<Element> { &mut self.children }
}

/// Shadowed surface for the detailed-content variant.
pub(crate) fn tooltip_surface(th: Theme, child: Element) -> impl IntoElement {
    rect()
        .interactive(Interactive::No)
        .background(th.panel())
        .border(Border::new().fill(th.hairline_strong()).width(1.))
        .corner_radius(CornerRadius::new_all(8.))
        .padding(Gaps::new_all(8.))
        .child(child)
}

impl Component for OxideTooltip {
    fn render(&self) -> impl IntoElement {
        let th = Theme::default();
        let mut is_hovering = use_state(|| false);
        let mut delay_task = use_state::<Option<TaskHandle>>(|| None);

        let animation = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            conf.on_creation(OnCreation::Finish);
            let scale = AnimNum::new(0.9, 1.).time(150).ease(Ease::Out).function(Function::Expo);
            let opacity = AnimNum::new(0., 1.).time(150).ease(Ease::Out).function(Function::Expo);
            if is_hovering() { (scale, opacity) } else { (scale.into_reversed(), opacity.into_reversed()) }
        });
        let (scale, opacity) = animation.read().value();

        let group = use_try_consume::<TooltipGroupState>();
        let has_group = group.is_some();
        let warm = group.as_ref().map(|g| *g.warm.read()).unwrap_or(false);
        let effective_delay = group_effective_delay_value(has_group, warm, self.delay);

        let on_pointer_over = move |_| {
            if let Some(mut g) = group { if let Some(h) = g.cold_task.write().take() { h.cancel(); } }
            if let Some(handle) = delay_task.write().take() { handle.cancel(); }
            let mut group2 = group;
            let task = spawn(async move {
                async_io::Timer::after(effective_delay).await;
                is_hovering.set_if_modified(true);
                if let Some(g) = &mut group2 { g.warm.set_if_modified(true); }
            });
            delay_task.set(Some(task));
        };
        let on_pointer_out = move |_| {
            if let Some(handle) = delay_task.write().take() { handle.cancel(); }
            is_hovering.set_if_modified(false);
            if let Some(mut g) = group {
                let mut warm = g.warm;
                let task = spawn(async move {
                    async_io::Timer::after(TOOLTIP_COOLDOWN).await;
                    warm.set_if_modified(false);
                });
                g.cold_task.set(Some(task));
            }
        };

        let is_visible = opacity > 0.;
        let pad = match self.position {
            AttachedPosition::Top => Gaps::new(0., 0., 5. + self.offset, 0.),
            AttachedPosition::Bottom => Gaps::new(5. + self.offset, 0., 0., 0.),
            AttachedPosition::Left => Gaps::new(0., 5. + self.offset, 0., 0.),
            AttachedPosition::Right => Gaps::new(0., 0., 0., 5. + self.offset),
        };
        let body: Element = match &self.body {
            TooltipBody::Text(t) => rect()
                .interactive(Interactive::No)
                .padding(Gaps::new(4., 10., 4., 10.))
                .border(Border::new().fill(th.hairline_strong()).width(1.))
                .background(th.panel())
                .corner_radius(CornerRadius::new_all(8.))
                .child(label().max_lines(1).font_size(12.5).color(th.text()).text(t.clone()))
                .into_element(),
            TooltipBody::Detailed(e) => tooltip_surface(th, e.clone()).into_element(),
        };

        rect()
            .a11y_role(AccessibilityRole::Tooltip)
            .a11y_focusable(false)
            .on_pointer_over(on_pointer_over)
            .on_pointer_out(on_pointer_out)
            .child(
                Attached::new(rect().children(self.children.clone()))
                    .position(self.position)
                    .maybe_child(is_visible.then(|| {
                        rect().opacity(opacity).scale(scale).padding(pad).child(body)
                    })),
            )
    }
    fn render_key(&self) -> DiffKey { self.key.clone().or(self.default_key()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn warm_makes_delay_instant_else_base() {
        use std::time::Duration;
        let base = Duration::from_millis(1000);
        // No group → base delay.
        assert_eq!(super::group_effective_delay_value(false, true, base), base, "no group → base");
        // Group present but cold → base delay.
        assert_eq!(super::group_effective_delay_value(true, false, base), base, "cold group → base");
        // Group present + warm → instant.
        assert_eq!(super::group_effective_delay_value(true, true, base), Duration::ZERO, "warm group → 0");
    }

    /// Snapshot: renders the text tooltip body force-visible on a dark bg.
    /// Run with: LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui tooltip_snapshot_text -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn tooltip_snapshot_text() {
        let th = Theme::default();
        let text_body = rect()
            .interactive(Interactive::No)
            .padding(Gaps::new(4., 10., 4., 10.))
            .border(Border::new().fill(th.hairline_strong()).width(1.))
            .background(th.panel())
            .corner_radius(CornerRadius::new_all(8.))
            .child(label().max_lines(1).font_size(12.5).color(th.text()).text("Save file"))
            .into_element();
        let text_body_clone = text_body.clone();
        fn app(text_body: Element) -> impl IntoElement {
            use_init_theme(dark_theme);
            rect()
                .background(Theme::default().bg_deep())
                .padding(Gaps::new_all(24.))
                .child(text_body)
        }
        let (mut runner, _) =
            TestingRunner::new(move || app(text_body_clone.clone()), (240., 80.).into(), |_| {}, 1.);
        runner.sync_and_update();
        runner.render_to_file("/tmp/tooltip-text.png");
    }

    /// Snapshot: renders the detailed tooltip body force-visible on a dark bg.
    /// Run with: LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui tooltip_snapshot_detailed -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn tooltip_snapshot_detailed() {
        let th = Theme::default();
        let detail_content = rect()
            .direction(Direction::Vertical)
            .spacing(4.)
            .child(label().font_size(13.).color(th.text()).text("Keyboard shortcut"))
            .child(label().font_size(11.5).color(th.subtext()).text("Ctrl + S"))
            .into_element();
        let detail_surface = tooltip_surface(th, detail_content).into_element();
        let detail_surface_clone = detail_surface.clone();
        fn app(body: Element) -> impl IntoElement {
            use_init_theme(dark_theme);
            rect()
                .background(Theme::default().bg_deep())
                .padding(Gaps::new_all(24.))
                .child(body)
        }
        let (mut runner, _) =
            TestingRunner::new(move || app(detail_surface_clone.clone()), (280., 120.).into(), |_| {}, 1.);
        runner.sync_and_update();
        runner.render_to_file("/tmp/tooltip-detailed.png");
    }

    /// Snapshot: renders the text tooltip body attached at each placement (Top/Bottom/Left/Right).
    /// One PNG per placement: /tmp/tooltip-{top,bottom,left,right}.png
    /// Run with: LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui tooltip_snapshot_placements -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn tooltip_snapshot_placements() {
        fn make_body(th: Theme) -> Element {
            rect()
                .interactive(Interactive::No)
                .padding(Gaps::new(4., 10., 4., 10.))
                .border(Border::new().fill(th.hairline_strong()).width(1.))
                .background(th.panel())
                .corner_radius(CornerRadius::new_all(8.))
                .child(label().max_lines(1).font_size(12.5).color(th.text()).text("Save file"))
                .into_element()
        }

        fn make_trigger(th: Theme) -> impl IntoElement {
            rect()
                .width(Size::px(60.))
                .height(Size::px(28.))
                .background(th.surface())
                .corner_radius(CornerRadius::new_all(4.))
                .center()
                .child(label().font_size(11.).color(th.text()).text("Btn"))
        }

        let placements = [
            (AttachedPosition::Top, "/tmp/tooltip-top.png"),
            (AttachedPosition::Bottom, "/tmp/tooltip-bottom.png"),
            (AttachedPosition::Left, "/tmp/tooltip-left.png"),
            (AttachedPosition::Right, "/tmp/tooltip-right.png"),
        ];

        for (pos, path) in placements {
            let th = Theme::default();
            let body = make_body(th);
            fn app(pos: AttachedPosition, body: Element) -> impl IntoElement {
                use_init_theme(dark_theme);
                let th = Theme::default();
                rect()
                    .background(th.bg_deep())
                    .width(Size::px(240.))
                    .height(Size::px(120.))
                    .center()
                    .child(
                        Attached::new(make_trigger(th))
                            .position(pos)
                            .child(body),
                    )
            }
            let (mut runner, _) = TestingRunner::new(
                move || app(pos, body.clone()),
                (240., 120.).into(),
                |_| {},
                1.,
            );
            runner.sync_and_update();
            runner.poll(
                std::time::Duration::from_millis(16),
                std::time::Duration::from_millis(160),
            );
            runner.render_to_file(path);
        }
    }

    /// Snapshot: renders the detailed tooltip body attached Right of a trigger.
    /// Output: /tmp/tooltip-detailed-right.png
    /// Run with: LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test -p oxide-ui tooltip_snapshot_detailed_right -- --ignored
    #[test]
    #[ignore = "snapshot: writes PNG to /tmp for visual review"]
    fn tooltip_snapshot_detailed_right() {
        let th = Theme::default();
        let detail_content = rect()
            .direction(Direction::Vertical)
            .spacing(4.)
            .child(label().font_size(13.).color(th.text()).text("Keyboard shortcut"))
            .child(label().font_size(11.5).color(th.subtext()).text("Ctrl + S"))
            .into_element();
        let detail_surface = tooltip_surface(th, detail_content).into_element();
        fn app(body: Element) -> impl IntoElement {
            use_init_theme(dark_theme);
            let th = Theme::default();
            rect()
                .background(th.bg_deep())
                .width(Size::px(360.))
                .height(Size::px(120.))
                .center()
                .child(
                    Attached::new(
                        rect()
                            .width(Size::px(60.))
                            .height(Size::px(28.))
                            .background(th.surface())
                            .corner_radius(CornerRadius::new_all(4.))
                            .center()
                            .child(label().font_size(11.).color(th.text()).text("Btn")),
                    )
                    .position(AttachedPosition::Right)
                    .child(body),
                )
        }
        let (mut runner, _) = TestingRunner::new(
            move || app(detail_surface.clone()),
            (360., 120.).into(),
            |_| {},
            1.,
        );
        runner.sync_and_update();
        runner.poll(
            std::time::Duration::from_millis(16),
            std::time::Duration::from_millis(160),
        );
        runner.render_to_file("/tmp/tooltip-detailed-right.png");
    }
}
