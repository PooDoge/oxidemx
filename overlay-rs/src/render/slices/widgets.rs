//! Live-data widget wedges (`draw_widget_wedge`), the plugin-scene
//! replay path (`draw_custom_widget` + `draw_custom_fallback`), and
//! the shared centred-text helper.

use iced::widget::canvas::{Frame, Path, Stroke};
use iced::{Color, Point, Size};
use oxidemx_shared::theme::{parse_hex_rgba, ThemeColors};
use oxidemx_shared::Slice;
use oxidemx_widget_host::{InstanceId, WidgetSummary};
use oxidemx_widget_proto::Scene;

// The scene replay (prim walk + palette colour resolution) lives in
// the shared `oxidemx-scene-render` crate so the settings app's
// options-card preview replays the exact pixels this ring draws.
pub(super) use oxidemx_scene_render::draw_custom_widget;

use super::rgba;

/// Per-ring view of the custom-widget runtime state the painter
/// hands down to `draw_slice`: the page name (derived instance
/// keys), the replay store, the failure set, and the installed-
/// widget registry (fallback icons).
pub struct CustomWidgets<'a> {
    pub page_name: &'a str,
    pub scenes: &'a std::collections::HashMap<InstanceId, (Scene, u64)>,
    pub failed: &'a std::collections::HashMap<InstanceId, String>,
    pub registry: &'a std::collections::HashMap<String, WidgetSummary>,
}

