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

use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::{Color, Point, Radians, Vector};
use oxidemx_shared::theme::{parse_hex_rgba, ThemeColors};
use oxidemx_shared::{ComposedTransform, ElementAnimation, Slice};

use crate::anim;
use crate::radial::{
    SubmenuState, SUBITEM_RENDER_RADIUS, SUBITEM_RENDER_SPREAD_DEG, SUBMENU_RADIUS,
};
use crate::render::icons::{draw_icon, IconCache};

/// Fixed rasterization size for slice icons (ICON_BG_RADIUS × 1.4
/// at rest scale). Icons are always rasterized at THIS size and the
/// canvas scales the bitmap to the animated draw size — resolving
/// at the scaled size instead re-rasterizes every SVG through resvg
/// on every frame of any scale animation (menu-open spring, page
/// transitions, submenu pop), which is exactly the page-switch lag.
const GLYPH_RASTER_PX: u32 = 36;

/// Default wedge sweep for the legacy 8-slot ring. Kept for the
/// submenu pop-out which still uses this for parent-bisector
/// math; the main ring computes its sweep from the active page's
/// `slot_count` at render time.
#[allow(dead_code)]
const SLICE_DEGREES: f32 = 45.0;

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
            draw_widget_wedge(
                frame,
                icon_pos,
                s,
                widgets,
                palette,
                mo,
                hl,
                Color::from_rgba(sr as f32, sg as f32, sb as f32, 1.0),
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

/// Approximate-width centred single-line canvas text (the canvas
/// API has no measure pass; 0.55 em/char matches `draw_center`).
fn draw_centered_text(
    frame: &mut Frame,
    content: &str,
    center: Point,
    size: f32,
    color: Color,
    font: iced::Font,
) {
    let approx_w = content.chars().count() as f32 * size * 0.55;
    frame.fill_text(iced::widget::canvas::Text {
        content: content.to_string(),
        position: Point::new(center.x - approx_w / 2.0, center.y - size / 2.0),
        color,
        size: size.into(),
        font,
        ..iced::widget::canvas::Text::default()
    });
}

/// Live-data widget wedge: big value, optional sparkline, sublabel,
/// uppercase label — per `radial.jsx`'s widget slices.
#[allow(clippy::too_many_arguments)]
fn draw_widget_wedge(
    frame: &mut Frame,
    icon_pos: Point,
    slice: &Slice,
    widgets: &crate::radial::WidgetData,
    palette: &ThemeColors,
    mo: f32,
    hl: f32,
    slot_color: Color,
) {
    use oxidemx_shared::WidgetSource;
    let snap = &widgets.snap;
    let source = slice.widget.as_ref().map(|w| w.source);

    // Resolve (big value, sublabel, sparkline data) per source.
    let (big, small, spark): (String, String, Option<&std::collections::VecDeque<f32>>) =
        match source {
            Some(WidgetSource::Weather) => match &snap.weather {
                Some((t, cond)) => (format!("{}°", t.round() as i32), cond.clone(), None),
                None => ("—".into(), "set location".into(), None),
            },
            Some(WidgetSource::Cpu) => (
                snap.cpu_percent
                    .map(|c| format!("{}%", c.round() as u32))
                    .unwrap_or_else(|| "—".into()),
                match (snap.cpu_cores, snap.cpu_temp_c) {
                    (n, Some(t)) if n > 0 => format!("{n} cores · {}°C", t.round() as i32),
                    (n, None) if n > 0 => format!("{n} cores"),
                    _ => String::new(),
                },
                Some(&widgets.cpu_history),
            ),
            Some(WidgetSource::Memory) => (
                snap.mem_used_gb
                    .map(|u| format!("{u:.1}"))
                    .unwrap_or_else(|| "—".into()),
                snap.mem_total_gb
                    .map(|t| format!("of {} GB", t.round() as u32))
                    .unwrap_or_default(),
                None,
            ),
            Some(WidgetSource::Network) => (
                snap.net_down_mbps
                    .map(|d| format!("{}↓", d.round() as u32))
                    .unwrap_or_else(|| "—".into()),
                snap.net_up_mbps
                    .map(|u| format!("{}↑ Mb/s", u.round() as u32))
                    .unwrap_or_default(),
                Some(&widgets.net_history),
            ),
            Some(WidgetSource::Disk) => (
                snap.disk_free_gb
                    .map(|f| format!("{}", f.round() as u32))
                    .unwrap_or_else(|| "—".into()),
                "GB free".into(),
                None,
            ),
            // No task backend exists yet — honest stub per plan.
            Some(WidgetSource::TasksDue) => ("—".into(), "no task source".into(), None),
            Some(WidgetSource::MouseBattery) => match snap.mouse_battery {
                Some((pct, charging)) => (
                    format!("{pct}%"),
                    if charging {
                        "charging".into()
                    } else {
                        "MX Master 4".into()
                    },
                    None,
                ),
                None => ("—".into(), "no daemon".into(), None),
            },
            None => ("—".into(), "no source".into(), None),
        };

    let (tr, tg, tb, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));
    let text_c = Color::from_rgba(tr as f32, tg as f32, tb as f32, mo);
    let (s0r, s0g, s0b, _) = parse_hex_rgba(&palette.subtext0).unwrap_or((0.6, 0.65, 0.7, 1.0));
    let sub_c = Color::from_rgba(s0r as f32, s0g as f32, s0b as f32, mo);
    let (s1r, s1g, s1b, _) = parse_hex_rgba(&palette.subtext1).unwrap_or((0.78, 0.8, 0.85, 1.0));
    let label_c = Color::from_rgba(s1r as f32, s1g as f32, s1b as f32, mo);
    let value_c = if hl > 0.5 {
        Color {
            a: mo,
            ..slot_color
        }
    } else {
        text_c
    };

    // Vertical stack centred on the icon point: value, sparkline,
    // sublabel, uppercase label — proportions from radial.jsx
    // scaled to the 150 px ring.
    let mut y = icon_pos.y - 14.0;
    draw_centered_text(
        frame,
        &big,
        Point::new(icon_pos.x, y),
        18.0,
        value_c,
        iced::Font {
            weight: iced::font::Weight::Bold,
            ..Default::default()
        },
    );
    y += 13.0;

    if let Some(history) = spark {
        if history.len() >= 2 {
            let w = 40.0;
            let h = 10.0;
            let max = history.iter().cloned().fold(1.0_f32, f32::max);
            let step = w / (history.len() - 1) as f32;
            let path = Path::new(|b| {
                for (j, v) in history.iter().enumerate() {
                    let px = icon_pos.x - w / 2.0 + j as f32 * step;
                    let py = y + h - (v / max).clamp(0.0, 1.0) * h;
                    if j == 0 {
                        b.move_to(Point::new(px, py));
                    } else {
                        b.line_to(Point::new(px, py));
                    }
                }
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(Color {
                        a: 0.85 * mo,
                        ..slot_color
                    })
                    .with_width(1.5),
            );
            y += h + 3.0;
        }
    }

    if !small.is_empty() {
        draw_centered_text(
            frame,
            &small,
            Point::new(icon_pos.x, y + 4.0),
            8.5,
            sub_c,
            iced::Font::DEFAULT,
        );
        y += 11.0;
    }

    let label = slice.label.to_uppercase();
    if !label.is_empty() {
        draw_centered_text(
            frame,
            &label,
            Point::new(icon_pos.x, y + 6.0),
            8.0,
            if hl > 0.5 {
                Color {
                    a: mo,
                    ..slot_color
                }
            } else {
                label_c
            },
            iced::Font {
                weight: iced::font::Weight::Semibold,
                ..Default::default()
            },
        );
    }
}

/// Draw the page-name flash inside the centre puck — handles the
/// slide-in / cross-slide-out / fade-out sequence in one place.
/// Caller passes the current and (optional) previous page name,
/// the slide direction (+1 forward / -1 backward / 0 no-slide),
/// timing knobs, and the elapsed-since-trigger clock.
///
/// Timeline:
///   * `0..transition_ms` — incoming slides + fades in from
///     `direction × slide_distance` to 0; outgoing (if any)
///     slides + fades out from 0 to `−direction × slide_distance`.
///   * `transition_ms..(transition_ms + visible_ms)` — incoming
///     at full opacity, no slide.
///   * `(transition_ms + visible_ms)..total` — incoming fades
///     out to 0 over the same `transition_ms` window.
///
/// `total = 2 × transition_ms + visible_ms`. Returns `false` when
/// `elapsed >= total` so the caller can clear its timer.
#[allow(clippy::too_many_arguments)]
pub fn draw_page_name_transition(
    frame: &mut Frame,
    center: Point,
    radius: f32,
    palette: &ThemeColors,
    menu_opacity: f32,
    current_name: &str,
    previous_name: Option<&str>,
    direction: i32,
    elapsed_ms: u64,
    visible_ms: u32,
    transition_ms: u32,
    slide_distance_px: f32,
    label_size: f32,
    label_font: iced::Font,
    arced: bool,
) -> bool {
    let mo = menu_opacity.clamp(0.0, 1.0);
    let trans = transition_ms.max(1) as f32;
    let visible = visible_ms as f32;
    let total = trans + visible + trans;

    if (elapsed_ms as f32) >= total {
        return false;
    }
    let t = elapsed_ms as f32;

    // Incoming alpha + x_offset.
    // Phase 1 (0..trans): alpha ramps 0 → 1, x_offset ramps
    //   `direction * slide` → 0 (eased ease-out).
    // Phase 2 (trans..trans+visible): alpha 1, offset 0.
    // Phase 3 (trans+visible..total): alpha 1 → 0, offset 0.
    let (in_alpha, in_x) = if t < trans {
        let p = t / trans;
        let eased = 1.0 - (1.0 - p).powi(3); // ease-out cubic
        (eased, direction as f32 * slide_distance_px * (1.0 - eased))
    } else if t < trans + visible {
        (1.0, 0.0)
    } else {
        let p = (t - trans - visible) / trans;
        let eased = 1.0 - (1.0 - p).powi(3);
        (1.0 - eased, 0.0)
    };

    // Outgoing alpha + x_offset (only during phase 1).
    let outgoing = if t < trans && previous_name.is_some() && direction != 0 {
        let p = t / trans;
        let eased = 1.0 - (1.0 - p).powi(3);
        let alpha = 1.0 - eased;
        let x = -(direction as f32) * slide_distance_px * eased;
        Some((alpha, x))
    } else {
        None
    };

    let (tr, tg, tb, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));

    // Approximate visible-character width — same constant the
    // hover label uses (`label_size * 0.55`).
    let approx_w = |s: &str| s.chars().count() as f32 * label_size * 0.55;
    // Aggressive truncate so long page names fit the puck.
    let truncate = |s: &str| -> String {
        let max_chars = ((radius * 2.0 / (label_size * 0.55)).max(4.0)) as usize;
        if s.chars().count() > max_chars {
            let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
            out.push('…');
            out
        } else {
            s.to_string()
        }
    };

    let txt_color = |alpha: f32| {
        iced::Color::from_rgba(
            tr as f32,
            tg as f32,
            tb as f32,
            (mo * alpha).clamp(0.0, 1.0),
        )
    };

    let draw_label = |frame: &mut Frame, content: &str, alpha: f32, x_off: f32| {
        if alpha <= 0.001 {
            return;
        }
        let display = truncate(content);
        if arced {
            // Arced layout: each character on a small arc lifted
            // above the puck so the cursor (which sits on the
            // centre during page-cycle scrolling) doesn't sit
            // behind the label. `arc_radius` clears the puck
            // edge by ~font_size * 0.5; the slide translates the
            // whole arc horizontally so animations stay coherent
            // with the flat layout's behaviour.
            let arc_radius = radius + label_size * 0.9;
            let cell_w = label_size * 0.6;
            let chars: Vec<char> = display.chars().collect();
            let count = chars.len() as f32;
            let step = (cell_w / arc_radius).max(0.001);
            let centre_angle = -std::f32::consts::FRAC_PI_2;
            for (i, ch) in chars.iter().enumerate() {
                // Distribute chars symmetrically around 12 o'clock.
                let centred = i as f32 - (count - 1.0) / 2.0;
                let angle = centre_angle + centred * step;
                let pos = Point::new(
                    center.x + arc_radius * angle.cos() + x_off,
                    center.y + arc_radius * angle.sin(),
                );
                // Tangent so chars rotate to follow the arc.
                // Top of menu → tangent = angle + π/2 keeps the
                // top of each glyph pointing outward (away from
                // the puck).
                let tangent = angle + std::f32::consts::FRAC_PI_2;
                let s: String = ch.to_string();
                frame.with_save(|f| {
                    f.translate(Vector::new(pos.x, pos.y));
                    f.rotate(Radians(tangent));
                    f.fill_text(iced::widget::canvas::Text {
                        content: s,
                        position: iced::Point::new(-cell_w / 2.0, -label_size / 2.0),
                        color: txt_color(alpha),
                        size: label_size.into(),
                        font: label_font,
                        ..iced::widget::canvas::Text::default()
                    });
                });
            }
        } else {
            // Flat layout: single fill_text centred in the puck.
            let w = approx_w(&display);
            frame.fill_text(iced::widget::canvas::Text {
                content: display,
                position: iced::Point::new(center.x - w / 2.0 + x_off, center.y - label_size / 2.0),
                color: txt_color(alpha),
                size: label_size.into(),
                font: label_font,
                ..iced::widget::canvas::Text::default()
            });
        }
    };

    if let (Some(prev), Some((alpha, x_off))) = (previous_name, outgoing) {
        draw_label(frame, prev, alpha, x_off);
    }
    draw_label(frame, current_name, in_alpha, in_x);

    true
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

/// Centre puck — small filled circle with stroked accent ring,
/// optionally with a label drawn inside (the hovered slice's name).
/// Ports `_draw_center` from the Python overlay; the centre-text
/// rendering is the long-promised "/* text overlay lands in a
/// follow-up */" finally landing here.
/// `label_alpha_mul` — extra opacity multiplier applied **only**
/// to the centre label and description. Lets transient labels
/// (e.g. the page-name flash on a cycle) fade out independently
/// of the puck fill / rim. Pass `1.0` for the standard
/// hover-label path; the page-name flash passes a ramped value
/// while it fades out.
#[allow(clippy::too_many_arguments)]
pub fn draw_center(
    frame: &mut Frame,
    center: Point,
    radius: f32,
    palette: &ThemeColors,
    menu_opacity: f32,
    bg_opacity: f32,
    label: Option<&str>,
    description: Option<&str>,
    label_size: f32,
    label_font: iced::Font,
    accent_flash: f32,
    label_alpha_mul: f32,
) {
    let mo = menu_opacity.clamp(0.0, 1.0);
    let bgo = bg_opacity.clamp(0.0, 1.0);
    let flash = accent_flash.clamp(0.0, 1.0);
    let puck = Path::circle(center, radius);
    // Puck fill tracks the same opacity slider as the wedge fill,
    // so the user gets one consistent "how see-through is the
    // menu" knob instead of the previous split where the puck
    // had its own 86 % cap.
    frame.fill(&puck, rgba(&palette.surface0, mo * bgo));
    // Default rim stroke — uses accent_dim. During an accent flash
    // (CenterPulse page transition) the rim brightens up to the
    // full accent colour and thickens slightly so the swap reads
    // as "the centre just clicked into a new page".
    let base_rim = rgba(&palette.accent_dim, (140.0 / 255.0) * mo * bgo);
    let rim_color = if flash > 0.0 {
        let bright = rgba(&palette.accent, mo * bgo);
        // Linear blend in unpremultiplied RGBA — close enough at
        // these alphas, and lerp() is local to this module's hover
        // code so we'd be reaching past visibility.
        iced::Color {
            r: base_rim.r + (bright.r - base_rim.r) * flash,
            g: base_rim.g + (bright.g - base_rim.g) * flash,
            b: base_rim.b + (bright.b - base_rim.b) * flash,
            a: base_rim.a + (bright.a - base_rim.a) * flash,
        }
    } else {
        base_rim
    };
    let rim_width = 2.0 + 2.0 * flash;
    frame.stroke(
        &puck,
        Stroke::default()
            .with_color(rim_color)
            .with_width(rim_width),
    );
    // Outer halo ring — only during a flash. Sits just outside the
    // puck and fades in/out with the pulse. Gives the swap a
    // visible "ripple" rather than a silent radius bump.
    if flash > 0.0 {
        let halo = Path::circle(center, radius + 6.0);
        let (ar, ag, ab, _) = parse_hex_rgba(&palette.accent).unwrap_or((1.0, 1.0, 1.0, 1.0));
        frame.stroke(
            &halo,
            Stroke::default()
                .with_color(iced::Color::from_rgba(
                    ar as f32,
                    ag as f32,
                    ab as f32,
                    0.55 * flash * mo,
                ))
                .with_width(2.0),
        );
    }

    let has_description = description.map(|d| !d.trim().is_empty()).unwrap_or(false);
    let description_size = (label_size * 0.62).max(8.0);

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
            // Lift the label slightly when a description is also
            // shown so the two lines stack symmetrically across the
            // puck centre instead of the label sitting dead-centre
            // and the description hanging below.
            let label_y_offset = if has_description {
                -(description_size * 0.65)
            } else {
                0.0
            };
            let label_alpha = (mo * label_alpha_mul).clamp(0.0, 1.0);
            frame.fill_text(iced::widget::canvas::Text {
                content: display,
                position: iced::Point::new(
                    center.x - approx_w / 2.0,
                    center.y - label_size / 2.0 + label_y_offset,
                ),
                color: iced::Color::from_rgba(tr as f32, tg as f32, tb as f32, label_alpha),
                size: label_size.into(),
                font: label_font,
                ..iced::widget::canvas::Text::default()
            });
        }
    }

    if let Some(text) = description {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            // Description sits a half-line below the label; truncate
            // a bit more aggressively because the smaller font fits
            // more glyphs across the puck.
            let max_chars = ((radius * 2.0 / (description_size * 0.55)).max(6.0)) as usize;
            let display: String = if trimmed.chars().count() > max_chars {
                let mut out: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
                out.push('…');
                out
            } else {
                trimmed.to_string()
            };
            let approx_w = display.chars().count() as f32 * description_size * 0.55;
            let (tr, tg, tb, _) = parse_hex_rgba(&palette.subtext0).unwrap_or((0.7, 0.7, 0.7, 1.0));
            // Anchor description below the label baseline. The label
            // (when present) was nudged up by ~0.65× description
            // size; place description ~0.85× description size below
            // centre so the gap reads as a natural line break.
            frame.fill_text(iced::widget::canvas::Text {
                content: display,
                position: iced::Point::new(center.x - approx_w / 2.0, center.y + label_size * 0.05),
                color: iced::Color::from_rgba(
                    tr as f32,
                    tg as f32,
                    tb as f32,
                    (mo * label_alpha_mul * 0.85).clamp(0.0, 1.0),
                ),
                size: description_size.into(),
                font: label_font,
                ..iced::widget::canvas::Text::default()
            });
        }
    }
}

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

