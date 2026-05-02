//! Hover + click hit-testing.
//!
//! Two cursor sources, both producing the same kind of polar coordinate
//! around the menu centre:
//!
//!   * **Drag mode** — the daemon emits relative `dx, dy` deltas via
//!     `CursorMoved` D-Bus signals while the gesture button is held. We
//!     consume those directly; no widget-local translation needed.
//!   * **Toggle mode** — after a quick tap, the menu stays open and the
//!     user moves the OS cursor freely. `gtk::EventControllerMotion`
//!     gives us coords already local to the radial widget (and GTK4
//!     handles HiDPI for us, unlike Qt+xcb), so we can just compute the
//!     polar coords from the widget centre.
//!
//! Either way the angle/distance math, slice selection (8 slices, 45°
//! each), and the centre-deadzone check are identical to the legacy
//! Python overlay.

use crate::window::WINDOW_SIZE;

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
pub const HALF: f64 = WINDOW_SIZE as f64 / 2.0;