/// Approximate-width centred single-line canvas text (the canvas
/// API has no measure pass; 0.55 em/char matches `draw_center`).
pub(super) fn draw_centered_text(
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
/// uppercase label — per `radial.jsx`'s widget slices. The Weather
/// wedge is the exception: condition icon only (details live in
/// the hover popup — see `draw_weather_popup`).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_widget_wedge(
    frame: &mut Frame,
    icon_pos: Point,
    slice: &Slice,
    widgets: &crate::radial::WidgetData,
    palette: &ThemeColors,
    mo: f32,
    hl: f32,
    slot_color: Color,
    icons: &crate::render::icons::IconCache,
) {
    use oxidemx_shared::WidgetSource;
    let snap = &widgets.snap;
    let source = slice.widget.as_ref().map(|w| w.source.clone());

    let (s1r, s1g, s1b, _) = parse_hex_rgba(&palette.subtext1).unwrap_or((0.78, 0.8, 0.85, 1.0));
    let label_c = Color::from_rgba(s1r as f32, s1g as f32, s1b as f32, mo);

    // Weather: condition icon centred in the wedge + uppercase
    // label. Tinted to the slot colour on hover, text colour
    // otherwise, like the value typography of the other widgets.
    if source == Some(WidgetSource::Weather) {
        if let Some(w) = &snap.weather {
            let icon_name = crate::sampler::weather_icon_name(w.code);
            let tint = if hl > 0.5 {
                (slot_color.r, slot_color.g, slot_color.b, 1.0)
            } else {
                let (tr, tg, tb, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));
                (tr as f32, tg as f32, tb as f32, 1.0)
            };
            let icon_center = Point::new(icon_pos.x, icon_pos.y - 6.0);
            if let Some(handle) = icons.resolve(icon_name, super::GLYPH_RASTER_PX, tint) {
                crate::render::icons::draw_icon(
                    frame,
                    icon_center.x,
                    icon_center.y,
                    34.0,
                    &handle,
                    mo,
                );
            } else {
                // Theme without weather icons — fall back to the
                // short condition label so the wedge isn't blank.
                draw_centered_text(
                    frame,
                    &w.label,
                    icon_center,
                    11.0,
                    Color::from_rgba(tint.0, tint.1, tint.2, mo),
                    iced::Font::DEFAULT,
                );
            }
            let label = slice.label.to_uppercase();
            if !label.is_empty() {
                draw_centered_text(
                    frame,
                    &label,
                    Point::new(icon_pos.x, icon_pos.y + 22.0),
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
            return;
        }
        // No data yet — fall through to the standard stub layout.
    }

    // Resolve (big value, sublabel, sparkline data) per source.
    let (big, small, spark): (String, String, Option<&std::collections::VecDeque<f32>>) =
        match source {
            Some(WidgetSource::Weather) => ("—".into(), "set location".into(), None),
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
            Some(WidgetSource::TasksDue) => match snap.tasks_due {
                Some(n) => (n.to_string(), "due in 24h".into(), None),
                None => ("—".into(), "scheduled tasks".into(), None),
            },
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
            Some(WidgetSource::Custom(_)) => ("—".into(), "plugin".into(), None),
            None => ("—".into(), "no source".into(), None),
        };

    let (tr, tg, tb, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));
    let text_c = Color::from_rgba(tr as f32, tg as f32, tb as f32, mo);
    let (s0r, s0g, s0b, _) = parse_hex_rgba(&palette.subtext0).unwrap_or((0.6, 0.65, 0.7, 1.0));
    let sub_c = Color::from_rgba(s0r as f32, s0g as f32, s0b as f32, mo);
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

/// Fallback wedge for a disabled / missing / not-yet-rendered
/// plugin instance (spec §9): the manifest's `fallback_icon` (or
/// the widget's own icon) dimmed on a faint disc, a ⚠ badge at the
/// disc's top-right, and the slice label below — the same
/// icon+caption layout `draw_slice` paints for placeholder slots.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_custom_fallback(
    frame: &mut Frame,
    icon_pos: Point,
    slice: &Slice,
    summary: Option<&WidgetSummary>,
    palette: &ThemeColors,
    mo: f32,
    slot_color: Color,
    icons: &crate::render::icons::IconCache,
    icon_bg_radius: f32,
) {
    let dim = 0.45 * mo;

    // Faint icon-background disc (the regular widget path skips the
    // icon furniture; the fallback brings it back so the wedge
    // reads as "a slot with a problem", not "empty").
    let (s1r, s1g, s1b, _) = parse_hex_rgba(&palette.surface1).unwrap_or((0.2, 0.2, 0.25, 1.0));
    frame.fill(
        &Path::circle(icon_pos, icon_bg_radius),
        Color::from_rgba(s1r as f32, s1g as f32, s1b as f32, 0.5 * mo),
    );

    // fallback_icon (XDG name) first, then the bundle's own icon
    // file (absolute path) — IconCache resolves both shapes.
    let source: Option<String> = summary
        .and_then(|s| s.fallback_icon.clone())
        .or_else(|| summary.map(|s| s.icon_path.display().to_string()));
    let tint = (slot_color.r, slot_color.g, slot_color.b, 1.0);
    let glyph_size = (icon_bg_radius * 1.4).max(8.0);
    let drew_icon = source
        .and_then(|src| icons.resolve(&src, super::GLYPH_RASTER_PX, tint))
        .map(|handle| {
            crate::render::icons::draw_icon(
                frame, icon_pos.x, icon_pos.y, glyph_size, &handle, dim,
            );
        })
        .is_some();
    if !drew_icon {
        // Not installed / icon unloadable — placeholder dot, same as
        // draw_slice's icon-miss path, dimmed.
        frame.fill(
            &Path::circle(icon_pos, icon_bg_radius * 0.35),
            Color {
                a: dim,
                ..slot_color
            },
        );
    }

    // ⚠ badge at the disc's top-right (theme yellow).
    let (yr, yg, yb, _) = parse_hex_rgba(&palette.yellow).unwrap_or((0.96, 0.76, 0.25, 1.0));
    draw_centered_text(
        frame,
        "⚠",
        Point::new(
            icon_pos.x + icon_bg_radius * 0.78,
            icon_pos.y - icon_bg_radius * 0.78,
        ),
        12.0,
        Color::from_rgba(yr as f32, yg as f32, yb as f32, mo),
        iced::Font::DEFAULT,
    );

    // Caption below the disc — slice label (dimmed), mirroring the
    // under-icon caption of regular slices.
    if !slice.label.trim().is_empty() {
        let (tr, tg, tb, _) = parse_hex_rgba(&palette.subtext1).unwrap_or((0.8, 0.8, 0.8, 1.0));
        draw_centered_text(
            frame,
            &slice.label,
            Point::new(icon_pos.x, icon_pos.y + icon_bg_radius + 4.0),
            10.0,
            Color::from_rgba(tr as f32, tg as f32, tb as f32, 0.9 * dim),
            iced::Font::DEFAULT,
        );
    }
}

