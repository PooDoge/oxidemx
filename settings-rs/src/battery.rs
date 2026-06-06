//! UPower battery readings + theme-aware battery indicator widget.
//!
//! - Data: `poll()` returns the highest-percentage Logitech mouse
//!   battery currently exposed by UPower (so an unplugged battery
//!   doesn't shadow a connected one). Returns `None` when UPower
//!   isn't reachable or no Logitech device is paired.
//!
//! - Widget: `widget()` renders a small battery icon (Canvas) at
//!   the requested size, with a fill rect scaled to the percentage
//!   and color thresholds (green > 30 %, yellow 15-30 %, red ≤ 15 %).
//!   Strokes flip to white on dark themes / black on light themes.

use oxidemx_widgets::palette::Palette;
use iced::widget::canvas::{self, path::Builder, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Length, Point, Rectangle, Renderer, Theme};

// ============================================================================
// Data: UPower probe
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct BatteryStatus {
    pub percent: u8,
    pub charging: bool,
}

/// Probe UPower's system bus for a Logitech HID device's battery.
/// Returns the highest-percentage match. Synchronous; called from
/// a periodic Task in the iced app.
pub async fn poll() -> Option<BatteryStatus> {
    let conn = zbus::Connection::system().await.ok()?;

    let upower = zbus::Proxy::new(
        &conn,
        "org.freedesktop.UPower",
        "/org/freedesktop/UPower",
        "org.freedesktop.UPower",
    )
    .await
    .ok()?;

    let device_paths: Vec<zbus::zvariant::OwnedObjectPath> =
        upower.call("EnumerateDevices", &()).await.ok()?;

    let mut best: Option<BatteryStatus> = None;
    for path in device_paths {
        let dev = match zbus::Proxy::new(
            &conn,
            "org.freedesktop.UPower",
            path.as_str(),
            "org.freedesktop.UPower.Device",
        )
        .await
        {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Filter to peripherals — exclude the laptop battery + AC line.
        // Type 5 = mouse, 6 = keyboard, 7 = tablet, 8 = computer
        // accessory. We want 5 specifically.
        let kind: u32 = dev.get_property("Type").await.unwrap_or(0);
        if kind != 5 {
            continue;
        }
        // Optional brand check — skip non-Logitech mice if any.
        let vendor: String = dev.get_property("Vendor").await.unwrap_or_default();
        if !vendor.is_empty()
            && !vendor.to_lowercase().contains("logi")
            && !vendor.to_lowercase().contains("logitech")
        {
            continue;
        }

        let percent: f64 = dev.get_property("Percentage").await.unwrap_or(0.0);
        // UPower State enum: 1=Charging, 2=Discharging, 4=FullyCharged, ...
        let state: u32 = dev.get_property("State").await.unwrap_or(0);
        let charging = matches!(state, 1 | 4 | 5);

        let pct = percent.clamp(0.0, 100.0).round() as u8;
        let candidate = BatteryStatus { percent: pct, charging };
        match best {
            Some(prev) if prev.percent >= pct => {}
            _ => best = Some(candidate),
        }
    }
    best
}

// ============================================================================
// Widget
// ============================================================================

/// Returns an iced Canvas widget with the battery icon. `width_px`
/// drives both the canvas width and the battery aspect ratio
/// (height ≈ width × 0.5).
pub fn widget<'a, Message: 'a>(
    palette: &Palette,
    status: Option<BatteryStatus>,
    width_px: f32,
) -> iced::Element<'a, Message> {
    let painter = BatteryPainter {
        status,
        stroke: if palette.is_dark {
            Color::WHITE
        } else {
            Color::BLACK
        },
        green: palette.green,
        yellow: palette.yellow,
        red: palette.red,
        text: palette.text,
        text_dim: palette.subtext0,
    };
    let height = width_px * 0.5;
    iced::widget::canvas(painter)
        .width(Length::Fixed(width_px))
        .height(Length::Fixed(height))
        .into()
}

#[allow(dead_code)]
struct BatteryPainter {
    status: Option<BatteryStatus>,
    stroke: Color,
    green: Color,
    yellow: Color,
    red: Color,
    text: Color,
    text_dim: Color,
}

impl<Message> canvas::Program<Message> for BatteryPainter {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        // The whole canvas is the battery icon. The body fills
        // ~92 % of the width (leaving room for the nub on the
        // right edge), the nub takes the remaining ~8 %.
        let total_w = bounds.width;
        let total_h = bounds.height;
        let body_w = total_w * 0.90;
        let body_h = total_h * 0.78;
        let body_x = 0.5;
        let body_y = (total_h - body_h) / 2.0;
        let nub_w = total_w * 0.06;
        let nub_h = body_h * 0.45;
        let nub_x = body_x + body_w + 1.0;
        let nub_y = body_y + (body_h - nub_h) / 2.0;

        // Outer body.
        let body = rounded_rect(body_x, body_y, body_w, body_h, 2.0);
        frame.stroke(
            &body,
            Stroke::default().with_color(self.stroke).with_width(1.5),
        );

        // Terminal nub.
        let nub = rounded_rect(nub_x, nub_y, nub_w, nub_h, 1.0);
        frame.fill(&nub, self.stroke);

