//! `Popover` — anchors floating `content` to a trigger `anchor`.
//!
//! Built on Freya's [`Attached`] (positions an overlay above/below the inner
//! element by measuring both). The overlay subtree is only built while `open`,
//! and is opacity-gated until measured so it never flashes unsized.
//!
//! Auto-flip: when `placement == Below` but the measured content height exceeds
//! the space below the anchor (`Platform::get().root_size.height − anchor.max_y`)
//! and fits above, the content renders Above instead (and vice-versa). This
//! mirrors `Select`'s flip math.
//!
//! ## Dismissal — owned by the menu content, not the Popover
//!
//! `Popover` is a pure anchor wrapper: it has NO dismissal logic.
//! Earlier revisions tried to dismiss here — first a full-window backdrop on
//! `Layer::Relative(-1)` (wrong for a deeply nested anchor: the backdrop painted
//! BELOW higher-layer app content so outside presses never reached it), then a
//! Select-style `on_global_pointer_press` on the always-present Popover root.
//! The latter self-closed on the OPENING click: the root global handler exists
//! at the moment the trigger toggles `open`, so the very press that opens the
//! menu also fires the global dismiss in the same cycle.
//!
//! The correct model — proven by Freya's own `Menu` — puts the dismiss handler
//! (`on_global_pointer_press` + Escape) on the `Menu` node, which is
//! `Layer::Overlay` and only mounted while the menu is open. Because that
//! handler does NOT exist at opening-click time, the opening click cannot
//! self-close it; only a subsequent outside press dismisses. Our menus already
//! wrap Freya's `Menu` (inside `MenuSurface`), so dismissal threads through
//! `MenuSurface::on_close → Menu::on_close`. The Popover therefore only renders
//! the anchor + (when open) the animated content; the caller wires dismissal via
//! the menu's `on_close`.
use freya::prelude::*;

/// Where the popover content is placed relative to its anchor.
#[derive(PartialEq, Clone, Copy, Debug, Default)]
pub enum Placement {
    /// Content sits above the anchor.
    Above,
    /// Content sits below the anchor.
    #[default]
    Below,
}

/// Anchors floating `content` to a trigger `anchor`.
///
/// Pure anchor wrapper — dismissal is owned by the menu content (Freya
/// `Menu`'s `on_close`, threaded via `MenuSurface::on_close`).
///
/// Builder usage:
/// ```ignore
/// Popover::new(trigger_element)
///     .open(is_open)
///     .placement(Placement::Below)
///     .content(MenuSurface::new(theme).on_close(move |_| set_open(false)).child(body))
/// ```
#[derive(PartialEq)]
pub struct Popover {
    anchor: Element,
    open: bool,
    placement: Placement,
    content: Option<Element>,
}

impl Popover {
    pub fn new(anchor: impl IntoElement) -> Self {
        Self {
            anchor: anchor.into_element(),
            open: false,
            placement: Placement::default(),
            content: None,
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_element());
        self
    }
}

