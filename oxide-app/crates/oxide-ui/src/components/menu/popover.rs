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
//! ## Animation — persistent hook in `Popover::render` (Select-style)
//!
//! A `use_animation` hook lives in `Popover::render` (always-mounted), using
//! **`OnCreation::Finish`** + **`OnChange::Rerun`**:
//!
//! - `OnCreation::Finish` — jumps the animation to its finished state at mount
//!   (opacity=1 when open, opacity=0 when closed). The prior attempt used
//!   `OnCreation::Run`, which ran the 0→1 tween immediately at composer-mount
//!   — by the time the user first opened the menu the value was already 1.0 and
//!   the entrance fade was a no-op on every subsequent open.
//! - `OnChange::Rerun` — re-runs the factory when the subscribed signal changes.
//!   Because a plain `bool` prop cannot be subscribed, `open` is mirrored into a
//!   local `use_state` signal (`open_sig.set_if_modified(open)`) and the factory
//!   reads `open_sig()` to subscribe.
//! - When `open_sig()` is true the factory returns the forward tween (0.9→1 scale,
//!   0→1 opacity, slide→0). When false it returns `into_reversed()` versions —
//!   the exit plays the same curve in reverse. Exactly the pattern used in Freya's
//!   own `Select` component.
//!
//! The overlay mount gate is `(open || opacity > 0.0)` — content stays mounted
//! during the exit tween and unmounts only once `opacity` reaches 0.0.
//!
//! ## Animation + measurement composition
//!
//! `PopoverOverlay` receives `scale`, `opacity`, and `slide` from the parent and
//! applies them directly (no inner animation hook):
//!
//!   `final_opacity = if positioned { opacity } else { 0.0 }`
//!
//! - `positioned`: 0.0 until content is measured (prevents unsized flash)
//! - `opacity`: 0.0→1.0 on open, 1.0→0.0 on close (from parent hook)
//! - `scale`/`slide`: applied directly as `.scale()` / `.offset_y()`
//!
//! The `on_sized` handler records content size while `!positioned` (the FIRST
//! measurement — required to ever become positioned/visible) OR while visible
//! (`final_opacity > 0.0`); it skips only once positioned AND fully faded, so a
//! fading-out overlay does not needlessly re-measure.
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

fn slide_from(p: Placement) -> f32 {
    match p {
        Placement::Below => -8.0,
        Placement::Above => 8.0,
    }
}

// ── Private overlay sub-component ─────────────────────────────────────────────

/// Private overlay sub-component. Renders the floating content with animation
/// values driven by the parent's persistent `use_animation` hook.
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
    scale:       f32,
    opacity:     f32,
    slide:       f32,
}

impl PopoverOverlay {
    #[allow(clippy::too_many_arguments)]
    fn new(
        content: Element,
        parent_size: State<Option<Size2D>>,
        offset_top: f32,
        offset_left: f32,
        positioned: bool,
        scale: f32,
        opacity: f32,
        slide: f32,
    ) -> Self {
        Self { content, parent_size, offset_top, offset_left, positioned, scale, opacity, slide }
    }
}

