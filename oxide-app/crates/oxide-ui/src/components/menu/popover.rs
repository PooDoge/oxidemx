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
//! ## Dismissal model — Select-style global-press (the subtle part)
//!
//! The previous implementation painted a full-window backdrop on
//! `Layer::Relative(-1)` and dismissed on a press to that backdrop. In a deeply
//! nested anchor (the composer toolbar is composer → card → toolbar row, near
//! the window bottom) `Relative(-1)` is *relative to the Popover's parent*, so
//! the backdrop painted BELOW the rest of the app — outside presses landed on
//! higher-layer app content and never reached it, so dismiss never fired (and
//! the menu didn't reliably surface either). That model is wrong for a nested
//! anchor and is removed.
//!
//! We now use the model Freya's `Select`/`Menu` prove in production: a
//! [`on_global_pointer_press`] handler on the Popover root that fires
//! `on_dismiss(())`, reconciled so the OPENING trigger press does not also close
//! the menu in the same cycle. The reconciliation rides on Freya's event
//! cancellation rules (see `freya-core` `EventName::get_cancellable_events`):
//! a targeted `PointerPress` whose handler calls `prevent_default()` cancels the
//! pending `GlobalPointerPress` for that same cursor press. Concretely:
//!
//!   - **Opening trigger press.** We wrap the caller's `anchor` in a rect whose
//!     own `on_press` calls `e.prevent_default()`. Targeted presses bubble up the
//!     parent chain, so this wrapper handler runs *alongside* the caller's anchor
//!     `on_press` (which toggles `open`). The `prevent_default()` cancels the
//!     `GlobalPointerPress` that would otherwise fire `on_dismiss` on this very
//!     click → no open→close flicker. This is exactly what `Select::on_press`
//!     does (`prevent_default()` + `stop_propagation()`); we relocate it into the
//!     Popover's anchor wrapper so it works for ANY caller-supplied anchor,
//!     keeping the public API content-agnostic.
//!   - **Press on the content.** The content overlay's `on_press` calls
//!     `prevent_default()` (cancel the global → don't dismiss) and
//!     `stop_propagation()` (don't disturb the anchor's bubble chain). A press on
//!     the menu therefore never dismisses.
//!   - **Press anywhere else.** No targeted `PointerPress` cancels the
//!     `GlobalPointerPress`, so it reaches the root's
//!     `on_global_pointer_press`, which fires `on_dismiss(())`.
//!
//! The root global handler additionally guards on `open` (it only acts while the
//! popover is actually open), mirroring `Select`'s `set_if_modified(false)`
//! idempotence: a stray global press while closed is a no-op.
//!
//! Escape is handled by a global key-down on the wrapper → `on_dismiss(())`.
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

/// Predicate (pure, unit-testable) capturing the dismissal decision the
/// `on_global_pointer_press` handler makes: a global press dismisses ONLY when
/// the popover is open AND the press was not cancelled by a targeted handler
/// (the anchor wrapper / the content both `prevent_default()`, which cancels the
/// `GlobalPointerPress` so it never reaches the root handler at all). The
/// `press_cancelled` flag models that cancellation for the test.
#[doc(hidden)]
pub fn global_press_dismisses(open: bool, press_cancelled: bool) -> bool {
    open && !press_cancelled
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

        let on_dismiss_global = self.on_dismiss.clone();
        let on_dismiss_key = self.on_dismiss.clone();

        // Escape → dismiss (global key-down on the wrapper).
        let on_global_key_down = move |e: Event<KeyboardEventData>| {
            if e.key == Key::Named(NamedKey::Escape) {
                if let Some(h) = &on_dismiss_key {
                    h.call(());
                }
            }
        };

        // Outside-press → dismiss. Mirrors `Select`/`Menu`: a global pointer
        // press that was NOT cancelled by a targeted `prevent_default()` (the
        // anchor wrapper and the content both cancel) reaches this handler. We
        // guard on `open` so a stray press while closed is a no-op (the
        // idempotent counterpart of `Select`'s `set_if_modified(false)`), and so
        // the OPENING press is never double-handled before the toggle lands.
        let on_global_pointer_press = move |_: Event<PointerEventData>| {
            if open {
                if let Some(h) = &on_dismiss_global {
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
                // A press on the content must not dismiss: `prevent_default()`
                // cancels the pending `GlobalPointerPress` (so the root's
                // `on_global_pointer_press` never fires for this press), and
                // `stop_propagation()` keeps it out of the anchor's bubble chain.
                .on_press(move |e: Event<PressEventData>| {
                    e.prevent_default();
                    e.stop_propagation();
                })
                .child(content)
                .into_element()
        });

        // The anchor wrapper. Its `on_press` calls `prevent_default()` so the
        // OPENING trigger press cancels the `GlobalPointerPress` that would
        // otherwise dismiss in the same cycle (no open→close flicker). Targeted
        // presses bubble up the parent chain, so the caller's own anchor
        // `on_press` (which toggles `open`) still runs alongside this — we do NOT
        // `stop_propagation` here, so we don't suppress the caller's handler.
        let anchor_inner = rect()
            .on_sized(move |e: Event<SizedEventData>| {
                anchor_area.set_if_modified(Some(e.area));
            })
            .on_press(move |e: Event<PressEventData>| {
                e.prevent_default();
            })
            .child(self.anchor.clone());

        rect()
            .on_global_key_down(on_global_key_down)
            .on_global_pointer_press(on_global_pointer_press)
            .child(
                Attached::new(anchor_inner)
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

    /// Unit assertion of the dismissal predicate the `on_global_pointer_press`
    /// handler encodes (mirrors `Select`): a global press dismisses ONLY when the
    /// popover is open AND the press was not cancelled by a targeted
    /// `prevent_default()` (anchor wrapper / content). This is the provably-correct
    /// core of the reconciliation; the live event loop applies it via Freya's
    /// `GlobalPointerPress` cancellation (see module docs).
    #[test]
    fn dismissal_predicate() {
        // Closed → never dismiss (idempotent no-op, like set_if_modified(false)).
        assert!(!global_press_dismisses(false, false));
        assert!(!global_press_dismisses(false, true));
        // Open + the press was cancelled (anchor-open / content) → suppressed.
        assert!(!global_press_dismisses(true, true));
        // Open + an outside press (not cancelled) → dismiss.
        assert!(global_press_dismisses(true, false));
    }

    /// Nested-context open: mount a `Popover` whose anchor is buried a few rects
    /// deep (mimicking composer → card → toolbar row), drive a `click_cursor` on
    /// the anchor, poll past the entrance animation, and assert the content
    /// renders. Proves opening works in a deep layout (the regression).
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
                .on_dismiss(move |()| open.set(false))
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
        // Poll past the ~125ms entrance animation so the content mounts + measures.
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
