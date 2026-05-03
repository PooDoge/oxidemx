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
use juhradial_shared::{ElementAnimation, Slice};

use crate::anim;
use crate::radial::{
    SubmenuState, SUBITEM_RENDER_RADIUS, SUBITEM_RENDER_SPREAD_DEG, SUBMENU_RADIUS,
};
use crate::render::icons::{draw_icon, IconCache};

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
    menu_opacity: f32,
    bg_opacity: f32,
    highlight_opacity: f32,
    icons: &IconCache,
) {
    // Slice angular range in cairo coords (clockwise from +X axis,
    // radians). Python uses `index * 45 - 22.5 - 90` with degrees;
    // the −90 rotates "0° = top" into iced's "0° = right".
    let start_deg = (index as f32) * SLICE_DEGREES - SLICE_DEGREES / 2.0 - 90.0;
    let end_deg = start_deg + SLICE_DEGREES;
    let start_rad = start_deg.to_radians();
    let end_rad = end_deg.to_radians();

    let wedge = build_wedge(center, inner_r, outer_r, start_rad, end_rad);
    let mo = menu_opacity.clamp(0.0, 1.0);
    let bgo = bg_opacity.clamp(0.0, 1.0);
    let hlo = highlight_opacity.clamp(0.0, 1.0);
    // The user's hover-glow modulation only applies to the
    // *highlight* portion of the slice (the stroke brightness
    // delta, the hover wash, the glow ring). The base wedge fill
    // tracks `bg_opacity` so users can darken the wheel substrate
    // without losing the hover feedback.
    let hl = highlight * hlo;

    // Base fill — surface0 @ alpha 80/255.
    frame.fill(&wedge, rgba(&palette.surface0, (80.0 / 255.0) * mo * bgo));

    // Stroke — interpolate surface2 → white, alpha 60..120, line
    // width 1.0..1.5.
    let stroke_color = lerp(rgba(&palette.surface2, 1.0), Color::WHITE, hl);
    let alpha = ((60.0 + 60.0 * hl) / 255.0) * mo;
    frame.stroke(
        &wedge,
        Stroke::default()
            .with_color(Color { a: alpha, ..stroke_color })
            .with_width(1.0 + 0.5 * hl),
    );

    // Hover fade-in.
    if hl > 0.0 {
        frame.fill(
            &wedge,
            Color::from_rgba(1.0, 1.0, 1.0, (45.0 / 255.0) * hl * mo),
        );
    }

    // Icon centre on the slice bisector.
    let icon_angle = ((index as f32) * SLICE_DEGREES - 90.0).to_radians();
    let icon_pos = polar(center, icon_r, icon_angle);

    // Glow ring on hover.
    if hl > 0.0 {
        let glow = Path::circle(icon_pos, icon_bg_radius + 2.0);
        frame.stroke(
            &glow,
            Stroke::default()
                .with_color(Color::from_rgba(
                    1.0, 1.0, 1.0, (40.0 / 255.0) * hl * mo,
                ))
                .with_width(3.0),
        );
    }

    // Icon background — interpolate surface1 → surface2.
    let s1 = rgba(&palette.surface1, 1.0);
    let s2 = rgba(&palette.surface2, 1.0);
    let bg = lerp(s1, s2, hl);
    let bg_alpha = ((230.0 + 25.0 * hl) / 255.0) * mo;
    frame.fill(
        &Path::circle(icon_pos, icon_bg_radius),
        Color { a: bg_alpha, ..bg },
    );

    // Icon colour for the slot — uses the configured slice color
    // (e.g. "green", "sapphire") looked up in the active palette.
    let slot_color_key = slice
        .map(|s| s.color.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("accent");
    let (sr, sg, sb, _) = palette.slice_color_rgba(slot_color_key);
    let icon_color_rgba = (sr as f32, sg as f32, sb as f32, mo);

    // Try to load + tint the slice's configured icon. On miss
    // (icon name not in any theme dir, file load failure, etc.),
    // fall back to a placeholder dot in the slice colour so the
    // ring still has a visible identity.
    let icon_source = slice.map(|s| s.icon.as_str()).unwrap_or("");
    let glyph_size = (icon_bg_radius * 1.4).max(8.0);
    let glyph_size_px = glyph_size.round() as u32;

    if let Some(handle) = icons.resolve(icon_source, glyph_size_px, icon_color_rgba) {
        draw_icon(frame, icon_pos.x, icon_pos.y, glyph_size, &handle);
    } else {
        let dot_color = Color::from_rgba(sr as f32, sg as f32, sb as f32, mo);
        frame.fill(&Path::circle(icon_pos, icon_bg_radius * 0.35), dot_color);
    }
}

/// Render the submenu pop-out arc for an open `SubmenuState`. Each
/// sub-item gets its own staggered grow + fade-in animation driven
/// off `submenu.progress.current` (0.0 → 1.0). Sub-items are
/// arranged on an arc beyond the main ring, centred on the parent
/// slice's bisector, with `SUBITEM_RENDER_SPREAD_DEG` between
/// adjacent items.
///
/// Visual ports of `_draw_submenu` from the legacy Python overlay:
///   * Per-item stagger: each item starts `STAGGER` seconds after
///     the previous, in normalised progress units.
///   * `ease_out_back` overshoot for the radius interpolation —
///     sub-items pop out past their final radius, then settle.
///   * Linear scale 0.5 → 1.0 over the same item-local progress
///     window.
///   * Faster fade-in (item_t × 2.5) so items are readable while
///     still travelling.
///   * Highlighted item gets a glow ring + brighter background.
pub fn draw_submenu(
    frame: &mut Frame,
    center: Point,
    submenu: &SubmenuState,
    slices: &[Slice],
    palette: &ThemeColors,
    submenu_anim: &ElementAnimation,
    menu_opacity: f32,
    icons: &IconCache,
) {
    let parent = match slices.get(submenu.parent) {
        Some(p) => p,
        None => return,
    };
    let items = &parent.submenu;
    if items.is_empty() {
        return;
    }
    let n = items.len() as f32;
    let parent_angle_deg = (submenu.parent as f32) * 45.0 - 90.0;
    let mo = menu_opacity.clamp(0.0, 1.0);

    // Per-item stagger comes from the user's chain config; 0 means
    // all sub-items animate simultaneously. The renderer uses the
    // master tween's elapsed clock + the per-item offset to derive
    // each item's Visual, so users can dial the ripple feel from
    // the settings UI.
    let stagger_ms = anim::chain_stagger_ms(submenu_anim.chain.as_ref());

    for (i, item) in items.iter().enumerate() {
        let allowed = item
            .visible_if
            .as_ref()
            .map(|c| c.eval())
            .unwrap_or(true);
        if !allowed {
            continue;
        }

        let offset = (i as f32) * stagger_ms;
        let v = anim::evaluate_chain_item(
            &submenu.progress,
            &submenu_anim.enter,
            &submenu_anim.exit,
            offset,
        );

        // Radius interpolates from the ring edge out to the
        // submenu position — the "growing out of the wedge" feel.
        let menu_r = crate::geometry::MENU_RADIUS as f32;
        let anim_radius = menu_r + (SUBMENU_RADIUS - menu_r) * v.progress.clamp(0.0, 1.0);

        let item_opacity = (v.opacity * mo).clamp(0.0, 1.0);
        let scaled_radius = SUBITEM_RENDER_RADIUS * v.scale.max(0.0);
        let is_highlighted = submenu.highlighted == Some(i);

        let offset_deg = (i as f32 - (n - 1.0) / 2.0) * SUBITEM_RENDER_SPREAD_DEG;
        let item_angle = (parent_angle_deg + offset_deg).to_radians();
        let item_pos = Point::new(
            center.x + anim_radius * item_angle.cos(),
            center.y + anim_radius * item_angle.sin(),
        );

        // Drop shadow — slight south-east offset.
        frame.fill(
            &Path::circle(
                Point::new(item_pos.x + 2.0, item_pos.y + 3.0),
                scaled_radius,
            ),
            Color::from_rgba(0.0, 0.0, 0.0, 80.0 / 255.0 * item_opacity),
        );

        // Glow ring on hover.
        if is_highlighted {
            frame.stroke(
                &Path::circle(item_pos, scaled_radius + 3.0),
                Stroke::default()
                    .with_color(Color::from_rgba(
                        1.0, 1.0, 1.0, 60.0 / 255.0 * item_opacity,
                    ))
                    .with_width(3.0),
            );
        }

        let bg_hex = if is_highlighted {
            &palette.surface2
        } else {
            &palette.surface1
        };
        let bg_alpha = if is_highlighted { 1.0 } else { 240.0 / 255.0 };
        let bg = rgba(bg_hex, bg_alpha * item_opacity);
        frame.fill(&Path::circle(item_pos, scaled_radius), bg);

        let border_color = if is_highlighted {
            Color::from_rgba(1.0, 1.0, 1.0, 150.0 / 255.0 * item_opacity)
        } else {
            rgba(&palette.surface2, item_opacity)
        };
        frame.stroke(
            &Path::circle(item_pos, scaled_radius),
            Stroke::default()
                .with_color(border_color)
                .with_width(1.5),
        );

        let color_key = if !item.color.is_empty() {
            item.color.as_str()
        } else if !parent.color.is_empty() {
            parent.color.as_str()
        } else {
            "accent"
        };
        let (sr, sg, sb, _) = palette.slice_color_rgba(color_key);
        let icon_color = (sr as f32, sg as f32, sb as f32, item_opacity);
        let glyph_size = (scaled_radius * 1.4).max(6.0);
        let glyph_size_px = glyph_size.round().max(1.0) as u32;
        if let Some(handle) = icons.resolve(item.icon.as_str(), glyph_size_px, icon_color) {
            draw_icon(frame, item_pos.x, item_pos.y, glyph_size, &handle);
        } else {
            frame.fill(
                &Path::circle(item_pos, scaled_radius * 0.35),
                Color::from_rgba(sr as f32, sg as f32, sb as f32, item_opacity),
            );
        }
    }
}

/// Centre puck — small filled circle with stroked accent ring,
/// optionally with a label drawn inside (the hovered slice's name).
/// Ports `_draw_center` from the Python overlay; the centre-text
/// rendering is the long-promised "/* text overlay lands in a
/// follow-up */" finally landing here.
pub fn draw_center(
    frame: &mut Frame,
    center: Point,
    radius: f32,
    palette: &ThemeColors,
    menu_opacity: f32,
    bg_opacity: f32,
    label: Option<&str>,
    label_size: f32,
) {
    let mo = menu_opacity.clamp(0.0, 1.0);
    let bgo = bg_opacity.clamp(0.0, 1.0);
    let puck = Path::circle(center, radius);
    frame.fill(&puck, rgba(&palette.surface0, (220.0 / 255.0) * mo * bgo));
    frame.stroke(
        &puck,
        Stroke::default()
            .with_color(rgba(&palette.accent_dim, (140.0 / 255.0) * mo * bgo))
            .with_width(2.0),
    );

    if let Some(text) = label {
        if !text.is_empty() {
            // Truncate aggressively so long labels don't run off the
            // puck. The puck is ~90 px in diameter at the default
            // CENTER_ZONE_RADIUS=45, so ~10 chars max keeps
            // everything inside.
            let max_chars = ((radius * 2.0 / (label_size * 0.55)).max(4.0)) as usize;
            let display: String = if text.chars().count() > max_chars {
                let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
                out.push('…');
                out
            } else {
                text.to_string()
            };
            let approx_w = display.chars().count() as f32 * label_size * 0.55;
            let (tr, tg, tb, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));
            frame.fill_text(iced::widget::canvas::Text {
                content: display,
                position: iced::Point::new(
                    center.x - approx_w / 2.0,
                    center.y - label_size / 2.0,
                ),
                color: iced::Color::from_rgba(tr as f32, tg as f32, tb as f32, mo),
                size: label_size.into(),
                ..iced::widget::canvas::Text::default()
            });
        }
    }
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
