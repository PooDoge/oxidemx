//! Slice rendering on iced's canvas Frame.
//!
//! Port of `overlay/overlay_painting.py::_draw_slice` plus the cairo
//! version that lived here previously. Same visual behaviour — the
//! Path API differs but the geometry is identical:
//!   * donut wedge (45° each, 8 slices)
//!   * surface0-tinted base fill
//!   * surface2 → white interpolated stroke on hover
//!   * white fade-in fill on hover
//!   * glow ring + icon-background disc on the slice's bisector
//!
//! Icon glyph composition lives in `super::icons` and is called from
//! here once the icon resolver picks up its first compile-clean
//! iced surface API.

use iced::widget::canvas::{self, Path};
use iced::{Color, Point};
use oxidemx_shared::theme::parse_hex_rgba;

mod center;
mod ring;
mod submenu;
mod tooltip;
mod widgets;

pub use center::{
    draw_center, draw_page_indicator, draw_page_name_transition, draw_puck, PuckRing,
};
pub use ring::draw_ring_transformed;
pub use submenu::draw_submenu;
pub use tooltip::{draw_arc_tooltip, ArcTooltipStyle};
pub use widgets::draw_weather_popup;

/// Fixed rasterization size for slice icons (ICON_BG_RADIUS × 1.4
/// at rest scale). Icons are always rasterized at THIS size and the
/// canvas scales the bitmap to the animated draw size — resolving
/// at the scaled size instead re-rasterizes every SVG through resvg
/// on every frame of any scale animation (menu-open spring, page
/// transitions, submenu pop), which is exactly the page-switch lag.
pub(super) const GLYPH_RASTER_PX: u32 = 36;

/// Default wedge sweep for the legacy 8-slot ring. Kept for the
/// submenu pop-out which still uses this for parent-bisector
/// math; the main ring computes its sweep from the active page's
/// `slot_count` at render time.
#[allow(dead_code)]
const SLICE_DEGREES: f32 = 45.0;

/// Build a donut wedge (annular sector) as a single continuous
/// closed sub-path.
///
/// **Why we don't use `p.arc(...)`**: iced's canvas
/// `path::Builder::arc` internally calls `move_to(arc.start)` (see
/// `ellipse()` in iced_graphics::geometry::path::builder), which
/// terminates the current sub-path and starts a new one. Mixing
/// `line_to` + `arc` in the same path therefore produces several
/// *disconnected* sub-paths — when filled with α<1 they
/// double-fill at overlaps, creating dark triangular patches and
/// odd cuts across the shape.
///
/// We manually subdivide each arc into short `line_to` segments
/// so the whole wedge stays in one sub-path. Step size scales
/// with the arc's radius so a tiny inner arc doesn't waste
/// segments and a large outer arc still looks smooth.
/// Append `line_to` points along an arc from `from` to `to` at
/// `radius`, picking enough samples that the chord error stays
/// under ~0.5 px. `from > to` is fine — we always step from
/// `from` toward `to` regardless of direction. Module-private
/// helper shared by `build_wedge` and `build_tooltip_ribbon`.
pub(super) fn arc_line_to(
    p: &mut canvas::path::Builder,
    center: Point,
    radius: f32,
    from: f32,
    to: f32,
) {
    let sweep = (to - from).abs();
    if sweep < 1e-4 || radius < 0.5 {
        return;
    }
    let max_step = (4.0_f32 / radius.max(1.0)).sqrt().max(0.05);
    let segments = ((sweep / max_step).ceil() as usize).max(12);
    for i in 1..=segments {
        let t = i as f32 / segments as f32;
        let a = from + (to - from) * t;
        p.line_to(polar(center, radius, a));
    }
}

pub(super) fn build_wedge(
    center: Point,
    inner_r: f32,
    outer_r: f32,
    start_rad: f32,
    end_rad: f32,
) -> Path {
    Path::new(|p| {
        let inner_start = polar(center, inner_r, start_rad);
        let outer_start = polar(center, outer_r, start_rad);
        // Walk the wedge boundary as one continuous sub-path:
        //   inner_start → outer_start (radial line, start side)
        //   outer arc start_rad → end_rad
        //   outer_end → inner_end (radial line, end side)
        //   inner arc end_rad → start_rad (back the other way)
        p.move_to(inner_start);
        p.line_to(outer_start);
        arc_line_to(p, center, outer_r, start_rad, end_rad);
        p.line_to(polar(center, inner_r, end_rad));
        arc_line_to(p, center, inner_r, end_rad, start_rad);
        p.close();
    })
}

pub(super) fn polar(center: Point, radius: f32, angle_rad: f32) -> Point {
    Point::new(
        center.x + radius * angle_rad.cos(),
        center.y + radius * angle_rad.sin(),
    )
}

pub(super) fn rgba(hex: &str, override_alpha: f32) -> Color {
    let (r, g, b, _a) = parse_hex_rgba(hex).unwrap_or((1.0, 1.0, 1.0, 1.0));
    Color::from_rgba(r as f32, g as f32, b as f32, override_alpha)
}

pub(super) fn lerp(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}
