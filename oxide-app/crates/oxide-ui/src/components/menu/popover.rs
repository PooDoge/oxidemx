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
//!
//! ## Entrance animation — per-open-mounted overlay subcomponent
//!
//! The overlay content is rendered by a private `PopoverOverlay` sub-component
//! that is only mounted while `open=true`. Because it is only mounted while the
//! popover is open, its hooks are created fresh on each open:
//! `use_animation(OnCreation::Run)` fires on every open → the 0→1 fade plays
//! every time. On close the sub-component unmounts; on re-open it remounts and
//! the animation restarts.
//!
//! The prior implementation hoisted `use_animation` into the always-mounted
//! `Popover` component body. Because `Popover` is always mounted (the anchor
//! always renders), the hook was created at the very first composer-mount — the
//! 120ms tween ran to 1.0 immediately, so by the time the user opened the menu
//! `fade_value` was already 1.0 and the fade was a no-op on every subsequent open.
//!
//! Do NOT revert to hoisting `use_animation` into `Popover::render` or using
//! `OnChange::Rerun` + `open()` in the factory — both defeat the per-open replay.
//!
//! ## Animation + measurement composition
//!
//! `PopoverOverlay` owns both gates:
//!   `final_opacity = gated_opacity * fade_value`
//!
//! - `gated_opacity`: 0.0 until content is measured (prevents unsized flash), then 1.0
//! - `fade_value`: 0.0→1.0 over 120ms on mount (`OnCreation::Run`)
//!
//! Both must be 1.0 for content to be visible.
//!
//! ## Auto-flip measurement
//!
//! `PopoverOverlay` reports its measured size to the parent via a `State` handle
//! stored as a field. `State<T>` is `Copy + PartialEq`, so it is a safe component
//! field. The parent's `content_size: State<Option<Size2D>>` is updated via this
//! handle and drives the flip decision exactly as before.
use freya::animation::*;
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

// ── Private overlay sub-component ─────────────────────────────────────────────

/// Private overlay sub-component, only mounted while the popover is open.
///
/// Owns `use_animation(OnCreation::Run)` — because this component is only
/// mounted while `open=true`, the hook is created fresh on each open and the
/// 120ms entrance fade plays on every open. On close it unmounts; on re-open
/// it remounts and replays.
///
/// `parent_size` is the parent's `State<Option<Size2D>>` handle. `State<T>` is
/// `Copy + PartialEq`, so it is a safe component field and can be captured by
/// `FnMut` closures for mutation.
#[derive(PartialEq, Clone)]
struct PopoverOverlay {
    content:     Element,
    parent_size: State<Option<Size2D>>,
    offset_top:  f32,
    offset_left: f32,
    /// The parent has computed an edge-aware position (anchor + content measured).
    positioned:  bool,
}

impl PopoverOverlay {
    fn new(
        content: Element,
        parent_size: State<Option<Size2D>>,
        offset_top: f32,
        offset_left: f32,
        positioned: bool,
    ) -> Self {
        Self { content, parent_size, offset_top, offset_left, positioned }
    }
}

impl Component for PopoverOverlay {
    fn render(&self) -> impl IntoElement {
        // Entrance fade: `OnCreation::Run` fires on mount.
        // Because `PopoverOverlay` is only mounted while `open=true`, this hook
        // is created fresh on every open → the fade plays every time.
        let entrance_anim = use_animation(|conf| {
            conf.on_creation(OnCreation::Run);
            AnimNum::new(0.0_f32, 1.0_f32)
                .time(120)
                .ease(Ease::Out)
                .function(Function::Quart)
        });
        let fade_value = entrance_anim.get().value();
        // Hidden until the parent has an edge-aware position (which needs this
        // overlay's measured size) — avoids a flash at the un-positioned origin.
        let opacity = if self.positioned { fade_value } else { 0.0_f32 };

        let mut parent_size = self.parent_size;
        let content         = self.content.clone();

        rect()
            .layer(Layer::Overlay)
            .position(Position::new_absolute().top(self.offset_top).left(self.offset_left))
            .opacity(opacity)
            .on_sized(move |e: Event<SizedEventData>| {
                parent_size.set_if_modified(Some(e.area.size));
            })
            .child(content)
    }
}

// ── Public component ──────────────────────────────────────────────────────────

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
    anchor:    Element,
    open:      bool,
    placement: Placement,
    content:   Option<Element>,
}

