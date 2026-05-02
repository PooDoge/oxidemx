//! Slice geometry: angular ranges, hit-testing, and cairo path
//! construction. Pure math + cairo calls; no GTK widget code.
//!
//! See `overlay/overlay_painting.py` (`_draw_3d_icon`,
//! `_draw_minimal_icon`, `_draw_submenu`) for the visual reference
//! we're matching.

// TODO: port slice arc rendering, the centre puck, the bloom/flash
// animations, and the submenu droplet pop-out.
