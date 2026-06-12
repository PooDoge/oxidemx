//! Live-data widget wedges (`draw_widget_wedge`) and the shared
//! centred-text helper.

use iced::widget::canvas::{Frame, Path, Stroke};
use iced::{Color, Point};
use oxidemx_shared::theme::{parse_hex_rgba, ThemeColors};
use oxidemx_shared::Slice;

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
/// uppercase label — per `radial.jsx`'s widget slices.
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