        // Fill bar — width scales with percentage.
        if let Some(s) = self.status {
            let inset = 2.0;
            let max_w = body_w - inset * 2.0;
            let pct = (s.percent as f32 / 100.0).clamp(0.0, 1.0);
            let fill_w = max_w * pct;
            if fill_w > 0.5 {
                // Fill colour: when charging, force the accent
                // green regardless of percent so the swap from
                // "discharging-yellow/red" → "charging-green" is
                // unmistakable. Otherwise stay on the standard
                // capacity thresholds.
                let fill_color = if s.charging {
                    self.green
                } else if s.percent <= 15 {
                    self.red
                } else if s.percent <= 30 {
                    self.yellow
                } else {
                    self.green
                };
                let bar = rounded_rect(
                    body_x + inset,
                    body_y + inset,
                    fill_w,
                    body_h - inset * 2.0,
                    1.0,
                );
                frame.fill(&bar, fill_color);
            }

            // Charging indicator — a centred lightning-bolt path
            // overlaid on the body, drawn before the percent text.
            // Visible immediately at any icon size; the previous
            // `⚡` glyph prefix was easy to miss at the 56 px
            // sidebar render. Yellow on dark themes / accent on
            // light, so it punches through whatever fill colour
            // sits underneath.
            if s.charging {
                let bolt = lightning_bolt(
                    body_x + body_w / 2.0,
                    body_y + body_h / 2.0,
                    body_h * 0.55,
                );
                // Black outline first so the bolt reads against
                // any fill.
                frame.stroke(
                    &bolt,
                    Stroke::default()
                        .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.8))
                        .with_width(2.4),
                );
                // Bright fill on top.
                frame.fill(&bolt, self.yellow);
            }

            // Percentage text — sits below the lightning bolt
            // when charging (smaller so they share the body),
            // centre when not. Rendered with a subtle shadow for
            // readability over the colour bar.
            let label = if s.percent >= 100 {
                "100%".to_string()
            } else {
                format!("{}%", s.percent)
            };
            let label_size = if s.charging {
                body_h * 0.40
            } else {
                body_h * 0.55
            };
            let approx_w = label.chars().count() as f32 * label_size * 0.55;
            let cx = body_x + body_w / 2.0 - approx_w / 2.0;
            // When charging: percent text under the bolt. Otherwise
            // centred vertically.
            let cy = if s.charging {
                body_y + body_h - label_size * 1.05
            } else {
                body_y + body_h / 2.0 - label_size * 0.6
            };

            // Subtle shadow for readability over the colour bar.
            frame.fill_text(canvas::Text {
                content: label.clone(),
                position: Point::new(cx + 0.5, cy + 0.5),
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.55),
                size: label_size.into(),
                ..canvas::Text::default()
            });
            // Main glyph — opposite of the body stroke so it stays
            // visible on dark + light themes.
            let label_color = if self.stroke == Color::WHITE {
                Color::WHITE
            } else {
                Color::BLACK
            };
            frame.fill_text(canvas::Text {
                content: label,
                position: Point::new(cx, cy),
                color: label_color,
                size: label_size.into(),
                ..canvas::Text::default()
            });

        } else {
            // No data — single dim diagonal across the body.
            let mut b = Builder::new();
            b.move_to(Point::new(body_x + 2.0, body_y + 2.0));
            b.line_to(Point::new(body_x + body_w - 2.0, body_y + body_h - 2.0));
            frame.stroke(
                &b.build(),
                Stroke::default()
                    .with_color(self.text_dim)
                    .with_width(1.0),
            );
        }

        vec![frame.into_geometry()]
    }
}


// ============================================================================
// Path helpers
// ============================================================================

/// Stylised lightning-bolt path centred at `(cx, cy)` with the
/// given total `height`. The bolt's width is ~0.55 × height so
/// it reads as a tall narrow zig-zag. Designed for overlay use:
/// stroke + fill against any background fill colour.
fn lightning_bolt(cx: f32, cy: f32, height: f32) -> Path {
    let h = height.max(4.0);
    let w = h * 0.55;
    // Six-point zig-zag: top → upper-right notch → middle-left
    // → middle-right notch → bottom → lower-left notch → close.
    let pts: [(f32, f32); 6] = [
        ( 0.10, -0.50), // top tip (slightly off-centre)
        (-0.40,  0.05), // upper-left valley
        (-0.05,  0.05), // mid-shelf
        (-0.18,  0.50), // bottom tip
        ( 0.30, -0.05), // lower-right plateau
        (-0.05, -0.05), // mid-shelf back
    ];
    let mut b = Builder::new();
    let p0 = pts[0];
    b.move_to(Point::new(cx + p0.0 * w, cy + p0.1 * h));
    for &(px, py) in pts.iter().skip(1) {
        b.line_to(Point::new(cx + px * w, cy + py * h));
    }
    b.close();
    b.build()
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut b = Builder::new();
    b.move_to(Point::new(x + r, y));
    b.line_to(Point::new(x + w - r, y));
    b.arc_to(Point::new(x + w, y), Point::new(x + w, y + r), r);
    b.line_to(Point::new(x + w, y + h - r));
    b.arc_to(Point::new(x + w, y + h), Point::new(x + w - r, y + h), r);
    b.line_to(Point::new(x + r, y + h));
    b.arc_to(Point::new(x, y + h), Point::new(x, y + h - r), r);
    b.line_to(Point::new(x, y + r));
    b.arc_to(Point::new(x, y), Point::new(x + r, y), r);
    b.close();
    b.build()
}

