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
            .maybe_child(ctx.menu.read().clone().map(|(at, menu)| {
                let at = at.to_f32();

                // Place at the cursor, then nudge back into the window using the
                // MEASURED area (origin + size in layout space) vs root_size — the
                // same space, exactly how Freya's own `Menu` handles overflow. The
                // `offset_*` are post-layout shifts (they don't move the measured
                // area), so measuring once is enough; reset on each open.
                let (offset_x, offset_y, opacity) = match ctx.measured.read().as_ref() {
                    None => (0.0_f32, 0.0_f32, 0.0_f32),
                    Some((area, root)) => (
                        overflow_offset(area.origin.x, area.size.width, root.width),
                        overflow_offset(area.origin.y, area.size.height, root.height),
                        1.0_f32,
                    ),
                };

                let mut measured = ctx.measured;
                rect()
                    .layer(Layer::Overlay)
                    .position(Position::new_global().left(at.x).top(at.y))
                    .offset_x(offset_x)
                    .offset_y(offset_y)
                    .opacity(opacity)
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
                    // First close (the opening right-click's release) is ignored via
                    // the `close_request` debounce; the next press actually closes.
                    .child(menu.on_close(move |_| match (ctx.close_request)() {
                        CloseReq::None => ctx.close_request.set(CloseReq::Pending),
                        CloseReq::Pending => {
                            ctx.menu.set(None);
                            ctx.close_request.set(CloseReq::None);
                        }
                    }))
            }))
    }
}
