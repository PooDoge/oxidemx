//! Hover + click hit-testing.
//!
//! Two cursor sources, both producing the same kind of polar
//! coordinate around the menu centre:
//!
//!   * **Drag mode** — the daemon emits relative `dx, dy` deltas via
//!     `CursorMoved` D-Bus signals while the gesture button is held.
//!     We consume those directly; no widget-local translation needed.
//!   * **Toggle mode** — after a quick tap, the menu stays open and
//!     the user moves the OS cursor freely.
//!     `gtk::EventControllerMotion` gives us coords already local to
//!     the radial widget (and GTK4 handles HiDPI for us, unlike
//!     Qt+xcb), so we can just compute the polar coords from the
//!     widget centre.
//!
//! Either way the angle/distance math, slice selection (8 slices,
//! 45° each), and the centre-deadzone check are identical to the
//! legacy Python overlay.

use crate::geometry::WINDOW_SIZE;

/// Turn an `(x, y)` offset from the menu centre into the highlighted
/// slice index (0..7), or `None` when the cursor is in the centre
/// deadzone or past the outer ring.
///
/// Slice 0 is the slot at the top (12 o'clock). Indexing proceeds
/// clockwise (1 = top-right, 2 = right, …, 7 = top-left). The angle
/// is measured from the +Y-up axis; GTK and cairo both have +Y
/// pointing *down*, so we flip it: `atan2(dx, -dy)`. The `+22.5`
/// then `/45` rounds to the nearest 45° slot.
pub fn slice_index_at(dx: f64, dy: f64, center_radius: f64, max_radius: f64) -> Option<usize> {
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < center_radius || distance > max_radius {
        return None;
    }
    let mut angle = dx.atan2(-dy).to_degrees();
    if angle < 0.0 {
        angle += 360.0;
    }
    Some(((angle + 22.5) / 45.0) as usize % 8)
}

/// Half the menu diameter in logical pixels — the (cx, cy) origin
/// against which `gtk::EventControllerMotion` deltas are computed.
pub const HALF: f64 = WINDOW_SIZE / 2.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{CENTER_ZONE_RADIUS, MENU_RADIUS};

    fn slot(dx: f64, dy: f64) -> Option<usize> {
        slice_index_at(dx, dy, CENTER_ZONE_RADIUS, MENU_RADIUS)
    }

    #[test]
    fn straight_up_is_top() {
        // Cursor 100 above centre — clearly in slot 0.
        assert_eq!(slot(0.0, -100.0), Some(0));
    }

    #[test]
    fn straight_right_is_index_two() {
        assert_eq!(slot(100.0, 0.0), Some(2));
    }

    #[test]
    fn straight_down_is_index_four() {
        assert_eq!(slot(0.0, 100.0), Some(4));
    }

    #[test]
    fn straight_left_is_index_six() {
        assert_eq!(slot(-100.0, 0.0), Some(6));
    }

    #[test]
    fn diagonals_match_clockwise_order() {
        // 45° each, starting from top, going clockwise.
        assert_eq!(slot(70.0, -70.0), Some(1)); // top-right
        assert_eq!(slot(70.0, 70.0), Some(3)); // bottom-right
        assert_eq!(slot(-70.0, 70.0), Some(5)); // bottom-left
        assert_eq!(slot(-70.0, -70.0), Some(7)); // top-left
    }

    #[test]
    fn dead_zone_returns_none() {
        // Inside the centre deadzone — no slice highlighted.
        assert_eq!(slot(5.0, 5.0), None);
        assert_eq!(slot(0.0, 0.0), None);
    }

    #[test]
    fn outside_max_radius_returns_none() {
        // Past the outer ring.
        assert_eq!(slot(0.0, -(MENU_RADIUS + 10.0)), None);
        assert_eq!(slot(MENU_RADIUS + 50.0, 0.0), None);
    }

    #[test]
    fn boundary_between_slots_rounds_up() {
        // Right at the +22.5° boundary between slot 0 and slot 1 —
        // anything ≥ 22.5° lands in slot 1.
        let r = MENU_RADIUS / 2.0;
        let a = 22.5_f64.to_radians();
        let dx = r * a.sin();
        let dy = -r * a.cos();
        assert_eq!(slot(dx + 0.5, dy), Some(1));
    }
}