/// Draw the multi-page indicator — small dots arrayed in a shallow
/// arc that hugs the bottom rim of the centre puck. The arc is
/// anchored at 90° (straight down) and fans symmetrically left/right
/// from there, so when the active page is the middle of the cycle
/// the active dot sits at dead-bottom. No-op for single-page menus.
///
/// Sits along the puck's inside edge (just inboard of the rim
/// stroke) so the dots and the centre-hover label don't fight for
/// the same pixels.
pub fn draw_page_indicator(
    frame: &mut Frame,
    center: Point,
    radius: f32,
    palette: &ThemeColors,
    menu_opacity: f32,
    page_count: usize,
    active: Option<usize>,
) {
    if page_count < 2 {
        return;
    }
    let mo = menu_opacity.clamp(0.0, 1.0);
    let dot_r: f32 = 2.5;

    // Place the dots on a circle slightly inside the puck rim so
    // they read as "on the puck" without clipping the stroke.
    let arc_radius = (radius - dot_r * 2.5).max(dot_r * 2.0);

    // Angular step between adjacent dots. Aim for ~8 px chord
    // distance so the spacing visually matches the old straight
    // strip; clamp to a minimum so two-page menus don't crowd.
    let target_chord: f32 = 8.0;
    let mut step_rad = (target_chord / arc_radius.max(1.0)).max(0.22); // ≈ 12.6° min
                                                                       // Cap the total arc so the strip never sweeps past ~±45° from
                                                                       // straight-down — beyond that the dots start overlapping the
                                                                       // hover label and the page-cycle reads as a curve rather than
                                                                       // an indicator.
    let max_total_rad: f32 = std::f32::consts::FRAC_PI_2; // 90° total
    if (page_count as f32 - 1.0) * step_rad > max_total_rad {
        step_rad = max_total_rad / (page_count as f32 - 1.0).max(1.0);
    }
    let center_idx = (page_count as f32 - 1.0) / 2.0;
    // Bottom of the puck in iced canvas coords is +Y, which is
    // angle = π/2 (90°) from polar() since polar() uses
    // sin(angle) for Y with the canvas Y-down convention.
    let base_angle = std::f32::consts::FRAC_PI_2;

    let (ar, ag, ab, _) = parse_hex_rgba(&palette.accent).unwrap_or((1.0, 1.0, 1.0, 1.0));
    let (tr, tg, tb, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));

    for i in 0..page_count {
        // Negate the offset so dot 0 lands on the LEFT and dot
        // N-1 on the RIGHT (Western reading order). Iced's
        // canvas Y is down, so a positive angle offset moves the
        // sample point counter-clockwise (toward the left at the
        // bottom of the puck). We want the opposite: dot index
        // grows left-to-right, so flip.
        let offset = (center_idx - i as f32) * step_rad;
        let angle = base_angle + offset;
        let pos = polar(center, arc_radius, angle);
        let path = Path::circle(pos, dot_r);
        let is_active = active == Some(i);
        let color = if is_active {
            iced::Color::from_rgba(ar as f32, ag as f32, ab as f32, mo)
        } else {
            iced::Color::from_rgba(tr as f32, tg as f32, tb as f32, 0.35 * mo)
        };
        frame.fill(&path, color);
    }
}

