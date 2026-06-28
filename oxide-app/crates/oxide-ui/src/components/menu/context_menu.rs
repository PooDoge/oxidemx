//! `OxideContextMenuViewer` — an edge-aware replacement for Freya's
//! `ContextMenuViewer`.
//!
//! Freya's built-in viewer pins the menu's top-left at the cursor and grows
//! DOWN/RIGHT with no window-edge awareness, so a right-click near the bottom
//! (e.g. in the composer) clips the menu off-screen. This viewer measures the
//! menu and:
//! - flips it ABOVE the cursor when it would overflow the bottom edge, and
//! - clamps it horizontally inside the window.
//!
//! Mechanism mirrors Freya's: one viewer mounted high in the tree provides a
//! `OxideCtxMenu` state into `ScopeId::ROOT`; any descendant opens the menu via
//! [`open_context_menu`] from a right-click (`on_secondary_down`). Dismissal is
//! the Freya `Menu`'s own `on_close` (outside-press + Escape) — the menu mounts
//! AFTER the opening click, so that click can't self-close it.
use freya::animation::*;
use freya::prelude::*;

/// Mirrors Freya's `ContextMenuCloseRequest`. The Freya `Menu`'s `on_close`
/// fires once for the opening right-click's own release (it registers as an
/// outside-press); we IGNORE that first close so the menu stays open, then close
/// on the next press. Without this the menu opens on mouse-down and vanishes on
/// mouse-up.
#[derive(Clone, Copy, PartialEq)]
enum CloseReq {
    None,
    Pending,
}

/// Root-scoped state: the live cursor location + the open menu frozen with the
/// location it was opened at + the close-debounce flag.
#[derive(Clone, Copy, PartialEq)]
struct OxideCtxMenu {
    location:      State<CursorPoint>,
    menu:          State<Option<(CursorPoint, Menu)>>,
    close_request: State<CloseReq>,
    /// The open menu's measured layout area + the root size at measure time.
    /// Reset to `None` on each open so the overflow nudge is recomputed.
    measured:      State<Option<(Area, Size2D)>>,
}

/// How far to shift an overlay back when it overflows the window past `window`
/// (never more than `origin`, so it can't be pushed off the opposite edge).
/// Copied from Freya's `Menu` (`freya-components/src/menu.rs`) so our cursor-
/// anchored context menu uses the SAME consistent layout space as the rest of
/// Freya's overflow handling — comparing the cursor's `global_location` with
/// `root_size` (different spaces) is what made the naive flip mis-fire.
fn overflow_offset(origin: f32, size: f32, window: f32) -> f32 {
    let overflow = origin + size - window;
    if overflow > 0.0 {
        -overflow.min(origin)
    } else {
        0.0
    }
}

impl OxideCtxMenu {
    fn try_get() -> Option<Self> {
        try_consume_root_context::<OxideCtxMenu>()
    }
}

/// Open the edge-aware context menu at the right-click location. No-op if no
/// [`OxideContextMenuViewer`] is mounted in an ancestor scope.
pub fn open_context_menu(_event: &Event<PressEventData>, menu: Menu) {
    if let Some(mut this) = OxideCtxMenu::try_get() {
        let at = this.location.peek().to_owned();
        this.menu.set(Some((at, menu)));
        // Opened via right-click (`on_secondary_down`): ignore the first close so
        // the release of that same right-click doesn't immediately dismiss it.
        this.close_request.set(CloseReq::None);
        // Re-measure for the new position (recompute the overflow nudge).
        this.measured.set(None);
    }
}

/// Close the context menu (if open).
pub fn close_context_menu() {
    if let Some(mut this) = OxideCtxMenu::try_get() {
        this.menu.set(None);
    }
}

/// Mount once, high in the tree (typically the app shell root), so the menu
/// inherits root styling and the state lives at `ScopeId::ROOT`.
#[derive(Default, Clone, PartialEq)]
pub struct OxideContextMenuViewer {
    key: DiffKey,
}

impl KeyExt for OxideContextMenuViewer {
    fn write_key(&mut self) -> &mut DiffKey {
        &mut self.key
    }
}