impl Popover {
    pub fn new(anchor: impl IntoElement) -> Self {
        Self {
            anchor:    anchor.into_element(),
            open:      false,
            placement: Placement::default(),
            content:   None,
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
        let open      = self.open;
        let placement = self.placement;

        // Measured areas drive the auto-flip decision.
        let mut anchor_area:  State<Option<Area>>   = use_state(|| None);
        let content_size: State<Option<Size2D>>     = use_state(|| None);

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

        // ── Edge-aware absolute offset of the overlay (relative to the anchor) ──
        // Replaces Freya's `Attached`, which centers the overlay on the anchor with
        // no window-edge awareness — so a near-edge anchor (e.g. the `+` button with
        // the sidebar collapsed) pushed the menu off-screen and clipped it.
        //
        // Horizontal: LEFT-ALIGN the menu to the anchor's left edge, then CLAMP it
        // inside the window `[EDGE_MARGIN, root_w - menu_w - EDGE_MARGIN]` so it
        // shifts in at either edge. Vertical: above/below per the flip
        // (`-content_height` above the anchor top, or `anchor_height` below it).
        // Offsets are relative to the anchor's top-left (the overlay is an absolutely
        // positioned sibling of the anchor inside the wrapper rect, like `Attached`).
        const EDGE_MARGIN: f32 = 8.0;
        let (offset_top, offset_left, positioned) = match (anchor_area(), content_size()) {
            (Some(a), Some(c)) => {
                let root_w = Platform::get().root_size.peek().width;
                let max_x = (root_w - c.width - EDGE_MARGIN).max(EDGE_MARGIN);
                let clamped_x = a.min_x().clamp(EDGE_MARGIN, max_x);
                let off_left = clamped_x - a.min_x();
                let off_top = match effective_placement {
                    Placement::Above => -c.height,
                    Placement::Below => a.height(),
                };
                (off_top, off_left, true)
            }
            // Not yet measured — keep the overlay hidden (positioned = false).
            _ => (0.0, 0.0, false),
        };

        let content_el = self.content.clone();

        // Overlay sub-component — only mounted while open.
        // `PopoverOverlay` owns the animation hook, so it fires fresh on every open.
        // Pass `content_size` so it reports its measured size back for positioning;
        // `State<T>` is `Copy + PartialEq` and safe as a component field.
        let overlay: Option<Element> = (open && content_el.is_some()).then(|| {
            PopoverOverlay::new(
                content_el.clone().unwrap(),
                content_size,
                offset_top,
                offset_left,
                positioned,
            )
            .into_element()
        });

        // The anchor wrapper — measures the anchor area (window coords) for the
        // flip + edge-clamp math. The overlay is an absolutely positioned sibling,
        // so it does not affect the anchor's layout.
        let anchor_inner = rect()
            .on_sized(move |e: Event<SizedEventData>| {
                anchor_area.set_if_modified(Some(e.area));
            })
            .child(self.anchor.clone());

        rect().child(anchor_inner).maybe_child(overlay)
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

    /// Proves the per-open-mount idiom: open → close → open must still show content.
    /// Guards against hook state persisting across mount/unmount cycles.
    #[test]
    fn popover_content_visible_after_reopen() {
        use freya_testing::TestingRunner;

        fn app() -> Element {
            let mut open = use_state(|| false);
            let trigger = rect()
                .width(Size::px(80.))
                .height(Size::px(32.))
                .on_press(move |_: Event<PressEventData>| open.toggle())
                .child(label().text("anchor").font_size(12.))
                .into_element();

            Popover::new(trigger)
                .open(open())
                .content(label().text("MENU").into_element())
                .into_element()
        }

        let (mut t, _) = TestingRunner::new(app, (400., 320.).into(), |_| {}, 1.);
        t.poll(
            std::time::Duration::from_millis(5),
            std::time::Duration::from_millis(40),
        );
        t.sync_and_update();

        // First open — re-find anchor center each time in case layout shifts.
        let center = t.find(|node, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("anchor"))
                .map(|_| {
                    let c = node.layout().visible_area().center();
                    (c.x as f64, c.y as f64)
                })
        });
        assert!(center.is_some(), "anchor should render");
        t.click_cursor(center.unwrap());
        t.poll(
            std::time::Duration::from_millis(10),
            std::time::Duration::from_millis(200),
        );
        t.sync_and_update();
        let after_first_open = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("MENU"))
        });
        assert!(after_first_open.is_some(), "content should render after first open");

        // Close — re-find anchor center because layout may have shifted.
        let center = t.find(|node, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("anchor"))
                .map(|_| {
                    let c = node.layout().visible_area().center();
                    (c.x as f64, c.y as f64)
                })
        }).unwrap();
        t.click_cursor(center);
        t.poll(
            std::time::Duration::from_millis(5),
            std::time::Duration::from_millis(40),
        );
        t.sync_and_update();
        let after_close = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("MENU"))
        });
        assert!(after_close.is_none(), "content should NOT render after close");

        // Re-open: proves per-open-mount remount works.
        let center = t.find(|node, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("anchor"))
                .map(|_| {
                    let c = node.layout().visible_area().center();
                    (c.x as f64, c.y as f64)
                })
        }).unwrap();
        t.click_cursor(center);
        t.poll(
            std::time::Duration::from_millis(10),
            std::time::Duration::from_millis(200),
        );
        t.sync_and_update();
        let after_reopen = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("MENU"))
        });
        assert!(
            after_reopen.is_some(),
            "content should render after re-open (per-open-mount remount)"
        );
    }
}
