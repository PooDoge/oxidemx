//! Ring + wedge rendering: `draw_ring_transformed`, `draw_slice`.

use iced::widget::canvas::{Frame, Path, Stroke};
use iced::{Color, Point};
use oxidemx_shared::theme::{parse_hex_rgba, ThemeColors};
use oxidemx_shared::{ComposedTransform, Slice};

use crate::render::icons::{draw_icon, IconCache};

use super::widgets::{
    draw_centered_text, draw_custom_fallback, draw_custom_widget, draw_widget_wedge, CustomWidgets,
};
use super::{build_wedge, lerp, polar, rgba, GLYPH_RASTER_PX};

/// Render an entire 8-slot ring of slices with an optional uniform
/// rotation + scale around the centre. The transform is applied via
/// iced's canvas matrix stack so wedges, icon backgrounds, and icon
/// glyphs all rotate/scale together. `extra_rotation_rad == 0.0`
/// and `scale == 1.0` reduce to the plain single-ring path with
/// just one save/restore — the page-transition code uses this to
/// double-render an outgoing + incoming ring during a scroll-cycle.
///
/// `slices` may be shorter than 8; missing slots render as empty
/// wedges (same as `draw_slice` with `slice = None`).
/// `highlights` is the per-slot hover progress, mirrors the
/// `RadialState::highlights` array layout — pass `&[0.0; 8]` when
/// the ring shouldn't show any hover state (e.g. the outgoing
/// ring during a transition).
/// `wedge_fill_mul` scales the alpha of the canvas-painted wedge
/// fill, stroke, and hover wash — leaves icons + icon-glow rings
/// untouched. The SDF wedge spike pipes `1.0 - sdf_intensity`
/// through here so the canvas wedges fade out as the SDF layer
/// fades in. `1.0` (the default at every existing call site) is
/// the historic full-strength canvas behaviour.
///
/// `ring_transform` carries any per-ring translate/rotate/scale/
/// flip composed for this draw — used by the page-cycle
/// transition (incoming ring uses Enter, outgoing uses Exit) and
/// by future custom-track-driven preset paths. Pass
/// `&ComposedTransform::IDENTITY` for the no-transform case.
///
/// `slot_transforms` carries optional per-slot transforms applied
/// AROUND each slice's icon centre (useful for slice-highlight
/// custom tracks). `None` skips the per-slot wrapping entirely.
#[allow(clippy::too_many_arguments)]
pub fn draw_ring_transformed(
    frame: &mut Frame,
    center: Point,
    inner_r: f32,
    outer_r: f32,
    icon_r: f32,
    icon_bg_radius: f32,
    slices: &[Slice],
    palette: &ThemeColors,
    highlights: &[f32; 8],
    menu_opacity: f32,
    bg_opacity: f32,
    highlight_opacity: f32,
    icons: &IconCache,
    ring_transform: &ComposedTransform,
    slot_transforms: Option<&[ComposedTransform; 8]>,
    slot_count: usize,
    wedge_fill_mul: f32,
    widgets: &crate::radial::WidgetData,
    custom: &CustomWidgets<'_>,
) {
    let n = slot_count.clamp(2, 8);
    frame.with_save(|f| {
        // Apply the ring-level composed transform around the menu
        // centre (translate + rotate + flip-scale + uniform-scale
        // are all baked into `ring_transform.scale`-multiplied
        // radii via `mscale` upstream; this call applies the
        // remaining channels). Identity transform = no-op.
        crate::render::animation::apply_composed_transform(f, center, ring_transform);
        let ring_alpha = ring_transform.alpha.clamp(0.0, 1.0);
        for i in 0..n {
            let highlight = highlights.get(i).copied().unwrap_or(0.0);
            let slice_for_render = slices
                .get(i)
                .filter(|s| s.visible_if.as_ref().map(|c| c.eval()).unwrap_or(true));
            // Per-slot transform: wrap slice draw in with_save so
            // the per-slot transform doesn't leak across to the
            // next slot's draw_slice call. Skipped when
            // slot_transforms is None.
            let slot_t = slot_transforms.and_then(|s| s.get(i));
            let needs_slot_save = slot_t.is_some();
            // icon centre — needed both for the slot-pivot and as
            // the slice-internal computation. Keep it here so the
            // slot transform can pivot around it.
            let n_f = n as f32;
            let slice_degrees = 360.0 / n_f;
            let icon_angle = ((i as f32) * slice_degrees - 90.0).to_radians();
            let icon_pos = polar(center, icon_r, icon_angle);

            let draw = |fr: &mut Frame| {
                draw_slice(
                    fr,
                    center,
                    inner_r,
                    outer_r,
                    icon_r,
                    icon_bg_radius,
                    i,
                    slice_for_render,
                    palette,
                    highlight,
                    menu_opacity * ring_alpha,
                    bg_opacity,
                    highlight_opacity,
                    icons,
                    n,
                    wedge_fill_mul,
                    widgets,
                    custom,
                );
            };

            if needs_slot_save {
                f.with_save(|fr| {
                    if let Some(t) = slot_t {
                        crate::render::animation::apply_composed_transform(fr, icon_pos, t);
                    }
                    draw(fr);
                });
            } else {
                draw(f);
            }
        }
    });
}

