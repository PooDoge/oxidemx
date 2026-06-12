//! Arced slice-description tooltip along the outer ring.

use iced::widget::canvas::{Frame, Path, Stroke};
use iced::{Point, Radians, Vector};
use oxidemx_shared::theme::ThemeColors;

use super::{arc_line_to, build_wedge, polar, rgba};

/// Styling knobs for `draw_arc_tooltip`. Bundled into a struct so
/// the call site doesn't have to thread eight individual params
/// through the renderer.
pub struct ArcTooltipStyle {
    /// Font used for the text. Pass `iced::Font::MONOSPACE` for
    /// perfectly even arc spacing, or any other family the user
    /// has installed for a more typographic look. The cell-width
    /// estimate stays the same in either case (`0.60 × font_size`)
    /// — proportional fonts will bunch / overlap a little.
    pub font: iced::Font,
    /// Whether `font` is a monospace family. Affects only the
    /// per-character width assumption: monospace = exact, prop =
    /// approximate. Reserved for future per-glyph width tweaks
    /// when proportional fonts are picked.
    #[allow(dead_code)]
    pub monospace: bool,
    /// Foreground (text) colour.
    pub fg: iced::Color,
    /// Background ribbon colour.
    pub bg: iced::Color,
    /// Background ribbon alpha multiplier in [0, 1]. The ribbon's
    /// final alpha is `bg.a * bg_alpha * menu_opacity * tween_alpha`.
    pub bg_alpha: f32,
}