impl Component for Popover {
    fn render(&self) -> impl IntoElement {
        let open = self.open;
        let placement = self.placement;

        // Measured areas drive both the opacity gate and the auto-flip decision.
        let mut anchor_area: State<Option<Area>> = use_state(|| None);
        let mut content_size: State<Option<Size2D>> = use_state(|| None);

        let show_overlay = open;

        // Auto-flip: resolve the effective placement from measured geometry.
        let effective_placement = match (anchor_area(), content_size()) {
            (Some(anchor), Some(content)) => {
                let root_height = Platform::get().root_size.peek().height;
                let space_below = root_height - anchor.max_y();
                let space_above = anchor.min_y();
                match placement {
                    Placement::Below => {
                        if content.height > space_below && content.height <= space_above {
                            Placement::Above
                        } else {
                            Placement::Below
                        }
                    }
                    Placement::Above => {
                        if content.height > space_above && content.height <= space_below {
                            Placement::Below
                        } else {
                            Placement::Above
                        }
                    }
                }
            }
            // Not yet measured — fall back to the requested placement.
            _ => placement,
        };

        // Opacity-gate until the content has been measured (no unsized flash).
        // Show at full opacity once the content has a measured size.
        let measured = content_size().is_some();
        let gated_opacity = if measured { 1.0_f32 } else { 0.0_f32 };

        let attached_position = match effective_placement {
            Placement::Above => AttachedPosition::Top,
            Placement::Below => AttachedPosition::Bottom,
        };

        let content_el = self.content.clone();

        // The overlay content rect: opacity-gated until measured, then fully opaque.
        // Dismissal lives INSIDE this subtree (Freya `Menu`'s `on_close`, via
        // `MenuSurface`), so it only exists while the menu is open and the
        // opening click can never reach it to self-close.
        let overlay: Option<Element> = (show_overlay && content_el.is_some()).then(|| {
            let content = content_el.clone().unwrap();
            rect()
                .layer(Layer::Overlay)
                .opacity(gated_opacity)
                .on_sized(move |e: Event<SizedEventData>| {
                    content_size.set_if_modified(Some(e.area.size));
                })
                .child(content)
                .into_element()
        });

        // The anchor wrapper — measures the anchor area for the auto-flip math.
        let anchor_inner = rect()
            .on_sized(move |e: Event<SizedEventData>| {
                anchor_area.set_if_modified(Some(e.area));
            })
            .child(self.anchor.clone());

        Attached::new(anchor_inner)
            .position(attached_position)
            .maybe_child(overlay)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    /// When closed, the anchor renders but the content does NOT.
    #[test]
    fn popover_hidden_when_closed() {
        fn app() -> impl IntoElement {
            Popover::new(label().text("anchor").into_element())
                .open(false)
                .content(label().text("MENU").into_element())
        }
        let mut t = launch_test(app);
        t.sync_and_update();

        let anchor = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("anchor"))
        });
        assert!(anchor.is_some(), "anchor label should render when closed");

        let menu = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("MENU"))
        });
        assert!(menu.is_none(), "content should NOT render when closed");
    }

    /// When open, the content renders alongside the anchor.
    #[test]
    fn popover_shows_content_when_open() {
        fn app() -> impl IntoElement {
            Popover::new(label().text("anchor").into_element())
                .open(true)
                .content(label().text("MENU").into_element())
        }
        let mut t = launch_test(app);
        t.sync_and_update();

        let menu = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("MENU"))
        });
        assert!(menu.is_some(), "content should render when open");
    }

    /// Nested-context open: mount a `Popover` whose anchor is buried a few rects
    /// deep (mimicking composer → card → toolbar row), drive a `click_cursor` on
    /// the anchor, poll for measurement, and assert the content renders. Proves
    /// opening works in a deep layout (the original regression).
    ///
    /// Drives the OPEN path through the caller's anchor `on_press` (which toggles
    /// a `use_state`), exercising the same wiring the toolbar uses.
    #[test]
    fn popover_opens_in_nested_context() {
        use freya_testing::TestingRunner;

        fn app() -> Element {
            let mut open = use_state(|| false);
            // Anchor buried under several wrapper rects, near the window bottom,
            // mimicking the real toolbar's depth.
            let trigger = rect()
                .width(Size::px(80.))
                .height(Size::px(32.))
                .background(Color::from_rgb(80, 80, 120))
                .on_press(move |_: Event<PressEventData>| open.toggle())
                .child(label().text("anchor").font_size(12.))
                .into_element();

            let popover = Popover::new(trigger)
                .open(open())
                .placement(Placement::Above)
                .content(label().text("MENU").into_element())
                .into_element();

            // Deep nesting + bottom anchoring (column pushes the row down).
            rect()
                .expanded()
                .direction(Direction::Vertical)
                .main_align(Alignment::End)
                .child(
                    rect().padding(Gaps::new_all(8.)).child(
                        rect().padding(Gaps::new_all(8.)).child(
                            rect().direction(Direction::Horizontal).child(popover),
                        ),
                    ),
                )
                .into_element()
        }

        let (mut t, _) = TestingRunner::new(app, (400., 320.).into(), |_| {}, 1.);
        t.poll(
            std::time::Duration::from_millis(5),
            std::time::Duration::from_millis(40),
        );
        t.sync_and_update();

        // Find the anchor's center and click it to open.
        let center = t.find(|node, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("anchor"))
                .map(|_| {
                    let c = node.layout().visible_area().center();
                    (c.x as f64, c.y as f64)
                })
        });
        assert!(center.is_some(), "anchor should render");

        let center = center.unwrap();
        t.click_cursor(center);
        // Poll a few frames so the content mounts and measures (no animation).
        t.poll(
            std::time::Duration::from_millis(10),
            std::time::Duration::from_millis(300),
        );
        t.sync_and_update();

        let menu = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("MENU"))
        });
        assert!(
            menu.is_some(),
            "content should render after a click-driven open in a nested context"
        );
    }
}
