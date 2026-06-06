//! Iced view for the indicator popup.
//!
//! Mirrors the structure of `design/oxidemx-indicator/popup.jsx`:
//!
//!   • Header row — device name pill on the left, connection status on right.
//!   • Battery ring — large circular progress drawn on a `Canvas`.
//!   • Easy-Switch segmented host buttons (when `popup.show_host_buttons`).
//!   • Quick toggles — icon glyph + label + space + toggler.
//!   • Quick sliders — `labeled_int_slider` rows (Power User mode only).
//!   • Footer — "Settings" flat button + version text.
//!
//! All colours come from the resolved `Palette` (same one the settings
//! window uses), so re-theming works automatically.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{
    button, column, container, row, rule, scrollable, text, toggler, Space,
};
use iced::{Alignment, Color, Element, Length, Radians};
use oxidemx_widgets::style;
use oxidemx_widgets::widgets::labeled_int_slider;
use oxidemx_shared::{PopupMode, QUICK_SLIDER_CATALOG, QUICK_TOGGLE_CATALOG};

use crate::app::{Message, State};
use crate::gsettings_bridge::BatteryColors;

// ---------------------------------------------------------------------------
// Battery ring — canvas-based circular progress
// ---------------------------------------------------------------------------

/// Painter for the battery ring. Draws a circular arc that fills
/// proportionally to the battery percentage, colour-coded by band.
struct RingPainter {
    pct: u8,
    charging: bool,
    fg: Color,
    bg: Color,
}

impl canvas::Program<Message> for RingPainter {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.width.min(bounds.height);
        let stroke_w = 7.0_f32;
        let r = (size - stroke_w) / 2.0;
        let cx = bounds.width / 2.0;
        let cy = bounds.height / 2.0;

        // Background arc (full circle, dimmed).
        let bg_path = Path::new(|b| {
            b.arc(canvas::path::Arc {
                center: iced::Point::new(cx, cy),
                radius: r,
                start_angle: Radians(-std::f32::consts::FRAC_PI_2),
                end_angle: Radians(-std::f32::consts::FRAC_PI_2 + 2.0 * std::f32::consts::PI),
            });
        });
        frame.stroke(
            &bg_path,
            Stroke::default()
                .with_color(self.bg)
                .with_width(stroke_w),
        );

        // Foreground arc — proportional to battery percentage.
        let fill_angle = 2.0 * std::f32::consts::PI * (self.pct as f32 / 100.0);
        if fill_angle > 0.01 {
            let fg_path = Path::new(|b| {
                b.arc(canvas::path::Arc {
                    center: iced::Point::new(cx, cy),
                    radius: r,
                    start_angle: Radians(-std::f32::consts::FRAC_PI_2),
                    end_angle: Radians(-std::f32::consts::FRAC_PI_2 + fill_angle),
                });
            });
            frame.stroke(
                &fg_path,
                Stroke::default()
                    .with_color(self.fg)
                    .with_width(stroke_w),
            );
        }

        // Centre text — percentage value.  Position is top-left of text,
        // not centre — iced canvas::Text doesn't support centre alignment
        // via field; we approximate by offsetting by half the estimated
        // text width. Good enough for 2–4 char labels.
        let label = if self.charging {
            format!("{}⚡", self.pct)
        } else {
            format!("{}%", self.pct)
        };
        let font_size = 16.0_f32;
        // Rough character width estimate (proportional font, ~0.6 × height).
        let approx_w = label.chars().count() as f32 * font_size * 0.55;
        frame.fill_text(canvas::Text {
            content: label,
            position: iced::Point::new(cx - approx_w / 2.0, cy - font_size * 0.7),
            color: Color::WHITE,
            size: font_size.into(),
            ..canvas::Text::default()
        });

        vec![frame.into_geometry()]
    }
}

/// Resolve which colour to use for the ring fill given the battery
/// state and the GSettings colour prefs.
fn ring_fill_color(pct: u8, charging: bool, colors: &BatteryColors) -> Color {
    let hex = if charging {
        &colors.color_charging
    } else if pct <= colors.threshold_critical {
        &colors.color_critical
    } else if pct <= colors.threshold_low {
        &colors.color_low
    } else {
        &colors.color_healthy
    };
    parse_hex_color(hex).unwrap_or(Color::WHITE)
}

/// Parse a CSS hex colour like `#f38ba8` into an `iced::Color`.
fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::from_rgb8(r, g, b))
}

// ---------------------------------------------------------------------------
// Battery ring element
// ---------------------------------------------------------------------------

fn battery_ring<'a>(
    pct: u8,
    charging: bool,
    colors: &BatteryColors,
    palette: &oxidemx_widgets::palette::Palette,
) -> Element<'a, Message> {
    let fg = ring_fill_color(pct, charging, colors);
    let bg = palette.surface0;
    let ring_size = 88.0_f32;

    Canvas::new(RingPainter { pct, charging, fg, bg })
        .width(Length::Fixed(ring_size))
        .height(Length::Fixed(ring_size))
        .into()
}

// ---------------------------------------------------------------------------
// Main view entry point
// ---------------------------------------------------------------------------