/// Draw a tooltip arced around the outer ring, centred on the
/// hovered slice's bisector. Each char is drawn with a subtle
/// dark shadow underneath + a coloured ribbon background for
/// legibility against the busy radial substrate.
///
/// **Half-flip**: when the slice is in the bottom half of the
/// menu the text would naturally appear upside-down. We detect
/// that case (via the bisector's sin component in canvas coords)
/// and flip both the rotation (chars head-toward-centre instead
/// of head-away) and the character draw order (so reading still
/// proceeds left-to-right visually).
#[allow(clippy::too_many_arguments)]
pub fn draw_arc_tooltip(
    frame: &mut Frame,
    center: Point,
    outer_r: f32,
    slot_index: usize,
    slot_count: usize,
    text: &str,
    palette: &ThemeColors,
    menu_opacity: f32,
    alpha: f32,
    font_size: f32,
    style: ArcTooltipStyle,
) {
    let trimmed = text.trim();
    if trimmed.is_empty() || font_size < 0.5 || alpha <= 0.001 {
        return;
    }
    let n = slot_count.max(1) as f32;
    let slice_degrees = 360.0 / n;
    let bisector_deg = (slot_index as f32) * slice_degrees - 90.0;
    let bisector = bisector_deg.to_radians();
    let flip = bisector.sin() > 0.0;
    let radius = outer_r + font_size * 0.95;

    // Monospace cell width — `0.6 * font_size` is the canonical
    // ratio for most monospace fonts (DejaVu Sans Mono, Liberation
    // Mono, Cascadia, etc. all sit within ±0.05 of this).
    let cell_w = font_size * 0.60;
    let chars: Vec<char> = trimmed.chars().collect();

    // Cap arc width so a runaway description doesn't wrap behind
    // the menu. Truncate with ellipsis when overflow would
    // otherwise push past the wedge + half a wedge each side.
    let max_arc_rad = (slice_degrees * 2.0).to_radians();
    let mut visible: Vec<char> = chars.clone();
    while (visible.len() as f32 * cell_w) / radius > max_arc_rad && visible.len() > 1 {
        visible.pop();
        if visible.last() != Some(&'…') {
            *visible.last_mut().unwrap() = '…';
        }
    }
    let count = visible.len() as f32;
    let step = cell_w / radius;

    // ----- Background ribbon -----
    // Annular sector (ring slice) behind the text so the chars
    // read against a uniform dark backdrop instead of fighting
    // with the wedges + icons + desktop showing through. Span
    // covers all visible chars + horizontal padding on each end.
    // Half-flip-aware: the ribbon's start/end angles match the
    // chars' draw direction so the ribbon and text agree on
    // which edge is "left".
    //
    // Padding values:
    // - `pad_rad` ≈ one full character cell on each end. iced's
    //   text-bounding-box width approximates the glyph but the
    //   visible-character ends don't reach the corners, so a
    //   full cell of padding gives a comfortable margin.
    // - Radial thickness is **asymmetric** AND **flip-aware**.
    //   iced's text bounding box already pads the cap-height +
    //   leading on the glyph TOP, so we only need a lean
    //   margin on whichever geometric side coincides with the
    //   reader's "top of text". Half-flip swaps which side that
    //   is: top-half slices have visual top = outer (away from
    //   menu centre); bottom-half slices flip the chars so
    //   visual top = inner (toward menu centre).
    //     cap_pad = 0.55 * font_size  ← lean: bbox already pads
    //     base_pad = 0.85 * font_size  ← generous: descender room
    let mo = menu_opacity.clamp(0.0, 1.0);
    let pad_rad = step * 1.0;
    let half_arc = (count * step) / 2.0 + pad_rad;
    let ribbon_start = bisector - half_arc;
    let ribbon_end = bisector + half_arc;
    let cap_pad = font_size * 0.55;
    let base_pad = font_size * 0.85;
    let (inner_pad, outer_pad) = if flip {
        // Bottom-half slices: chars are flipped so glyph top
        // points inward → cap-padding sits on the inner edge.
        (cap_pad, base_pad)
    } else {
        // Top-half slices: glyph top points outward.
        (base_pad, cap_pad)
    };
    let ribbon_inner = radius - inner_pad;
    let ribbon_outer = radius + outer_pad;
    // Round the OUTER corners (geometrically furthest from menu
    // centre — see `build_tooltip_ribbon`). Visually these are
    // the "label corners" while the inner pair tucks against the
    // menu's outer ring and reads better when sharp. ~0.35 of
    // font_size (~3.85 px at fs=11) is a tasteful softening; the
    // ribbon function caps it if the geometry is too tight.
    let corner_px = font_size * 0.35;
    let ribbon = build_tooltip_ribbon(
        center,
        ribbon_inner,
        ribbon_outer,
        ribbon_start,
        ribbon_end,
        corner_px,
    );
    // Honour user-configured background colour + alpha.
    let bg_alpha_eff = style.bg_alpha.clamp(0.0, 1.0) * mo * alpha;
    if bg_alpha_eff > 0.001 {
        frame.fill(
            &ribbon,
            iced::Color {
                a: style.bg.a * bg_alpha_eff,
                ..style.bg
            },
        );
        // Thin accent_dim stroke gives the ribbon a defined edge.
        // Scaled by the same effective alpha so the stroke fades
        // out alongside the fill.
        frame.stroke(
            &ribbon,
            Stroke::default()
                .with_color(rgba(&palette.accent_dim, 0.5 * bg_alpha_eff))
                .with_width(1.0),
        );
    }

    let fg = iced::Color {
        a: style.fg.a * mo * alpha,
        ..style.fg
    };
    // Text shadow removed — the ribbon background already
    // provides enough contrast for the glyphs to read against
    // the busy radial substrate. Adding a drop shadow on top
    // of the ribbon was redundant and slightly muddied the
    // text edges.

    for (visual_index, ch) in visible.iter().enumerate() {
        // Centre the run on the bisector. visual_index 0 is the
        // leftmost reading character.
        let centered = visual_index as f32 - (count - 1.0) / 2.0;
        let angle = if flip {
            bisector - centered * step
        } else {
            bisector + centered * step
        };
        let pos = Point::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        );
        let tangent = if flip {
            angle - std::f32::consts::FRAC_PI_2
        } else {
            angle + std::f32::consts::FRAC_PI_2
        };

        let s: String = ch.to_string();
        let draw = |f: &mut Frame, color: iced::Color, dx: f32, dy: f32| {
            f.fill_text(iced::widget::canvas::Text {
                content: s.clone(),
                position: Point::new(-cell_w / 2.0 + dx, -font_size / 2.0 + dy),
                color,
                size: font_size.into(),
                font: style.font,
                ..iced::widget::canvas::Text::default()
            });
        };
        frame.with_save(|f| {
            f.translate(Vector::new(pos.x, pos.y));
            f.rotate(Radians(tangent));
            draw(f, fg, 0.0, 0.0);
        });
    }
}

