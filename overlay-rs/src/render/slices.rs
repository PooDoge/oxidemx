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

use iced::widget::canvas::{path::Arc, Frame, Path, Stroke};
use iced::{Color, Point, Radians};
use juhradial_shared::theme::{parse_hex_rgba, ThemeColors};
use juhradial_shared::Slice;

const SLICE_DEGREES: f32 = 45.0;

/// Render a single slice. `highlight` is per-slice hover progress in
/// `[0.0, 1.0]`; `slice` carries the user-configured label / colour /
/// icon for slot `index`. `slice = None` means the slot is unused
/// (we still draw an empty wedge so the ring stays visually
/// continuous — same as the Python overlay).
pub fn draw_slice(
    frame: &mut Frame,
    center: Point,
    inner_r: f32,
    outer_r: f32,
    icon_r: f32,
    icon_bg_radius: f32,
    index: usize,
    slice: Option<&Slice>,
    palette: &ThemeColors,
    highlight: f32,
) {
    // Slice angular range in cairo coords (clockwise from +X axis,
    // radians). Python uses `index * 45 - 22.5 - 90` with degrees;
    // the −90 rotates "0° = top" into iced's "0° = right".
    let start_deg = (index as f32) * SLICE_DEGREES - SLICE_DEGREES / 2.0 - 90.0;
    let end_deg = start_deg + SLICE_DEGREES;
    let start_rad = start_deg.to_radians();
    let end_rad = end_deg.to_radians();

    let wedge = build_wedge(center, inner_r, outer_r, start_rad, end_rad);

    // Base fill — surface0 @ alpha 80/255.
    frame.fill(&wedge, rgba(&palette.surface0, 80.0 / 255.0));

    // Stroke — interpolate surface2 → white, alpha 60..120, line
    // width 1.0..1.5.
    let stroke_color = lerp(rgba(&palette.surface2, 1.0), Color::WHITE, highlight);
    let alpha = (60.0 + 60.0 * highlight) / 255.0;
    frame.stroke(
        &wedge,
        Stroke::default()
            .with_color(Color { a: alpha, ..stroke_color })
            .with_width(1.0 + 0.5 * highlight),
    );

    // Hover fade-in.
    if highlight > 0.0 {
        frame.fill(
            &wedge,
            Color::from_rgba(1.0, 1.0, 1.0, 45.0 / 255.0 * highlight),
        );
    }

    // Icon centre on the slice bisector.
    let icon_angle = ((index as f32) * SLICE_DEGREES - 90.0).to_radians();
    let icon_pos = polar(center, icon_r, icon_angle);

    // Glow ring on hover.
    if highlight > 0.0 {
        let glow = Path::circle(icon_pos, icon_bg_radius + 2.0);
        frame.stroke(
            &glow,
            Stroke::default()
                .with_color(Color::from_rgba(
                    1.0, 1.0, 1.0, 40.0 / 255.0 * highlight,
                ))
                .with_width(3.0),
        );
    }

    // Icon background — interpolate surface1 → surface2.
    let s1 = rgba(&palette.surface1, 1.0);
    let s2 = rgba(&palette.surface2, 1.0);
    let bg = lerp(s1, s2, highlight);
    let bg_alpha = (230.0 + 25.0 * highlight) / 255.0;
    frame.fill(
        &Path::circle(icon_pos, icon_bg_radius),
        Color { a: bg_alpha, ..bg },
    );

    // Slice colour placeholder. The icon resolver port comes next
    // — until then, paint a tinted dot in the slot's configured
    // colour so the ring has a visible identity per slot.
    let slot_color_key = slice
        .map(|s| s.color.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("accent");
    let (sr, sg, sb, _) = palette.slice_color_rgba(slot_color_key);
    let dot_color = Color::from_rgb(sr as f32, sg as f32, sb as f32);
    frame.fill(&Path::circle(icon_pos, icon_bg_radius * 0.35), dot_color);
}

/// Centre puck — small filled circle with stroked accent ring,
/// drawn over the slices. Ports `_draw_center` from the Python
/// overlay (centre text overlay lands in a follow-up commit when
/// Pango/iced text rendering is wired in).
pub fn draw_center(
    frame: &mut Frame,
    center: Point,
    radius: f32,
    palette: &ThemeColors,
) {
    let puck = Path::circle(center, radius);
    frame.fill(&puck, rgba(&palette.surface0, 220.0 / 255.0));
    frame.stroke(
        &puck,
        Stroke::default()
            .with_color(rgba(&palette.accent_dim, 140.0 / 255.0))
            .with_width(2.0),
    );
}

// =============================================================================
// helpers
// =============================================================================

fn build_wedge(
    center: Point,
    inner_r: f32,
    outer_r: f32,
    start_rad: f32,
    end_rad: f32,
) -> Path {
    Path::new(|p| {
        let inner_start = polar(center, inner_r, start_rad);
        let outer_start = polar(center, outer_r, start_rad);
        let inner_end = polar(center, inner_r, end_rad);
        p.move_to(inner_start);
        p.line_to(outer_start);
        p.arc(Arc {
            center,
            radius: outer_r,
            start_angle: Radians(start_rad),
            end_angle: Radians(end_rad),
        });
        p.line_to(inner_end);
        p.arc(Arc {
            center,
            radius: inner_r,
            start_angle: Radians(end_rad),
            end_angle: Radians(start_rad),
        });
        p.close();
    })
}

fn polar(center: Point, radius: f32, angle_rad: f32) -> Point {
    Point::new(
        center.x + radius * angle_rad.cos(),
        center.y + radius * angle_rad.sin(),
    )
}

fn rgba(hex: &str, override_alpha: f32) -> Color {
    let (r, g, b, _a) = parse_hex_rgba(hex).unwrap_or((1.0, 1.0, 1.0, 1.0));
    Color::from_rgba(r as f32, g as f32, b as f32, override_alpha)
}

fn lerp(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}