/// Hover popup for the Weather wedge: place name, current
/// conditions, and the 7-day forecast as a horizontal strip.
///
/// Placement (per design feedback): horizontally centred on the
/// menu, in the clear band ABOVE or BELOW the ring — whichever
/// half the hovered slice sits in — so the card never covers the
/// wedges. The bands are the only ring-free regions of the disc
/// square, which is also why the card is a wide strip rather
/// than a tall list. Falls back to the opposite band if the
/// preferred one would clip off-canvas.
#[allow(clippy::too_many_arguments)]
pub fn draw_weather_popup(
    frame: &mut Frame,
    canvas_size: Size,
    center: Point,
    outer_r: f32,
    slot_index: usize,
    slot_count: usize,
    weather: &crate::sampler::WeatherInfo,
    palette: &ThemeColors,
    icons: &crate::render::icons::IconCache,
    alpha: f32,
) {
    if alpha <= 0.001 {
        return;
    }
    // The popup is an information surface, not menu chrome — it
    // fades only with the hover dwell, NOT with the user's menu
    // opacity, so its content stays legible whatever the ring's
    // translucency is set to.
    let a = alpha.clamp(0.0, 1.0);

    const PAD: f32 = 10.0;
    const COL_W: f32 = 42.0;
    const GAP: f32 = 6.0;
    let cols = weather.daily.len().min(7) as f32;
    let w: f32 = (cols * COL_W).max(220.0) + 2.0 * PAD;
    let header_h = 15.0;
    let day_block_h = 50.0;
    let h = PAD + header_h + GAP + day_block_h + PAD - 2.0;

    // Centre on the menu; pick the ring-free band matching the
    // hovered slice's vertical half (slice 0 is 12 o'clock).
    let bisector = ((slot_index as f32) * (360.0 / slot_count.max(1) as f32) - 90.0).to_radians();
    let x = (center.x - w / 2.0).clamp(2.0, (canvas_size.width - w - 2.0).max(2.0));
    let y_top = center.y - outer_r - h - 6.0;
    let y_bottom = center.y + outer_r + 6.0;
    let prefer_top = bisector.sin() <= 0.0;
    let fits = |y: f32| y >= 2.0 && y + h <= canvas_size.height - 2.0;
    let y = match (prefer_top, fits(y_top), fits(y_bottom)) {
        (true, true, _) | (false, true, false) => y_top,
        (false, _, true) | (true, false, true) => y_bottom,
        // Neither band fits (tiny canvas) — clamp the preferred one.
        _ => y_top.clamp(2.0, (canvas_size.height - h - 2.0).max(2.0)),
    };

    let card = Path::new(|b| {
        b.rounded_rectangle(Point::new(x, y), Size::new(w, h), 12.0.into());
    });
    frame.fill(&card, rgba(&palette.crust, 0.97 * a));
    frame.stroke(
        &card,
        Stroke::default()
            .with_color(rgba(&palette.accent_dim, 0.6 * a))
            .with_width(1.0),
    );

    let text_c = rgba(&palette.text, a);
    let sub_c = rgba(&palette.subtext0, a);
    let dim_c = rgba(&palette.subtext1, a);
    let left = x + PAD;
    let right = x + w - PAD;

    let put = |f: &mut Frame, s: &str, px: f32, py: f32, size: f32, color: Color, bold: bool| {
        f.fill_text(iced::widget::canvas::Text {
            content: s.to_string(),
            position: Point::new(px, py),
            color,
            size: size.into(),
            font: iced::Font {
                weight: if bold {
                    iced::font::Weight::Semibold
                } else {
                    iced::font::Weight::Normal
                },
                ..Default::default()
            },
            ..iced::widget::canvas::Text::default()
        });
    };
    // Approximate text width — same 0.55 em/char heuristic as
    // `draw_centered_text` (the canvas API has no measure pass).
    let approx_w = |s: &str, size: f32| s.chars().count() as f32 * size * 0.55;

    // Header line: place name (left, ellipsised to the space the
    // current-conditions block leaves free) + current temp (right).
    let unit = if weather.fahrenheit { "°F" } else { "°C" };
    let current = format!("{}{unit} {}", weather.temp.round() as i32, weather.label);
    let cur_w = approx_w(&current, 11.0);
    let (tr, tg, tb, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));
    let tint = (tr as f32, tg as f32, tb as f32, 1.0);
    if let Some(place) = &weather.place {
        let budget = w - 2.0 * PAD - cur_w - 12.0;
        let mut shown: String = place.clone();
        while approx_w(&shown, 10.5) > budget && shown.chars().count() > 1 {
            shown.pop();
            if !shown.ends_with('…') {
                shown.pop();
                shown.push('…');
            }
        }
        put(frame, &shown, left, y + PAD, 10.5, sub_c, true);
    }
    put(frame, &current, right - cur_w, y + PAD, 11.0, text_c, true);

    // 7-day strip: one column per day — weekday, condition icon,
    // high, low.
    let strip_y = y + PAD + header_h + GAP;
    let strip_w = cols * COL_W;
    let strip_x = x + (w - strip_w) / 2.0;
    for (i, day) in weather.daily.iter().take(7).enumerate() {
        let cx = strip_x + (i as f32 + 0.5) * COL_W;
        draw_centered_text(
            frame,
            &day.day,
            Point::new(cx, strip_y + 4.0),
            8.5,
            dim_c,
            iced::Font::DEFAULT,
        );
        if let Some(handle) = icons.resolve(
            crate::sampler::weather_icon_name(day.code),
            super::GLYPH_RASTER_PX,
            tint,
        ) {
            crate::render::icons::draw_icon(frame, cx, strip_y + 18.0, 15.0, &handle, a);
        }
        draw_centered_text(
            frame,
            &format!("{}°", day.t_max.round() as i32),
            Point::new(cx, strip_y + 33.0),
            9.5,
            text_c,
            iced::Font {
                weight: iced::font::Weight::Semibold,
                ..Default::default()
            },
        );
        draw_centered_text(
            frame,
            &format!("{}°", day.t_min.round() as i32),
            Point::new(cx, strip_y + 44.0),
            9.0,
            sub_c,
            iced::Font::DEFAULT,
        );
    }
}