/// Build the full popup element from the current `State`.
pub fn popup_view(state: &State) -> Element<'_, Message> {
    let palette = &state.palette;
    let popup_cfg = &state.config.popup;
    let device = &state.device;
    let gsettings = &state.battery_colors;

    // ── Header: device name + connection status ──────────────────────────
    let device_label = device
        .as_ref()
        .map(|d| d.device_name.as_str())
        .unwrap_or("MX Master 4");

    let connection_label = device
        .as_ref()
        .map(|d| {
            if d.connected {
                format!("● {}", d.connection_type)
            } else {
                "○ Disconnected".to_string()
            }
        })
        .unwrap_or_else(|| "○ –".to_string());

    let header = row![
        container(text(device_label).size(14))
            .style(style::chip(palette))
            .padding([3, 8]),
        Space::new().width(Length::Fill),
        text(connection_label).size(12).style(style::text_dim(palette)),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    // ── Hero: battery ring + meta ────────────────────────────────────────
    let (pct, charging) = device
        .as_ref()
        .map(|d| (d.battery_pct, d.charging))
        .unwrap_or((0, false));

    let ring_el = battery_ring(pct, charging, gsettings, palette);

    let remaining = estimate_remaining(pct, charging, gsettings);

    let hero = row![
        ring_el,
        Space::new().width(Length::Fixed(12.0)),
        column![
            text(device_label).size(15),
            text(remaining).size(12).style(style::text_dim(palette)),
        ]
        .spacing(4),
    ]
    .align_y(Alignment::Center);

    // ── Easy-Switch host buttons ─────────────────────────────────────────
    let host_section: Option<Element<'_, Message>> = if popup_cfg.show_host_buttons {
        let buttons: Vec<Element<'_, Message>> = (0u8..3)
            .map(|i| {
                let label = format!("CH {}", i + 1);
                let is_active = state.active_host == Some(i);
                // btn_primary and btn_secondary return distinct opaque Fn types,
                // so we can't branch between them in a single .style() call.
                // Instead, pre-compute a concrete button::Style via the right
                // factory and pass a capturing closure.
                let accent = palette.accent;
                let on_accent = if palette.is_dark { palette.crust } else { Color::WHITE };
                let border_color = palette.hairline_strong;
                let text_color = palette.text;
                let row_hover = palette.row_hover;
                let btn = button(text(label).size(13))
                    .style(move |_, status: iced::widget::button::Status| {
                        if is_active {
                            let pressed = matches!(status, iced::widget::button::Status::Pressed);
                            let mut bg = accent;
                            if pressed { bg.a = 0.9; }
                            iced::widget::button::Style {
                                background: Some(iced::Background::Color(bg)),
                                text_color: on_accent,
                                border: iced::Border {
                                    color: accent,
                                    width: 1.0,
                                    radius: 6.0.into(),
                                },
                                ..Default::default()
                            }
                        } else {
                            let hovered = matches!(status, iced::widget::button::Status::Hovered);
                            iced::widget::button::Style {
                                background: if hovered {
                                    Some(iced::Background::Color(row_hover))
                                } else {
                                    None
                                },
                                text_color,
                                border: iced::Border {
                                    color: if hovered { accent } else { border_color },
                                    width: 1.0,
                                    radius: 6.0.into(),
                                },
                                ..Default::default()
                            }
                        }
                    })
                    .on_press(Message::SwitchHost(i))
                    .width(Length::Fill);
                btn.into()
            })
            .collect();

        let seg = row(buttons).spacing(4);
        Some(
            column![
                text("Easy-Switch").size(12).style(style::text_dim(palette)),
                seg,
            ]
            .spacing(6)
            .into(),
        )
    } else {
        None
    };

    // ── Quick toggles ────────────────────────────────────────────────────
    let active_toggles = match popup_cfg.mode {
        PopupMode::Simple => &popup_cfg.simple_toggles,
        PopupMode::Power => &popup_cfg.power_toggles,
    };

    let toggle_rows: Vec<Element<'_, Message>> = active_toggles
        .iter()
        .filter_map(|id| {
            let entry = QUICK_TOGGLE_CATALOG.iter().find(|e| e.id == id.as_str())?;
            let is_on = *state.toggle_states.get(id.as_str()).unwrap_or(&false);
            let id_owned = id.clone();
            Some(
                row![
                    text(entry.icon).size(14).style(style::text_dim(palette)),
                    Space::new().width(Length::Fixed(8.0)),
                    text(entry.label).size(13),
                    Space::new().width(Length::Fill),
                    toggler(is_on)
                        .on_toggle(move |v| Message::ToggleAction(id_owned.clone(), v))
                        .style(style::toggler_style(palette))
                        .size(20),
                ]
                .align_y(Alignment::Center)
                .spacing(4)
                .padding([6, 0])
                .into(),
            )
        })
        .collect();

    let toggles_section = column![
        text("Quick toggles").size(12).style(style::text_dim(palette)),
        column(toggle_rows).spacing(0),
    ]
    .spacing(6);

    // ── Quick sliders (Power mode only) ─────────────────────────────────
    let sliders_section: Option<Element<'_, Message>> =
        if matches!(popup_cfg.mode, PopupMode::Power) {
            let slider_rows: Vec<Element<'_, Message>> = popup_cfg
                .power_sliders
                .iter()
                .filter_map(|id| {
                    let entry = QUICK_SLIDER_CATALOG.iter().find(|e| e.id == id.as_str())?;
                    match id.as_str() {
                        "dpi" => {
                            let val = state.dpi as u32;
                            Some(labeled_int_slider(
                                entry.label,
                                val,
                                200..=6400,
                                |v| format!("{v} dpi"),
                                |v| Message::SliderDpi(v as u16),
                            ))
                        }
                        "scroll" => {
                            let val = state.scroll_sensitivity as u32;
                            Some(labeled_int_slider(
                                entry.label,
                                val,
                                1..=10,
                                |v| format!("{v}"),
                                |v| Message::SliderScroll(v as u8),
                            ))
                        }
                        "haptic_i" => {
                            let val = state.haptic_intensity as u32;
                            Some(labeled_int_slider(
                                entry.label,
                                val,
                                0..=5,
                                |v| haptic_intensity_label(v as u8),
                                |v| Message::SliderHapticIntensity(v as u8),
                            ))
                        }
                        "accel" => {
                            // Map -1.0..1.0 → 0..=100 for the integer slider.
                            let val = ((state.pointer_accel + 1.0) * 50.0).round() as u32;
                            Some(labeled_int_slider(
                                entry.label,
                                val.clamp(0, 100),
                                0..=100,
                                |v| format!("{:.2}", (v as f32 / 50.0) - 1.0),
                                |v| Message::SliderAccel((v as f32 / 50.0) - 1.0),
                            ))
                        }
                        _ => None,
                    }
                })
                .collect();

            if !slider_rows.is_empty() {
                Some(
                    column![
                        text("Quick sliders").size(12).style(style::text_dim(palette)),
                        column(slider_rows).spacing(10),
                    ]
                    .spacing(6)
                    .into(),
                )
            } else {
                None
            }
        } else {
            None
        };

    // ── Footer ───────────────────────────────────────────────────────────
    let version = env!("CARGO_PKG_VERSION");
    let footer = row![
        button(text("Settings").size(12))
            .style(style::btn_flat(palette))
            .on_press(Message::OpenSettings),
        Space::new().width(Length::Fill),
        text(format!("v{version}"))
            .size(11)
            .style(style::text_faint(palette)),
    ]
    .align_y(Alignment::Center);

    // ── Assemble ─────────────────────────────────────────────────────────
    let divider = || rule::horizontal(1).style(style::rule_style(palette));

    let mut content_col: Vec<Element<'_, Message>> = vec![
        header.into(),
        divider().into(),
        hero.into(),
    ];

    if let Some(hosts) = host_section {
        content_col.push(divider().into());
        content_col.push(hosts);
    }

    content_col.push(divider().into());
    content_col.push(toggles_section.into());

    if let Some(sliders) = sliders_section {
        content_col.push(divider().into());
        content_col.push(sliders);
    }

    content_col.push(divider().into());
    content_col.push(footer.into());

    let inner = column(content_col).spacing(10).padding(16);

    // Scrollable so nothing clips if the popup shrinks on a small panel.
    let scroll = scrollable(inner).width(Length::Fill).height(Length::Fill);

    container(scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::card(palette))
        .into()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn estimate_remaining(pct: u8, charging: bool, colors: &BatteryColors) -> String {
    if charging {
        return "Charging…".to_string();
    }
    if pct <= colors.threshold_critical {
        "About 4 hours left".to_string()
    } else if pct <= colors.threshold_low {
        "About 1 day left".to_string()
    } else {
        "About 3–5 days left".to_string()
    }
}

fn haptic_intensity_label(v: u8) -> String {
    match v {
        0 => "Off".to_string(),
        1 => "Light".to_string(),
        2 => "Soft".to_string(),
        3 => "Medium".to_string(),
        4 => "Firm".to_string(),
        _ => "Strong".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_color_valid() {
        let c = parse_hex_color("#f38ba8").unwrap();
        assert!((c.r - 0xf3 as f32 / 255.0).abs() < 0.01);
    }

    #[test]
    fn parse_hex_color_invalid() {
        assert!(parse_hex_color("nothex").is_none());
        assert!(parse_hex_color("#12345").is_none());
    }

    #[test]
    fn estimate_remaining_charging() {
        let colors = BatteryColors::default();
        assert!(estimate_remaining(50, true, &colors).contains("Charging"));
    }

    #[test]
    fn estimate_remaining_low() {
        let colors = BatteryColors { threshold_low: 30, ..BatteryColors::default() };
        let s = estimate_remaining(20, false, &colors);
        assert!(s.contains("1 day"), "got: {s}");
    }

    #[test]
    fn haptic_intensity_label_bounds() {
        assert_eq!(haptic_intensity_label(0), "Off");
        assert_eq!(haptic_intensity_label(255), "Strong");
    }
}