impl OxideContextMenuViewer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ComponentOwned for OxideContextMenuViewer {
    fn render(self) -> impl IntoElement {
        let mut ctx = use_hook(|| {
            OxideCtxMenu::try_get().unwrap_or_else(|| {
                let state = OxideCtxMenu {
                    location:      State::create_in_scope(CursorPoint::default(), ScopeId::ROOT),
                    menu:          State::create_in_scope(None, ScopeId::ROOT),
                    close_request: State::create_in_scope(CloseReq::None, ScopeId::ROOT),
                    measured:      State::create_in_scope(None, ScopeId::ROOT),
                };
                provide_context_for_scope_id(state, ScopeId::ROOT);
                state
            })
        });

        // Persist the last menu so it stays mounted during the exit fade.
        // `ctx.menu` already stores `Option<(CursorPoint, Menu)>`, so `Menu` is
        // safe in `State`. Root-scoped so it lives as long as the viewer.
        let mut last_menu: State<Option<(CursorPoint, Menu)>> =
            use_hook(|| State::create_in_scope(None, ScopeId::ROOT));
        use_side_effect(move || {
            if let Some(m) = ctx.menu.read().clone() {
                last_menu.set(Some(m));
            }
        });

        // Dismiss when the window loses focus.
        use_side_effect(move || {
            if !*Platform::get().is_app_focused.read() {
                ctx.menu.set(None);
            }
        });

        // LOGICAL window size, measured by a full-window global probe rect — used
        // for the menu's edge clamp. `Platform::root_size` is PHYSICAL, so it's off
        // by the display scale factor (which Freya doesn't expose to components).
        let mut win: State<Option<Size2D>> = use_state(|| None);

        // Select-style persistent animation (OnCreation::Finish + OnChange::Rerun).
        // `menu_state` is captured by the factory and `.read()`-ed INSIDE the closure
        // so that `OnChange::Rerun` subscribes to the signal and re-runs when the menu
        // opens or closes.  Reading a plain-bool snapshot outside (the previous code)
        // subscribes to NOTHING — the factory only ever runs once at gen-0, the
        // `OnCreation::Finish` settles to the closed state (opacity=0), and the menu
        // renders permanently invisible.
        let menu_open  = ctx.menu.read().is_some(); // plain bool — for mount gate + measured-clear
        let menu_state = ctx.menu;                  // State<T> is Copy — captured for factory signal read
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
            let slide = AnimNum::new(-8.0_f32, 0.)
                .time(125)
                .ease(Ease::Out)
                .function(Function::Quart);
            // Read the SIGNAL inside the factory so OnChange::Rerun fires on open/close.
            if menu_state.read().is_some() {
                (scale, opacity, slide)
            } else {
                (scale.into_reversed(), opacity.into_reversed(), slide.into_reversed())
            }
        });
        let (anim_scale, anim_opacity, anim_slide) = animation.read().value();

        // Clear `measured` once fully closed so the next open re-measures at the
        // new cursor position.
        if !menu_open && anim_opacity == 0.0 && ctx.measured.peek().is_some() {
            let mut m = ctx.measured;
            m.set(None);
        }

        rect()
            .on_global_pointer_move(move |e: Event<PointerEventData>| {
                ctx.location.set(e.global_location());
            })
            // Invisible probe that fills the window; its measured area is the
            // window size in LOGICAL (layout) space, matching the menu's area.
            .child(
                rect()
                    .layer(Layer::Overlay)
                    .position(Position::new_global().left(0.0).top(0.0))
                    .width(Size::fill())
                    .height(Size::fill())
                    .opacity(0.0_f32)
                    .on_sized(move |e: Event<SizedEventData>| {
                        win.set_if_modified(Some(e.area.size));
                    }),
            )
            .maybe_child(
                // Stay mounted while open OR while the exit tween is still playing
                // (anim_opacity > 0.0). Unmounts only once fully faded out, so
                // click-away dismissal continues to work during the exit fade.
                (menu_open || anim_opacity > 0.0)
                    .then(|| ctx.menu.read().clone().or_else(|| last_menu.read().clone()))
                    .flatten()
                    .map(|(at, menu)| {
                        let at = at.to_f32();

                        // Place at the cursor, then nudge back into the window using
                        // the MEASURED area (origin + size in layout space) vs root_size
                        // — the same space, exactly how Freya's own `Menu` handles
                        // overflow. The `offset_*` are post-layout shifts (they don't
                        // move the measured area), so measuring once is enough; reset on
                        // each open. `measure_gate` hides the menu until measured, then
                        // multiplied with `anim_opacity` for a smooth open.
                        let (offset_x, offset_y, measure_gate) = match ctx.measured.read().as_ref() {
                            None => (0.0_f32, 0.0_f32, 0.0_f32),
                            Some((area, root)) => (
                                overflow_offset(area.origin.x, area.size.width, root.width),
                                overflow_offset(area.origin.y, area.size.height, root.height),
                                1.0_f32,
                            ),
                        };
                        let final_opacity = measure_gate * anim_opacity;

                        let mut measured = ctx.measured;
                        rect()
                            .layer(Layer::Overlay)
                            .position(Position::new_global().left(at.x).top(at.y))
                            .offset_x(offset_x)
                            .offset_y(offset_y + anim_slide)
                            .scale(anim_scale)
                            .opacity(final_opacity)
                            .on_sized(move |e: Event<SizedEventData>| {
                                // Clamp against the LOGICAL window size (measured by the
                                // probe below) — NOT Platform::root_size, which is PHYSICAL
                                // (window.inner_size()) and so off by the display scale.
                                if measured.peek().is_none() {
                                    if let Some(w) = *win.peek() {
                                        measured.set(Some((e.area, w)));
                                    }
                                }
                            })
                            // First close (the opening right-click's release) is ignored
                            // via the `close_request` debounce; the next press closes.
                            // Node stays mounted during the exit fade, so click-away still
                            // fires `on_close` and dismisses correctly.
                            .child(menu.on_close(move |_| match (ctx.close_request)() {
                                CloseReq::None => ctx.close_request.set(CloseReq::Pending),
                                CloseReq::Pending => {
                                    ctx.menu.set(None);
                                    ctx.close_request.set(CloseReq::None);
                                }
                            }))
                    }),
            )
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::TestingRunner;

    /// Regression guard for the "factory captures a plain bool" bug.
    ///
    /// Before the fix, `use_animation`'s factory read a snapshot `menu_open` bool
    /// captured OUTSIDE the closure, so it subscribed to no signal.
    /// `OnCreation::Finish` settled the animation to the CLOSED state (opacity=0)
    /// at gen-0, and the factory never re-ran — the menu rendered permanently
    /// transparent.
    ///
    /// The fix reads `menu_state.read()` (the `State` signal) INSIDE the factory,
    /// which causes `OnChange::Rerun` to re-run the factory when the menu opens.
    ///
    /// This test:
    /// 1. Mounts `OxideContextMenuViewer` + a right-click target.
    /// 2. Opens the menu via a one-shot `use_side_effect` on the first frame.
    /// 3. Polls past the 125 ms entrance tween (~260 ms total).
    /// 4. Asserts the **rendered opacity** of the menu rect is > 0.5 — not merely
    ///    that the label is present.  Under the bug, `anim_opacity` stays 0.0
    ///    and the menu is mount-gated out (`menu_open || anim_opacity > 0.0` with
    ///    `anim_opacity = 0`) so neither the label nor a visible rect ever appear.
    ///    Even if the mount gate was satisfied by `menu_open` alone (a stale bool
    ///    read outside the factory), the label would be present but opacity 0 —
    ///    this assertion catches that weaker form of the bug too.
    ///
    /// Disambiguation: three `Layer::Overlay` rects appear when the menu is open:
    ///   (1) the invisible probe (`.opacity(0.0)`, always 0),
    ///   (2) OUR menu container rect (`.opacity(final_opacity)`, near 1 when open), and
    ///   (3) Freya's internal `MenuContainer` overlay (`.opacity(1.0)` once measured).
    /// A naive "max opacity across all overlay rects" would pass even under the bug
    /// (because Freya's MenuContainer contributes 1.0 regardless).
    /// Instead we collect every `Layer::Overlay` rect node + its opacity, then pick
    /// the one whose subtree contains the "CTX-ITEM" label.  That is uniquely OUR
    /// rect: the probe has no child labels, and Freya's MenuContainer is a
    /// descendant of ours (inner node, not the topmost overlay).
    ///
    /// API used: `t.find_many` → `Rect::try_downcast(el)` + `r.relative_layer` +
    ///           `r.effect.as_ref().and_then(|e| e.opacity)`;
    ///           `TestingNode::children()` for the subtree walk.
    #[test]
    fn context_menu_becomes_visible_after_open() {

        fn app() -> Element {
            // Open the menu on the very first frame via a one-shot side effect.
            // We use a State flag to ensure we only open once.
            let mut opened = use_state(|| false);
            use_side_effect(move || {
                if !opened() {
                    opened.set(true);
                    // Build a minimal Menu with a labelled MenuButton so we can
                    // also assert its presence as a sanity check.
                    let menu = Menu::new().child(
                        MenuButton::new().child("CTX-ITEM"),
                    );
                    // `open_context_menu` only needs an event for its signature;
                    // the location is taken from `ctx.location` which defaults to
                    // CursorPoint(0,0) — fine for a headless layout test.
                    if let Some(mut ctx) = OxideCtxMenu::try_get() {
                        let at = ctx.location.peek().to_owned();
                        ctx.menu.set(Some((at, menu)));
                        ctx.close_request.set(CloseReq::None);
                        ctx.measured.set(None);
                    }
                }
            });

            rect()
                .expanded()
                .child(OxideContextMenuViewer::new())
                .into_element()
        }

        let (mut t, _) = TestingRunner::new(app, (400., 320.).into(), |_| {}, 1.);
        // Let the first frame (and the side-effect open) settle.
        t.poll(std::time::Duration::from_millis(5), std::time::Duration::from_millis(40));
        t.sync_and_update();
        // Poll well past the 125 ms entrance tween so the animation reaches ~1.0.
        t.poll(std::time::Duration::from_millis(10), std::time::Duration::from_millis(260));
        t.sync_and_update();

        // Sanity: the CTX-ITEM label must be mounted.
        let item = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("CTX-ITEM"))
        });
        assert!(
            item.is_some(),
            "CTX-ITEM label must be mounted after open"
        );

        // Find the *rendered opacity* of the OxideContextMenuViewer's menu rect —
        // not the probe rect (opacity=0 always) and not Freya's internal
        // `MenuContainer` overlay (opacity=1 once measured).
        //
        // Disambiguation: collect every `Layer::Overlay` rect node + its rendered
        // opacity, then pick the one whose subtree contains the "CTX-ITEM" label.
        // That is uniquely our menu container rect: the probe has no label child,
        // and Freya's MenuContainer is nested INSIDE our rect (it's an inner node,
        // not the topmost overlay).
        //
        // Helper: depth-first check whether a TestingNode's subtree contains a
        // Label whose text contains `needle`.
        fn subtree_has_label(node: &freya_testing::TestingNode, needle: &str) -> bool {
            if let Some(l) = Label::try_downcast(node.element().as_ref()) {
                if l.text.as_ref().contains(needle) {
                    return true;
                }
            }
            node.children().iter().any(|c| subtree_has_label(c, needle))
        }

        // Gather all Layer::Overlay rects along with their node handles.
        let overlay_rects = t.find_many(|node, el| {
            Rect::try_downcast(el)
                .filter(|r| matches!(r.relative_layer, Layer::Overlay))
                .and_then(|r| r.effect.as_ref().and_then(|e| e.opacity))
                .map(|op| (node, op))
        });

        // Identify the menu container rect — the overlay rect whose subtree holds
        // the "CTX-ITEM" label (i.e. OUR rect, not the probe and not the inner
        // Freya MenuContainer rect that is a descendant of ours).
        let menu_rect_opacity = overlay_rects
            .into_iter()
            .find(|(node, _)| subtree_has_label(node, "CTX-ITEM"))
            .map(|(_, op)| op);

        assert!(
            menu_rect_opacity.is_some(),
            "could not find the Layer::Overlay rect whose subtree contains CTX-ITEM; \
             the menu may not have mounted"
        );

        let opacity = menu_rect_opacity.unwrap();
        assert!(
            opacity > 0.5,
            "menu rect opacity should be > 0.5 after the entrance tween completes \
             (got {opacity:.3}); a value near 0 means the animation factory never \
             re-ran — the plain-bool capture bug is active"
        );
    }
}
