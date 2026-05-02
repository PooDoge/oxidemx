//! Vector-mode slice rendering. Cairo translation of
//! `overlay/overlay_painting.py::_draw_slice` (line 360+).
//!
//! Each slice is a 45° donut wedge with:
//!   * a faint base fill (theme `surface0` @ alpha 80)
//!   * a stroked border that brightens on hover (theme `surface2` →
//!     white as `highlight` interpolates 0.0 → 1.0)
//!   * a hover overlay (white fade-in)
//!   * a circular icon background at `ICON_ZONE_RADIUS` along the
//!     slice's bisector
//!   * an icon glyph centred in that background (TODO; for now a
//!     placeholder dot so the geometry is visible end-to-end).
//!
//! Angles in cairo are clockwise radians starting at +X. We convert
//! the Python overlay's "0° = top, +clockwise" convention to cairo's
//! "0° = +X, +clockwise" by subtracting 90° (π/2 rad) wherever a slice
//! index drives the angle.

use cairo::Context;
use juhradial_shared::{Slice, ThemeColors};

use crate::geometry::Geometry;
use crate::render::icons::{draw_icon, IconCache};
use crate::theme::ActiveTheme;
use juhradial_shared::theme::parse_hex_rgba;

/// The width of each slice in degrees (8 slices = 360°/8).
const SLICE_DEGREES: f64 = 45.0;

/// Inner / outer radii inset slightly from the absolute geometry so
/// adjacent slices don't visually merge at the borders. Match the
/// Python overlay's `outer_r = MENU_RADIUS - 6` /
/// `inner_r = CENTER_ZONE_RADIUS + 6`.
const RING_OUTER_INSET: f64 = 6.0;
const RING_INNER_INSET: f64 = 6.0;

/// Radius of the per-slice icon background circle.
const ICON_BG_RADIUS: f64 = 26.0;

