//! `Popover` — anchors floating `content` to a trigger `anchor`.
//!
//! Built on Freya's [`Attached`] (positions an overlay above/below the inner
//! element by measuring both) plus a Select-style 3-part entrance animation
//! (scale 0.9→1, opacity 0→1, slide ∓8→0, ~125ms `Ease::Out` `Function::Quart`,
//! reversed on close). The overlay subtree is only built while `open` or the
//! close animation is still running, and is opacity-gated until measured so it
//! never flashes unsized.
//!
//! Auto-flip: when `placement == Below` but the measured content height exceeds
//! the space below the anchor (`Platform::get().root_size.height − anchor.max_y`)
//! and fits above, the content renders Above instead (and vice-versa). This
//! mirrors `Select`'s flip math.
//!
//! ## Dismissal guard (the subtle part)
//!
//! Dismissal must fire on an outside press (a press NOT on the anchor or the
//! content) or Escape, BUT the press that OPENS the popover (handled by the
//! caller on the trigger) must not immediately fire `on_dismiss` and cause an
//! open→close flicker.
//!
//! We solve this with a **full-window transparent backdrop** rect rendered on
//! `Layer::Relative(-1)` (behind the anchor's `Relative(0)`) at
//! `Position::new_global`, whose `on_press` fires `on_dismiss(())`. The content
//! sits on `Layer::Overlay` (far above everything), so it paints — and
//! hit-tests — over the backdrop. The anchor sits at the wrapper's base layer,
//! above the backdrop. So:
//!   - A press on the **content** hits the content (which `stop_propagation`s),
//!     never the backdrop.
//!   - A press on the **anchor/trigger** hits the anchor — the backdrop is
//!     *behind* it — so the caller's trigger handler (which `stop_propagation`s
//!     and toggles open) runs, and the backdrop's dismiss never fires. No
//!     open→close flicker.
//!   - A press anywhere **else** lands on the backdrop → dismiss.
//!
//! Escape is handled by a global key-down on the wrapper.
//!
//! This is the closest-correct flicker-free guard reachable from a
//! content-agnostic public API: the alternative (`on_global_pointer_press` that
//! checks the press is outside the measured content area, à la `Select`) would
//! require the open-toggling press to also `stop_propagation` on the SAME node
//! that owns the global handler — which we cannot guarantee for an arbitrary
//! caller-supplied anchor. The backdrop makes the guard structural instead.
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

/// Anchors floating `content` to a trigger `anchor`.
///
/// Builder usage:
/// ```ignore
/// Popover::new(trigger_element)
///     .open(is_open)
///     .placement(Placement::Below)
///     .on_dismiss(move |()| set_open(false))
///     .content(MenuSurface::new(theme).child(body))
/// ```
#[derive(PartialEq)]
pub struct Popover {
    anchor: Element,
    open: bool,
    placement: Placement,
    content: Option<Element>,
    on_dismiss: Option<EventHandler<()>>,
}

impl Popover {
    pub fn new(anchor: impl IntoElement) -> Self {
        Self {
            anchor: anchor.into_element(),
            open: false,
            placement: Placement::default(),
            content: None,
            on_dismiss: None,
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

    pub fn on_dismiss(mut self, h: impl Into<EventHandler<()>>) -> Self {
        self.on_dismiss = Some(h.into());
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

        // 3-part entrance animation, reversed on close (mirrors `Select`).
        let animation = use_animation(move |conf| {
            conf.on_change(OnChange::Rerun);
            conf.on_creation(OnCreation::Finish);

            let scale = AnimNum::new(0.9, 1.)
                .time(125)
                .ease(Ease::Out)
                .function(Function::Quart);
            let opacity = AnimNum::new(0., 1.)
                .time(125)
                .ease(Ease::Out)
                .function(Function::Quart);
            let slide = AnimNum::new(-8., 0.)
                .time(125)
                .ease(Ease::Out)
                .function(Function::Quart);
            if open {
                (scale, opacity, slide)
            } else {
                (
                    scale.into_reversed(),
                    opacity.into_reversed(),
                    slide.into_reversed(),
                )
            }
        });

        let is_animating = *animation.is_running().read();
        let show_overlay = open || is_animating;

        let (scale, opacity, slide) = animation.read().value();

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
        // The slide direction follows the effective placement: content above
        // slides up (negative), content below slides down (positive offset).
        let measured = content_size().is_some();
        let gated_opacity = if measured { opacity } else { 0. };
        let offset_y = match effective_placement {
            Placement::Above => -slide,
            Placement::Below => slide,
        };

        let on_dismiss = self.on_dismiss.clone();
        let on_dismiss_key = self.on_dismiss.clone();

        // Escape → dismiss (global key-down on the wrapper).
        let on_global_key_down = move |e: Event<KeyboardEventData>| {
            if e.key == Key::Named(NamedKey::Escape) {
                if let Some(h) = &on_dismiss_key {
                    h.call(());
                }
            }
        };

        let attached_position = match effective_placement {
            Placement::Above => AttachedPosition::Top,
            Placement::Below => AttachedPosition::Bottom,
        };

        let content_el = self.content.clone();

        // The animated content rect: scale + opacity + slide, measured on size.
        let overlay: Option<Element> = (show_overlay && content_el.is_some()).then(|| {
            let content = content_el.clone().unwrap();
            rect()
                .layer(Layer::Overlay)
                .offset_y(offset_y)
                .scale(scale)
                .opacity(gated_opacity)
                .on_sized(move |e: Event<SizedEventData>| {
                    content_size.set_if_modified(Some(e.area.size));
                })
                // A press on the content must not bubble to the backdrop.
                .on_press(move |e: Event<PressEventData>| {
                    e.stop_propagation();
                })
                .child(content)
                .into_element()
        });

        // Full-window transparent backdrop, behind the content, whose press
        // dismisses. Only built while the overlay is shown. See module docs.
        let backdrop: Option<Element> = show_overlay.then(|| {
            rect()
                .position(Position::new_global().top(0.).left(0.))
                .width(Size::window_percent(100.))
                .height(Size::window_percent(100.))
                // Paint behind the anchor (Relative(0)) so a press on the anchor
                // is NOT intercepted by the backdrop — that press belongs to the
                // caller's trigger handler (which stop_propagation's it), and
                // must never reach the backdrop's dismiss. The content sits on
                // `Layer::Overlay` (far above), so it still beats the backdrop.
                .layer(Layer::Relative(-1))
                .on_press(move |e: Event<PressEventData>| {
                    e.stop_propagation();
                    if let Some(h) = &on_dismiss {
                        h.call(());
                    }
                })
                .into_element()
        });

        rect()
            .on_global_key_down(on_global_key_down)
            .maybe_child(backdrop)
            .child(
                Attached::new(
                    rect()
                        .on_sized(move |e: Event<SizedEventData>| {
                            anchor_area.set_if_modified(Some(e.area));
                        })
                        .child(self.anchor.clone()),
                )
                .position(attached_position)
                .maybe_child(overlay),
            )
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
}