/// Render a single slice. `highlight` is per-slice hover progress in
/// `[0.0, 1.0]`; `slice` carries the user-configured label / colour /
/// icon for slot `index`. `slice = None` means the slot is unused
/// (we still draw an empty wedge so the ring stays visually
/// continuous — same as the Python overlay).
#[allow(clippy::too_many_arguments)] // mirrors the painter's full per-wedge parameter surface
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
    slot_count: usize,
    wedge_fill_mul: f32,
    widgets: &crate::radial::WidgetData,
    custom: &CustomWidgets<'_>,
) {
    // Wedge sweep — 360° / slot_count. The legacy 8-slot ring
    // hits 45°; a 4-slot ring uses 90° per wedge, etc. Half-sweep
    // shift centres slot 0 on 12 o'clock.
    let n = slot_count.max(1) as f32;
    let slice_degrees = 360.0 / n;

    let mo = menu_opacity.clamp(0.0, 1.0);
    let bgo = bg_opacity.clamp(0.0, 1.0);
    let hlo = highlight_opacity.clamp(0.0, 1.0);
    // The user's hover-glow modulation only applies to the
    // *highlight* portion of the slice (the stroke brightness
    // delta, the hover wash, the glow ring). The base wedge fill
    // tracks `bg_opacity` so users can darken the wheel substrate
    // without losing the hover feedback.
    let hl = highlight * hlo;
    let wfm = wedge_fill_mul.clamp(0.0, 1.0);

    // Icon centre on the slice bisector — needed by both the
    // wedge-decoration block (icon bg disc, hover glow) and the
    // icon-glyph block below, so compute it before the gate.
    let icon_angle = ((index as f32) * slice_degrees - 90.0).to_radians();
    let icon_pos = polar(center, icon_r, icon_angle);

    // ---- Wedge geometry block ----
    // When the SDF spike has fully taken over (`wfm` near zero)
    // we skip every wedge-related Path construction + frame
    // call. The output alpha would be ≤ 2 % anyway and the
    // tessellation work is the dominant cost during a multi-
    // shader frame. This is what users notice as "lag during
    // menu open" when the SDF is at full intensity.
    if wfm > 0.02 {
        let start_deg = (index as f32) * slice_degrees - slice_degrees / 2.0 - 90.0;
        let end_deg = start_deg + slice_degrees;
        let start_rad = start_deg.to_radians();
        let end_rad = end_deg.to_radians();
        let wedge = build_wedge(center, inner_r, outer_r, start_rad, end_rad);

        // Base wedge fill — surface0 with alpha driven directly
        // by the user's "menu background opacity" slider.
        frame.fill(&wedge, rgba(&palette.surface0, mo * bgo * wfm));

        // Resolve the active accent once — every hover-driven
        // highlight (stroke, wash, icon glow) interpolates
        // toward this so the feedback colour follows the
        // active theme instead of falling back to a hard-coded
        // white wash.
        let accent_color = rgba(&palette.accent, 1.0);

        // Stroke — interpolate surface2 → accent, alpha 60..150,
        // line width 1.0..1.5.
        let stroke_color = lerp(rgba(&palette.surface2, 1.0), accent_color, hl);
        let alpha = ((60.0 + 90.0 * hl) / 255.0) * mo * wfm;
        frame.stroke(
            &wedge,
            Stroke::default()
                .with_color(Color {
                    a: alpha,
                    ..stroke_color
                })
                .with_width(1.0 + 0.5 * hl),
        );

        // Hover fade-in — wedge wash in the accent colour.
        if hl > 0.0 {
            frame.fill(
                &wedge,
                Color {
                    a: (70.0 / 255.0) * hl * mo * wfm,
                    ..accent_color
                },
            );
        }

        // Widget wedges paint big-value typography instead of the
        // icon disc — skip the icon furniture for them.
        let is_widget = slice
            .map(|s| matches!(s.kind, oxidemx_shared::ActionKind::Widget))
            .unwrap_or(false);

        // Glow ring on hover — accent halo around the icon disc.
        if hl > 0.0 && !is_widget {
            let glow = Path::circle(icon_pos, icon_bg_radius + 2.0);
            frame.stroke(
                &glow,
                Stroke::default()
                    .with_color(Color {
                        a: (90.0 / 255.0) * hl * mo * wfm,
                        ..accent_color
                    })
                    .with_width(3.0),
            );
        }

        // Icon background — interpolate surface1 → surface2.
        if !is_widget {
            let s1 = rgba(&palette.surface1, 1.0);
            let s2 = rgba(&palette.surface2, 1.0);
            let bg = lerp(s1, s2, hl);
            let bg_alpha = ((230.0 + 25.0 * hl) / 255.0) * mo * wfm;
            frame.fill(
                &Path::circle(icon_pos, icon_bg_radius),
                Color { a: bg_alpha, ..bg },
            );
        }
    }
    // ---- end wedge geometry block ----

    // Icon colour for the slot — uses the configured slice color
    // (e.g. "green", "sapphire") looked up in the active palette.
    let slot_color_key = slice
        .map(|s| s.color.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("accent");
    let (sr, sg, sb, _) = palette.slice_color_rgba(slot_color_key);
    // Full-alpha tint: the animated opacity is applied by draw_icon,
    // not baked into the raster — alpha is part of the cache key and
    // a per-frame alpha would re-rasterize on every fade frame.
    let icon_color_rgba = (sr as f32, sg as f32, sb as f32, 1.0);

    // Try to load + tint the slice's configured icon. On miss
    // (icon name not in any theme dir, file load failure, etc.),
    // fall back to a placeholder dot in the slice colour so the
    // ring still has a visible identity.
    let icon_source = slice.map(|s| s.icon.as_str()).unwrap_or("");
    let glyph_size = (icon_bg_radius * 1.4).max(8.0);
    let glyph_size_px = GLYPH_RASTER_PX;

    // Widget wedges replace the icon block entirely with their
    // live-data typography.
    if let Some(s) = slice {
        if matches!(s.kind, oxidemx_shared::ActionKind::Widget) {
            let slot_color = Color::from_rgba(sr as f32, sg as f32, sb as f32, 1.0);
            // Plugin widgets: replay the instance's last decoded
            // scene (the frame path never calls wasm — spec §8);
            // disabled / missing / not-yet-rendered instances get
            // the dimmed fallback wedge (spec §9).
            if let Some(oxidemx_shared::WidgetSource::Custom(widget_id)) =
                s.widget.as_ref().map(|w| &w.source)
            {
                let instance_key = s
                    .widget
                    .as_ref()
                    .and_then(|w| w.instance_key.clone())
                    .unwrap_or_else(|| {
                        oxidemx_shared::widgets::instance_key(custom.page_name, index)
                    });
                let iid = oxidemx_widget_host::InstanceId {
                    instance_key,
                    widget_id: widget_id.clone(),
                };
                let scene = if custom.failed.contains_key(&iid) {
                    None
                } else {
                    custom.scenes.get(&iid).map(|(scene, _rev)| scene)
                };
                match scene {
                    Some(scene) => {
                        // Live wedge geometry matching this draw —
                        // mirrors widget_host::wedge_geom_for_slot
                        // but uses the (possibly animated) radii.
                        let sweep = slice_degrees.to_radians();
                        let a0 = (index as f32) * sweep - sweep / 2.0
                            - std::f32::consts::FRAC_PI_2;
                        let geom = oxidemx_widget_proto::WedgeGeom {
                            width: 2.0 * outer_r * (sweep / 2.0).sin(),
                            height: outer_r - inner_r,
                            inner_radius: inner_r,
                            outer_radius: outer_r,
                            angle_start: a0,
                            angle_end: a0 + sweep,
                            hovered: highlight.clamp(0.0, 1.0),
                        };
                        draw_custom_widget(
                            frame, scene, &geom, icon_pos, palette, hl, slot_color, mo,
                        );
                    }
                    None => {
                        draw_custom_fallback(
                            frame,
                            icon_pos,
                            s,
                            custom.registry.get(widget_id.as_str()),
                            palette,
                            mo,
                            slot_color,
                            icons,
                            icon_bg_radius,
                        );
                    }
                }
                return;
            }
            draw_widget_wedge(
                frame, icon_pos, s, widgets, palette, mo, hl, slot_color, icons,
            );
            return;
        }
    }

    // Per-slice override: when icon_untinted is set, render with
    // the original RGBA pixels (preserves brand colours for app
    // icons, custom artwork). Default path tints to the slice
    // colour for symbolic-icon consistency.
    let untinted = slice.map(|s| s.icon_untinted).unwrap_or(false);
    let resolved = if untinted {
        icons.resolve_untinted(icon_source, glyph_size_px)
    } else {
        icons.resolve(icon_source, glyph_size_px, icon_color_rgba)
    };
    if let Some(handle) = resolved {
        draw_icon(frame, icon_pos.x, icon_pos.y, glyph_size, &handle, mo);
    } else {
        let dot_color = Color::from_rgba(sr as f32, sg as f32, sb as f32, mo);
        frame.fill(&Path::circle(icon_pos, icon_bg_radius * 0.35), dot_color);
    }

    // Under-icon caption per the redesign: Dial wedges show their
    // live percentage, everything else its label. Skipped when the
    // slice has no label (placeholder slots stay clean).
    if let Some(s) = slice {
        let caption: Option<String> = match (s.kind, s.dial) {
            (oxidemx_shared::ActionKind::Dial, Some(kind)) => {
                let v = match kind {
                    oxidemx_shared::DialKind::Brightness => widgets.snap.brightness_percent,
                    oxidemx_shared::DialKind::Volume => widgets.snap.volume_percent,
                };
                Some(v.map(|p| format!("{p}%")).unwrap_or_else(|| "—".into()))
            }
            _ if !s.label.trim().is_empty() => Some(s.label.clone()),
            _ => None,
        };
        if let Some(caption) = caption {
            let (tr, tg, tb, _) = parse_hex_rgba(&palette.subtext1).unwrap_or((0.8, 0.8, 0.8, 1.0));
            let size = 10.0;
            draw_centered_text(
                frame,
                &caption,
                Point::new(icon_pos.x, icon_pos.y + icon_bg_radius + 4.0),
                size,
                Color::from_rgba(tr as f32, tg as f32, tb as f32, 0.9 * mo),
                iced::Font::DEFAULT,
            );
        }

        // Submenu badge: small surface bubble with a chevron at the
        // icon disc's bottom-right, per the design.
        if matches!(s.kind, oxidemx_shared::ActionKind::Submenu) && !s.submenu.is_empty() {
            let badge = Point::new(
                icon_pos.x + icon_bg_radius * 0.78,
                icon_pos.y + icon_bg_radius * 0.78,
            );
            let (b1r, b1g, b1b, _) =
                parse_hex_rgba(&palette.surface1).unwrap_or((0.2, 0.2, 0.25, 1.0));
            frame.fill(
                &Path::circle(badge, 8.0),
                Color::from_rgba(b1r as f32, b1g as f32, b1b as f32, 0.95 * mo),
            );
            frame.stroke(
                &Path::circle(badge, 8.0),
                Stroke::default()
                    .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.12 * mo))
                    .with_width(1.0),
            );
            let (str_, stg, stb, _) =
                parse_hex_rgba(&palette.subtext1).unwrap_or((0.8, 0.8, 0.85, 1.0));
            let chevron = Path::new(|b| {
                b.move_to(Point::new(badge.x - 1.5, badge.y - 3.0));
                b.line_to(Point::new(badge.x + 1.8, badge.y));
                b.line_to(Point::new(badge.x - 1.5, badge.y + 3.0));
            });
            frame.stroke(
                &chevron,
                Stroke::default()
                    .with_color(Color::from_rgba(str_ as f32, stg as f32, stb as f32, mo))
                    .with_width(1.6),
            );
        }

        // Toggle-state dot (night light): small glowing green dot at
        // the icon disc's top-right while the setting is on.
        if matches!(s.kind, oxidemx_shared::ActionKind::NightLight)
            && widgets.snap.night_light_on == Some(true)
        {
            let (gr, gg, gb, _) = parse_hex_rgba(&palette.green).unwrap_or((0.0, 0.9, 0.45, 1.0));
            let dot = Point::new(
                icon_pos.x + icon_bg_radius * 0.75,
                icon_pos.y - icon_bg_radius * 0.75,
            );
            frame.fill(
                &Path::circle(dot, 7.0),
                Color::from_rgba(gr as f32, gg as f32, gb as f32, 0.30 * mo),
            );
            frame.fill(
                &Path::circle(dot, 4.0),
                Color::from_rgba(gr as f32, gg as f32, gb as f32, mo),
            );
        }
    }
}