/// Render a single slice. `highlight` is the per-slice hover progress
/// in `[0.0, 1.0]` driven by the animation system (1.0 = fully
/// highlighted). `slice` carries the user-configured label / colour /
/// icon for slot `index`.
pub fn draw_slice(
    cr: &Context,
    geom: &Geometry,
    index: usize,
    slice: &Slice,
    theme: &ActiveTheme,
    highlight: f64,
    icons: &IconCache,
) {
    let palette = &theme.theme.colors;

    // Slice angular range in cairo coords (clockwise from +X axis,
    // radians). Python uses `index * 45 - 22.5 - 90` with degrees;
    // the −90 rotates "0° = top" into cairo's "0° = right".
    let start_deg = index as f64 * SLICE_DEGREES - SLICE_DEGREES / 2.0 - 90.0;
    let end_deg = start_deg + SLICE_DEGREES;
    let start_rad = start_deg.to_radians();
    let end_rad = end_deg.to_radians();

    let outer_r = geom.menu_radius - RING_OUTER_INSET;
    let inner_r = geom.center_radius + RING_INNER_INSET;

    // Build the donut-wedge path once and re-use for fill + stroke +
    // hover overlay.
    cr.new_path();
    let inner_start = polar(geom.cx, geom.cy, inner_r, start_rad);
    cr.move_to(inner_start.0, inner_start.1);
    let outer_start = polar(geom.cx, geom.cy, outer_r, start_rad);
    cr.line_to(outer_start.0, outer_start.1);
    cr.arc(geom.cx, geom.cy, outer_r, start_rad, end_rad);
    let inner_end = polar(geom.cx, geom.cy, inner_r, end_rad);
    cr.line_to(inner_end.0, inner_end.1);
    // Inner arc backwards.
    cr.arc_negative(geom.cx, geom.cy, inner_r, end_rad, start_rad);
    cr.close_path();

    // Base fill — theme surface0 @ alpha 80/255.
    let (r, g, b, _) = parse_hex_rgba(&palette.surface0).unwrap_or((0.2, 0.2, 0.3, 1.0));
    cr.set_source_rgba(r, g, b, 80.0 / 255.0);
    let _ = cr.fill_preserve();

    // Border — interpolate surface2 -> white on hover, alpha 60..120,
    // line width 1.0..1.5.
    let (br, bg, bb, _) = parse_hex_rgba(&palette.surface2).unwrap_or((0.4, 0.4, 0.5, 1.0));
    let lr = lerp(br, 1.0, highlight);
    let lg = lerp(bg, 1.0, highlight);
    let lb = lerp(bb, 1.0, highlight);
    let alpha = (60.0 + 60.0 * highlight) / 255.0;
    cr.set_source_rgba(lr, lg, lb, alpha);
    cr.set_line_width(1.0 + 0.5 * highlight);
    let _ = cr.stroke_preserve();

    // Hover overlay — white fade-in at alpha 45 * highlight.
    if highlight > 0.0 {
        cr.set_source_rgba(1.0, 1.0, 1.0, 45.0 / 255.0 * highlight);
        let _ = cr.fill();
    } else {
        cr.new_path(); // discard the preserved path
    }

    // Icon position (centre of slice along bisector).
    let icon_angle = (index as f64 * SLICE_DEGREES - 90.0).to_radians();
    let (icon_x, icon_y) = polar(geom.cx, geom.cy, geom.icon_radius, icon_angle);

    // Glow ring on hover.
    if highlight > 0.0 {
        cr.set_source_rgba(1.0, 1.0, 1.0, 40.0 / 255.0 * highlight);
        cr.set_line_width(3.0);
        cr.arc(icon_x, icon_y, ICON_BG_RADIUS + 2.0, 0.0, std::f64::consts::TAU);
        let _ = cr.stroke();
    }

    // Icon background — interpolate surface1 → surface2.
    let (s1r, s1g, s1b, _) = parse_hex_rgba(&palette.surface1).unwrap_or((0.3, 0.3, 0.4, 1.0));
    let (s2r, s2g, s2b, _) = parse_hex_rgba(&palette.surface2).unwrap_or((0.4, 0.4, 0.5, 1.0));
    let bg_r = lerp(s1r, s2r, highlight);
    let bg_g = lerp(s1g, s2g, highlight);
    let bg_b = lerp(s1b, s2b, highlight);
    let bg_a = (230.0 + 25.0 * highlight) / 255.0;
    cr.set_source_rgba(bg_r, bg_g, bg_b, bg_a);
    cr.arc(icon_x, icon_y, ICON_BG_RADIUS, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();

    // Icon glyph — slice colour tint, sized to ~70% of the icon
    // background so it sits inside the disc with a small margin.
    // Falls back to a small placeholder dot when the icon source
    // can't be resolved (lets the user *see* a slice with a typo'd
    // icon name rather than rendering blank).
    let glyph_size = (ICON_BG_RADIUS * 1.4) as i32;
    let glyph_color = icon_color(palette, highlight);
    let icon_source = pick_icon_source(slice);
    if let Some(surface) = icons.resolve(icon_source, glyph_size, glyph_color) {
        draw_icon(cr, icon_x, icon_y, &surface);
    } else {
        cr.set_source_rgba(
            glyph_color.0,
            glyph_color.1,
            glyph_color.2,
            glyph_color.3,
        );
        cr.arc(icon_x, icon_y, ICON_BG_RADIUS * 0.35, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
}

/// Pick the icon source string from a slice — for a typical
/// configuration this is `slice.icon`. Empty strings are normalised
/// so the resolver short-circuits without trying to look up an empty
/// icon name (which the cache would treat as a real key otherwise).
fn pick_icon_source(slice: &Slice) -> &str {
    if slice.icon.is_empty() {
        ""
    } else {
        slice.icon.as_str()
    }
}

/// Render every slice in `slices` at its angular slot. `highlights`
/// is a parallel 8-element array of per-slice hover progress values.
pub fn draw_slices(
    cr: &Context,
    geom: &Geometry,
    slices: &[Slice],
    theme: &ActiveTheme,
    highlights: &[f64; 8],
    icons: &IconCache,
) {
    for (i, slice) in slices.iter().enumerate().take(8) {
        draw_slice(cr, geom, i, slice, theme, highlights[i], icons);
    }
}

/// Centre puck — a small filled circle with an optional label, drawn
/// over the slices. Cairo translation of
/// `overlay/overlay_painting.py::_draw_center` (skeleton only — the
/// dynamic-text bits land in a follow-up commit).
pub fn draw_center(cr: &Context, geom: &Geometry, theme: &ActiveTheme) {
    let palette = &theme.theme.colors;
    let (r, g, b, _) = parse_hex_rgba(&palette.surface0).unwrap_or((0.1, 0.1, 0.15, 1.0));
    cr.set_source_rgba(r, g, b, 220.0 / 255.0);
    cr.arc(geom.cx, geom.cy, geom.center_radius, 0.0, std::f64::consts::TAU);
    let _ = cr.fill_preserve();

    let (br, bg_, bb, _) = parse_hex_rgba(&palette.accent_dim).unwrap_or((0.3, 0.5, 0.7, 1.0));
    cr.set_source_rgba(br, bg_, bb, 140.0 / 255.0);
    cr.set_line_width(2.0);
    let _ = cr.stroke();
}

// =============================================================================
// helpers
// =============================================================================

/// Convert polar (radius, angle) to cartesian, offset from `(cx, cy)`.
fn polar(cx: f64, cy: f64, r: f64, angle_rad: f64) -> (f64, f64) {
    (cx + r * angle_rad.cos(), cy + r * angle_rad.sin())
}

/// Linear interpolation between `a` and `b` at parameter `t ∈ [0, 1]`.
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Icon foreground colour: interpolate subtext1 → text on hover.
fn icon_color(palette: &ThemeColors, highlight: f64) -> (f64, f64, f64, f64) {
    let (a_r, a_g, a_b, _) = parse_hex_rgba(&palette.subtext1).unwrap_or((0.7, 0.7, 0.7, 1.0));
    let (b_r, b_g, b_b, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));
    (
        lerp(a_r, b_r, highlight),
        lerp(a_g, b_g, highlight),
        lerp(a_b, b_b, highlight),
        1.0,
    )
}
