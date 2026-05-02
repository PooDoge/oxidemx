//! Layout constants for the radial menu, ported from
//! `overlay/overlay_constants.py` so the visual proportions of the
//! Rust overlay match the legacy Python overlay exactly.
//!
//! All values are in logical pixels — cairo + GTK4 handle the
//! HiDPI multiply for us. (This is the part Qt+xcb made hard; layer-
//! shell + GTK4 means we never have to think about it.)

/// Visual radius of the menu's outer ring.
pub const MENU_RADIUS: f64 = 150.0;
/// Shadow halo extending past the visible disc.
pub const SHADOW_OFFSET: f64 = 12.0;
/// Inner deadzone where the centre puck sits — anything closer than
/// this to the menu centre counts as "no slice selected".
pub const CENTER_ZONE_RADIUS: f64 = 45.0;
/// Radial distance from menu centre at which slice icons sit.
pub const ICON_ZONE_RADIUS: f64 = 100.0;
/// Extra space outside the main ring reserved for submenu pop-outs.
pub const SUBMENU_EXTEND: f64 = 80.0;

/// Total window dimension. `WINDOW_SIZE × WINDOW_SIZE` is what we ask
/// the layer-shell surface for. Matches
/// `overlay/overlay_constants.py:WINDOW_SIZE = 484`.
pub const WINDOW_SIZE: f64 = (MENU_RADIUS + SHADOW_OFFSET + SUBMENU_EXTEND) * 2.0;

/// Convenience: half the window dimension. Centre of the radial widget.
pub const CENTER: f64 = WINDOW_SIZE / 2.0;

/// Render geometry resolved into concrete numbers for one paint pass.
/// Today this is just a wrapper over the constants, but giving it a
/// type means the rendering functions take a single `geom` parameter
/// instead of half a dozen floats — and a future user-configurable
/// menu radius drops in here without touching the call sites.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    pub cx: f64,
    pub cy: f64,
    pub menu_radius: f64,
    pub center_radius: f64,
    pub icon_radius: f64,
}

impl Default for Geometry {
    fn default() -> Self {
        Geometry {
            cx: CENTER,
            cy: CENTER,
            menu_radius: MENU_RADIUS,
            center_radius: CENTER_ZONE_RADIUS,
            icon_radius: ICON_ZONE_RADIUS,
        }
    }
}