/// Ring treatment for the travelling page puck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuckRing {
    /// Accent stroke + outer glow — the puck is armed (page cycle
    /// live, chat render-only).
    Armed,
    /// Subtle inactive stroke — the chat owns input; the puck is
    /// still a wheel target but visually parked.
    Dimmed,
}

/// Draw the centre puck as a standalone object: dome-ish fill, ring
/// stroke per handoff phase, live page dots. Used by the chat
/// shell's `CapsPainter` to keep the puck visible (and travelling)
/// through the disc → chat morph; visually consistent with
/// `draw_center` + `draw_page_indicator` at the morph's t = 0
/// boundary so there's no pop when the painters swap.
#[allow(clippy::too_many_arguments)]
pub fn draw_puck(
    frame: &mut Frame,
    center: Point,
    radius: f32,
    palette: &ThemeColors,
    alpha: f32,
    ring: PuckRing,
    page_count: usize,
    active: Option<usize>,
) {
    let a = alpha.clamp(0.0, 1.0);
    if a <= 0.001 || radius <= 1.0 {
        return;
    }
    let body = Path::circle(center, radius);
    // Dome fill: crust base + an offset surface2 highlight fakes the
    // design's "radial-gradient(circle at 36% 30%, surface2, crust)"
    // within the canvas API's solid fills.
    frame.fill(&body, rgba(&palette.crust, 0.96 * a));
    let highlight = Path::circle(
        Point::new(center.x - radius * 0.28, center.y - radius * 0.40),
        radius * 0.50,
    );
    frame.fill(&highlight, rgba(&palette.surface2, 0.30 * a));

    match ring {
        PuckRing::Armed => {
            // Outer glow first so the crisp ring paints over it.
            let glow = Path::circle(center, radius + 2.5);
            frame.stroke(
                &glow,
                Stroke::default()
                    .with_color(rgba(&palette.accent, 0.30 * a))
                    .with_width(5.0),
            );
            frame.stroke(
                &body,
                Stroke::default()
                    .with_color(rgba(&palette.accent, a))
                    .with_width(2.5),
            );
        }
        PuckRing::Dimmed => {
            frame.stroke(
                &body,
                Stroke::default()
                    .with_color(rgba(&palette.overlay0, 0.9 * a))
                    .with_width(1.5),
            );
        }
    }

    draw_page_indicator(frame, center, radius, palette, a, page_count, active);
}

// =============================================================================
// helpers
// =============================================================================

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
fn arc_line_to(p: &mut canvas::path::Builder, center: Point, radius: f32, from: f32, to: f32) {
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

fn build_wedge(center: Point, inner_r: f32, outer_r: f32, start_rad: f32, end_rad: f32) -> Path {
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
