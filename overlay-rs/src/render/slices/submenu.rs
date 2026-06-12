//! Submenu pop-out arc rendering (`draw_submenu`).

use iced::widget::canvas::{Frame, Path, Stroke};
use iced::{Color, Point};
use oxidemx_shared::theme::ThemeColors;
use oxidemx_shared::{ElementAnimation, Slice};

use crate::anim;
use crate::radial::{
    SubmenuState, SUBITEM_RENDER_RADIUS, SUBITEM_RENDER_SPREAD_DEG, SUBMENU_RADIUS,
};
use crate::render::icons::{draw_icon, IconCache};

use super::{rgba, GLYPH_RASTER_PX};

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
#[allow(clippy::too_many_arguments)] // mirrors the painter's full submenu parameter surface
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
        let allowed = item.visible_if.as_ref().map(|c| c.eval()).unwrap_or(true);
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
                    .with_color(Color::from_rgba(1.0, 1.0, 1.0, 60.0 / 255.0 * item_opacity))
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
            Stroke::default().with_color(border_color).with_width(1.5),
        );

        let color_key = if !item.color.is_empty() {
            item.color.as_str()
        } else if !parent.color.is_empty() {
            parent.color.as_str()
        } else {
            "accent"
        };
        let (sr, sg, sb, _) = palette.slice_color_rgba(color_key);
        // Full-alpha tint — see icon_color_rgba above; draw_icon
        // applies item_opacity at draw time.
        let icon_color = (sr as f32, sg as f32, sb as f32, 1.0);
        let glyph_size = (scaled_radius * 1.4).max(6.0);
        let glyph_size_px = GLYPH_RASTER_PX;
        let resolved = if item.icon_untinted {
            icons.resolve_untinted(item.icon.as_str(), glyph_size_px)
        } else {
            icons.resolve(item.icon.as_str(), glyph_size_px, icon_color)
        };
        if let Some(handle) = resolved {
            draw_icon(
                frame,
                item_pos.x,
                item_pos.y,
                glyph_size,
                &handle,
                item_opacity,
            );
        } else {
            frame.fill(
                &Path::circle(item_pos, scaled_radius * 0.35),
                Color::from_rgba(sr as f32, sg as f32, sb as f32, item_opacity),
            );
        }
    }
}