impl Component for PopoverOverlay {
    fn render(&self) -> impl IntoElement {
        // Hidden until the parent has an edge-aware position (which needs this
        // overlay's measured size) — avoids a flash at the un-positioned origin.
        // Also hidden while the exit tween is playing and opacity reaches 0.
        let final_opacity = if self.positioned { self.opacity } else { 0.0_f32 };
        let positioned    = self.positioned;

        let mut parent_size = self.parent_size;
        let content         = self.content.clone();

        rect()
            .layer(Layer::Overlay)
            .position(Position::new_absolute().top(self.offset_top).left(self.offset_left))
            .scale(self.scale)
            .offset_y(self.slide)
            .opacity(final_opacity)
            .on_sized(move |e: Event<SizedEventData>| {
                // Record size on the FIRST open (before `positioned`), else the
                // chicken-and-egg deadlocks the menu invisible: `final_opacity` needs
                // `positioned`, and `positioned` needs THIS measurement. Once positioned,
                // keep measuring while visible; skip only when positioned AND fully faded.
                if !positioned || final_opacity > 0.0 {
                    parent_size.set_if_modified(Some(e.area.size));
                }
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
        let mut content_size: State<Option<Size2D>> = use_state(|| None);

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

        // Mirror the plain-bool prop into a signal so `OnChange::Rerun` can subscribe.
        let mut open_sig = use_state(|| open);
        open_sig.set_if_modified(open);

        // Persistent animation hook — Select-style (OnCreation::Finish + OnChange::Rerun).
        // `open_sig()` inside the factory subscribes to the signal; when `open_sig` changes,
        // `OnChange::Rerun` re-runs the factory with the new direction.
        let animation = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            conf.on_creation(OnCreation::Finish);
            let scale = AnimNum::new(0.9_f32, 1.)
                .time(125)
                .ease(Ease::Out)
                .function(Function::Quart);
            let opacity = AnimNum::new(0.0_f32, 1.)
                .time(125)
                .ease(Ease::Out)
                .function(Function::Quart);
            let slide = AnimNum::new(slide_from(effective_placement), 0.)
                .time(125)
                .ease(Ease::Out)
                .function(Function::Quart);
            if open_sig() {
                (scale, opacity, slide)
            } else {
                (scale.into_reversed(), opacity.into_reversed(), slide.into_reversed())
            }
        });
        let (scale, opacity, slide) = animation.read().value();

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

        // Overlay — mounted while open OR while the exit tween is still playing
        // (opacity > 0.0). Unmounts only once opacity reaches 0.0.
        let overlay: Option<Element> = ((open || opacity > 0.0) && content_el.is_some()).then(|| {
            PopoverOverlay::new(
                content_el.clone().unwrap(),
                content_size,
                offset_top,
                offset_left,
                positioned,
                scale,
                opacity,
                slide,
            )
            .into_element()
        });

        // Clear measured content size once fully closed so re-open re-measures.
        if !open && opacity == 0.0 && content_size.peek().is_some() {
            content_size.set(None);
        }

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
        // Poll past the 125ms exit tween so the overlay fully unmounts.
        let center = t.find(|node, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("anchor"))
                .map(|_| {
                    let c = node.layout().visible_area().center();
                    (c.x as f64, c.y as f64)
                })
        }).unwrap();
        t.click_cursor(center);
        t.poll(std::time::Duration::from_millis(10), std::time::Duration::from_millis(260));
        t.sync_and_update();
        let after_close = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("MENU"))
        });
        assert!(after_close.is_none(), "content should NOT render after close + exit animation");

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
            "content should render after re-open (persistent hook reverse tween)"
        );
    }

    /// The exit animation must keep the overlay mounted briefly after close
    /// (proves the persistent-hook reverse tween plays, not an instant unmount).
    #[test]
    fn popover_exit_animation_keeps_content_mounted_briefly() {
        use freya_testing::TestingRunner;
        fn app() -> Element {
            let mut open = use_state(|| false);
            let trigger = rect().width(Size::px(80.)).height(Size::px(32.))
                .on_press(move |_: Event<PressEventData>| open.toggle())
                .child(label().text("anchor").font_size(12.)).into_element();
            Popover::new(trigger).open(open()).content(label().text("MENU").into_element()).into_element()
        }
        let (mut t, _) = TestingRunner::new(app, (400., 320.).into(), |_| {}, 1.);
        t.poll(std::time::Duration::from_millis(5), std::time::Duration::from_millis(40));
        t.sync_and_update();
        let center = t.find(|node, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("anchor"))
            .map(|_| { let c = node.layout().visible_area().center(); (c.x as f64, c.y as f64) })).unwrap();
        t.click_cursor(center); // open
        t.poll(std::time::Duration::from_millis(10), std::time::Duration::from_millis(200));
        t.sync_and_update();
        let center = t.find(|node, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("anchor"))
            .map(|_| { let c = node.layout().visible_area().center(); (c.x as f64, c.y as f64) })).unwrap();
        t.click_cursor(center); // close
        // Only a SHORT poll — less than the 125ms exit tween. Content must STILL be mounted.
        t.poll(std::time::Duration::from_millis(5), std::time::Duration::from_millis(30));
        t.sync_and_update();
        let mid_close = t.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref().contains("MENU")));
        assert!(mid_close.is_some(), "content must remain mounted during the exit animation");
    }

    /// After an anchor click opens the Popover, the overlay rect's *rendered* opacity
    /// must be significantly > 0.  This test catches the measurement-deadlock bug where
    /// `on_sized` was only called when `final_opacity > 0.0`, so the overlay was
    /// permanently invisible: opacity=0, unmeasured, unpositioned — but still mounted
    /// and clickable.
    ///
    /// The assertion reads `RectElement.effect.opacity` — the opacity actually applied
    /// to the overlay rect — NOT just node presence.  That value is `0.0` under the
    /// bug and `≈1.0` after the fix (post-tween).
    ///
    /// `TestingNode::is_visible()` only checks clip-region intersection and does NOT
    /// check opacity; it would PASS under the bug.  `Rect::try_downcast` + the
    /// `RectElement.effect.opacity` field is the only freya_testing API that directly
    /// exposes the opacity value set on the overlay rect.
    #[test]
    fn popover_content_is_visible_after_open() {
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

        // Click the anchor to open.
        let center = t
            .find(|node, el| {
                Label::try_downcast(el)
                    .filter(|l| l.text.as_ref().contains("anchor"))
                    .map(|_| {
                        let c = node.layout().visible_area().center();
                        (c.x as f64, c.y as f64)
                    })
            })
            .expect("anchor should render before open");
        t.click_cursor(center);

        // Poll well past the 125ms entrance tween so the animation reaches ~1.0.
        t.poll(
            std::time::Duration::from_millis(10),
            std::time::Duration::from_millis(260),
        );
        t.sync_and_update();

        // Sanity: MENU label must be mounted.
        let menu = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("MENU"))
        });
        assert!(menu.is_some(), "MENU label should be mounted after open");

        // Read the overlay rect's *rendered* opacity.
        //
        // Under the bug (`on_sized` gated behind `final_opacity > 0.0`):
        //   overlay is never measured → never positioned → final_opacity stays 0.0 forever.
        //
        // Under the fix (`!positioned || final_opacity > 0.0` gate):
        //   on_sized fires on first layout → positioned → opacity ramps to ~1.0.
        //
        // We locate the overlay by its `Layer::Overlay` on the `RectElement` and
        // read `RectElement.effect.opacity` — the opacity value set by
        // `.opacity(final_opacity)` in `PopoverOverlay::render`.
        let overlay_opacity: Option<f32> = t.find(|_, el| {
            Rect::try_downcast(el)
                .filter(|r| matches!(r.relative_layer, Layer::Overlay))
                .and_then(|r| r.effect.as_ref().and_then(|e| e.opacity))
        });

        assert!(
            overlay_opacity.is_some(),
            "could not find the Layer::Overlay rect with an opacity value; \
             check that PopoverOverlay renders a Layer::Overlay rect with .opacity()"
        );
        let opacity = overlay_opacity.unwrap();
        assert!(
            opacity > 0.5,
            "overlay opacity should be > 0.5 after the entrance tween completes \
             (got {opacity:.3}); a value near 0 means the measurement-deadlock is active — \
             on_sized was never called so the overlay was never positioned"
        );
    }

    /// Snapshot of a mid-open Popover (scale < 1, opacity < 1 from the entrance tween).
    /// Run manually: cargo test -p oxide-ui popover_mid_open_snapshot -- --ignored
    #[test]
    #[ignore]
    fn popover_mid_open_snapshot() {
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
        t.poll(std::time::Duration::from_millis(5), std::time::Duration::from_millis(40));
        t.sync_and_update();
        let center = t.find(|node, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("anchor"))
                .map(|_| { let c = node.layout().visible_area().center(); (c.x as f64, c.y as f64) })
        }).unwrap();
        t.click_cursor(center); // open
        // Poll to ~60ms — mid-tween, scale < 1 / opacity < 1
        t.poll(std::time::Duration::from_millis(5), std::time::Duration::from_millis(60));
        t.sync_and_update();
        t.render_to_file("/tmp/popover-mid-open.png");
    }
}
