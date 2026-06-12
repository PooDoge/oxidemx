//! Submenu pop-out state (`SubmenuState`) plus the sub-item
//! geometry constants and hit-testing shared with the renderer.

use crate::anim::Tween;
use crate::geometry::MENU_RADIUS;
use oxidemx_shared::{ElementAnimation, Slice};

/// Distance from the menu centre to a submenu sub-item's centre,
/// in logical pixels. Has to clear the outer ring so the popped-out
/// items sit beyond the main wedges. Mirrors the Python overlay's
/// `SUBMENU_RADIUS = MENU_RADIUS + 45`.
pub const SUBMENU_RADIUS: f32 = (MENU_RADIUS as f32) + 45.0;
/// Visible radius of each sub-item disc.
pub const SUBITEM_RENDER_RADIUS: f32 = 24.0;
/// Generous hit-test radius — slightly larger than render so the
/// items don't feel "spiky" when crossing between them.
pub const SUBITEM_HIT_RADIUS: f32 = 32.0;
/// Degrees between adjacent sub-items, measured from the menu centre.
/// Render uses 18°, hit-test uses 15° (matches Python overlay).
pub const SUBITEM_RENDER_SPREAD_DEG: f32 = 18.0;
pub const SUBITEM_HIT_SPREAD_DEG: f32 = 15.0;

/// Open submenu state. `None` when the user isn't hovering a
/// Submenu-kind slice (the common case). Created when hover lands
/// on a slice with `kind == Submenu` and a non-empty `submenu` vec;
/// dropped (after an exit fade settles) when the cursor leaves both
/// the parent slice and the sub-item arc.
#[derive(Debug, Clone)]
pub struct SubmenuState {
    /// Index of the parent slice (0..7) that opened this submenu.
    pub parent: usize,
    /// 0.0 → 1.0 grow-out tween. Drives the per-item radius / scale
    /// / opacity in the renderer via `crate::anim::evaluate`. Speed
    /// + curve come from `AnimationConfig::submenu`.
    ///
    /// The tween's `duration_ms` is **extended** to fit the full
    /// chain (`duration + (item_count − 1) × stagger`) so that the
    /// last sub-item gets to finish its per-item animation before
    /// `is_idle()` short-circuits the clock. Per-item progress eval
    /// in `anim::evaluate_chain_item` still uses the configured
    /// per-item duration. See `anim::extended_chain_duration_ms`.
    pub progress: Tween,
    /// Currently-hovered sub-item index, or `None` for "between
    /// items, mouse over the parent wedge".
    pub highlighted: Option<usize>,
    /// Cached count of sub-items at open time. Stored so
    /// `begin_exit` can recompute the chain duration without
    /// re-borrowing the parent slice list.
    pub item_count: usize,
}

impl SubmenuState {
    pub(super) fn new(parent: usize, item_count: usize, anim: &ElementAnimation) -> Self {
        let mut progress = Tween::at(0.0);
        let stagger = anim
            .chain
            .as_ref()
            .map(|c| c.stagger_ms as f32)
            .unwrap_or(0.0);
        let mut enter = anim.enter.clone();
        enter.duration_ms =
            crate::anim::extended_chain_duration_ms(enter.duration_ms, item_count, stagger);
        progress.set_target(1.0, &enter);
        Self {
            parent,
            progress,
            highlighted: None,
            item_count,
        }
    }

    /// Trigger the exit tween with chain-extended duration so
    /// every sub-item gets to finish its individual exit
    /// animation. Use this instead of calling
    /// `progress.set_target(0.0, &anim.exit)` directly — the
    /// raw call would only run for `anim.exit.duration_ms`,
    /// freezing the late items mid-flight.
    pub fn begin_exit(&mut self, anim: &ElementAnimation) {
        let stagger = anim
            .chain
            .as_ref()
            .map(|c| c.stagger_ms as f32)
            .unwrap_or(0.0);
        let mut exit = anim.exit.clone();
        exit.duration_ms =
            crate::anim::extended_chain_duration_ms(exit.duration_ms, self.item_count, stagger);
        self.progress.set_target(0.0, &exit);
    }
}

/// Hit-test the sub-items of slot `parent`. Returns the index of the
/// sub-item whose centre is within `SUBITEM_HIT_RADIUS` of the
/// cursor, or `None`. `dx`/`dy` are the cursor offset from the menu
/// centre (same convention as `slice_index_at`).
///
/// Sub-items are arranged on an arc of radius `SUBMENU_RADIUS`
/// centred on the parent slice's bisector, with
/// `SUBITEM_HIT_SPREAD_DEG` between adjacent items. Mirrors
/// `_get_subitem_at_position` in the legacy Python overlay.
pub fn subitem_at(dx: f64, dy: f64, parent: usize, slices: &[Slice]) -> Option<usize> {
    let parent_slice = slices.get(parent)?;
    if parent_slice.submenu.is_empty() {
        return None;
    }
    let n = parent_slice.submenu.len() as f32;
    let parent_angle_deg = (parent as f32) * 45.0 - 90.0;
    for (i, _) in parent_slice.submenu.iter().enumerate() {
        let offset_deg = (i as f32 - (n - 1.0) / 2.0) * SUBITEM_HIT_SPREAD_DEG;
        let item_angle = (parent_angle_deg + offset_deg).to_radians();
        let item_x = SUBMENU_RADIUS * item_angle.cos();
        let item_y = SUBMENU_RADIUS * item_angle.sin();
        let dist = ((dx - item_x as f64).powi(2) + (dy - item_y as f64).powi(2)).sqrt();
        if dist < SUBITEM_HIT_RADIUS as f64 {
            return Some(i);
        }
    }
    None
}
