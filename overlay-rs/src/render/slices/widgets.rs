//! Live-data widget wedges (`draw_widget_wedge`) and the shared
//! centred-text helper.

use iced::widget::canvas::{Frame, Path, Stroke};
use iced::{Color, Point, Size};
use oxidemx_shared::theme::{parse_hex_rgba, ThemeColors};
use oxidemx_shared::Slice;

use super::rgba;

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
    let source = slice.widget.as_ref().map(|w| w.source);

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

/// Hover popup for the Weather wedge: place name, current
/// conditions, and the 7-day forecast. Drawn last in the paint
/// pass so it sits above the ring; anchored along the hovered
/// slot's bisector just outside the outer ring and clamped to the
/// canvas so it never clips off-window.
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
    mo: f32,
    alpha: f32,
) {
    if alpha <= 0.001 {
        return;
    }
    let a = (mo * alpha).clamp(0.0, 1.0);

    const W: f32 = 178.0;
    const PAD: f32 = 12.0;
    const ROW_H: f32 = 16.0;
    let header_h = if weather.place.is_some() { 16.0 } else { 0.0 };
    let current_h = 30.0;
    let rows = weather.daily.len().min(7) as f32;
    let h = PAD + header_h + current_h + 8.0 + rows * ROW_H + PAD - 4.0;

    // Anchor outward along the hovered slot's bisector, then clamp.
    let bisector = ((slot_index as f32) * (360.0 / slot_count.max(1) as f32) - 90.0).to_radians();
    let anchor = Point::new(
        center.x + (outer_r + 6.0) * bisector.cos(),
        center.y + (outer_r + 6.0) * bisector.sin(),
    );
    let mut x = if bisector.cos() >= 0.0 {
        anchor.x
    } else {
        anchor.x - W
    };
    let mut y = anchor.y - h / 2.0 + (h / 2.0) * bisector.sin();
    x = x.clamp(4.0, (canvas_size.width - W - 4.0).max(4.0));
    y = y.clamp(4.0, (canvas_size.height - h - 4.0).max(4.0));

    let card = Path::new(|b| {
        b.rounded_rectangle(Point::new(x, y), Size::new(W, h), 12.0.into());
    });
    frame.fill(&card, rgba(&palette.crust, 0.96 * a));
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
    let right = x + W - PAD;
    let mut cy = y + PAD;

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
    // Right-aligned variant via the same approximate width used by
    // `draw_centered_text` (no canvas measure pass).
    let approx_w = |s: &str, size: f32| s.chars().count() as f32 * size * 0.55;

    if let Some(place) = &weather.place {
        put(frame, place, left, cy, 10.5, sub_c, true);
        cy += header_h;
    }

    // Current conditions: icon + temperature + label.
    let unit = if weather.fahrenheit { "°F" } else { "°C" };
    let (tr, tg, tb, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));
    let tint = (tr as f32, tg as f32, tb as f32, 1.0);
    if let Some(handle) = icons.resolve(
        crate::sampler::weather_icon_name(weather.code),
        super::GLYPH_RASTER_PX,
        tint,
    ) {
        crate::render::icons::draw_icon(frame, left + 11.0, cy + 12.0, 22.0, &handle, a);
    }
    put(
        frame,
        &format!("{}{unit}", weather.temp.round() as i32),
        left + 28.0,
        cy + 2.0,
        16.0,
        text_c,
        true,
    );
    put(
        frame,
        &weather.label,
        left + 28.0,
        cy + 19.0,
        9.0,
        sub_c,
        false,
    );
    cy += current_h;

    // Separator above the forecast rows.
    frame.fill(
        &Path::rectangle(Point::new(left, cy), Size::new(W - 2.0 * PAD, 1.0)),
        rgba(&palette.surface2, 0.9 * a),
    );
    cy += 7.0;

    for day in weather.daily.iter().take(7) {
        put(frame, &day.day, left, cy, 9.5, dim_c, false);
        if let Some(handle) = icons.resolve(
            crate::sampler::weather_icon_name(day.code),
            super::GLYPH_RASTER_PX,
            tint,
        ) {
            crate::render::icons::draw_icon(frame, left + 62.0, cy + 5.5, 13.0, &handle, a);
        }
        let hi = format!("{}°", day.t_max.round() as i32);
        let lo = format!("{}°", day.t_min.round() as i32);
        put(
            frame,
            &hi,
            right - approx_w(&hi, 9.5),
            cy,
            9.5,
            text_c,
            true,
        );
        put(
            frame,
            &lo,
            right - 30.0 - approx_w(&lo, 9.5),
            cy,
            9.5,
            sub_c,
            false,
        );
        cy += ROW_H;
    }
}