/// Annular sector with rounded *outer* corners only — the inner
/// corners (closest to the menu centre) stay sharp. Used by the
/// arced tooltip ribbon so its outer edge feels like a softened
/// label background while the inner edge tucks neatly against
/// the menu's outer ring.
///
/// Geometry: the outer corner is rounded with `corner_px` of
/// arc-length, achieved by insetting both radially (start at
/// `outer_r - corner_px` along the radial edge) and angularly
/// (resume the outer arc at `start_rad + corner_px / outer_r`).
/// A single quadratic Bézier with the sharp-corner point as the
/// control point bridges the two — close enough to a quarter
/// circle for the small radii we use here without the cost of
/// a cubic curve or arc-approximation.
fn build_tooltip_ribbon(
    center: Point,
    inner_r: f32,
    outer_r: f32,
    start_rad: f32,
    end_rad: f32,
    corner_px: f32,
) -> Path {
    // Cap the corner radius so it doesn't eat the whole ribbon
    // when the angular sweep is small or the radial thickness is
    // thin. Half of either dimension is the geometric upper
    // bound; we go a little tighter to keep the curve visibly
    // rounded rather than degenerate.
    let radial_thickness = (outer_r - inner_r).max(0.0);
    let arc_length = (end_rad - start_rad).abs() * outer_r.max(1.0);
    let max_corner = (radial_thickness * 0.45).min(arc_length * 0.4);
    let r = corner_px.max(0.0).min(max_corner.max(0.0));
    if r < 0.5 {
        // Corner radius too small to render visibly — fall back
        // to a plain wedge to avoid degenerate Bézier control
        // points.
        return build_wedge(center, inner_r, outer_r, start_rad, end_rad);
    }
    let angular_inset = r / outer_r.max(1.0);

    Path::new(|p| {
        let inner_start = polar(center, inner_r, start_rad);
        let inner_end = polar(center, inner_r, end_rad);
        // 1. Inner-start → up along the start-side radial to
        //    where the rounded corner begins (inset radially).
        p.move_to(inner_start);
        p.line_to(polar(center, outer_r - r, start_rad));
        // 2. Quadratic-curve the outer-start corner. Control
        //    point is the sharp original corner; endpoint is on
        //    the outer arc, inset angularly.
        let ctrl_start = polar(center, outer_r, start_rad);
        let outer_arc_in = polar(center, outer_r, start_rad + angular_inset);
        p.quadratic_curve_to(ctrl_start, outer_arc_in);
        // 3. Outer arc proper — from (start + inset) to (end - inset).
        arc_line_to(
            p,
            center,
            outer_r,
            start_rad + angular_inset,
            end_rad - angular_inset,
        );
        // 4. Quadratic-curve the outer-end corner. Endpoint is
        //    inset radially from outer.
        let ctrl_end = polar(center, outer_r, end_rad);
        let outer_arc_out = polar(center, outer_r - r, end_rad);
        p.quadratic_curve_to(ctrl_end, outer_arc_out);
        // 5. Radial line down to inner-end.
        p.line_to(inner_end);
        // 6. Inner arc back from end → start (sharp corners by
        //    spec — only the OUTER corners are rounded).
        arc_line_to(p, center, inner_r, end_rad, start_rad);
        p.close();
    })
}
