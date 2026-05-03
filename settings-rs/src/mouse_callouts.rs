//! Mouse photo + callout overlay (the centerpiece of the legacy
//! Buttons page).
//!
//! Renders the MX Master 4 photo and overlays each remappable
//! button's label as a small chip linked to the on-photo button by
//! a thin connector line. Positions + line directions ported from
//! `overlay/settings_constants.py::_BASE_MOUSE_BUTTONS` so the
//! visual lines up with the legacy dashboard.
//!
//! Implemented as a `canvas::Program` so we get pixel-precise
//! control over chip placement, connector geometry, and palette-
//! aware colouring without fighting iced's layout system.

use crate::palette::Palette;
use iced::widget::canvas::{self, path::Builder, Frame, Geometry, Path, Stroke, Text};
use iced::widget::image as iced_image;
use iced::{mouse, Color, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

/// Where the connector line attaches relative to the dot on the
/// photo. Mirrors the legacy `line_from` strings.
#[derive(Debug, Clone, Copy)]
pub enum LineFrom {
    /// Line goes straight up from the dot, label sits above.
    Top,
    /// Line goes straight left from the dot, label sits left.
    Left,
    /// L-shape: horizontal-left then vertical-up to the label
    /// (used for thumb-area dots whose labels stack above the
    /// visible mouse silhouette).
    LeftUp { label_y_frac: f32 },
}

/// One button's callout. `pos` is normalised into the rendered
/// image rect (0..1), so the same definition works at any image
/// scale. Mirrors `MOUSE_BUTTONS` in the legacy code.
#[derive(Debug, Clone)]
pub struct Callout {
    pub label: &'static str,
    pub pos: (f32, f32),
    pub line_from: LineFrom,
}

/// Canonical list for the MX Master 4 — copied verbatim from
/// `_BASE_MOUSE_BUTTONS` so the dot positions match the legacy
/// settings_widgets.py rendering.
pub const MX_MASTER_4_BUTTONS: &[Callout] = &[
    Callout {
        label: "Middle Button",
        pos: (0.58, 0.19),
        line_from: LineFrom::Top,
    },
    Callout {
        label: "Shift Wheel Mode",
        pos: (0.58, 0.36),
        line_from: LineFrom::Top,
    },
    Callout {
        label: "Forward",
        pos: (0.23, 0.40),
        line_from: LineFrom::Left,
    },
    Callout {
        label: "Horizontal Scroll",
        pos: (0.24, 0.47),
        line_from: LineFrom::Left,
    },
    Callout {
        label: "Back",
        pos: (0.27, 0.54),
        line_from: LineFrom::Left,
    },
    Callout {
        label: "Gestures",
        pos: (0.26, 0.36),
        line_from: LineFrom::LeftUp { label_y_frac: 0.34 },
    },
    Callout {
        label: "Show Actions Ring",
        pos: (0.28, 0.42),
        line_from: LineFrom::LeftUp { label_y_frac: 0.26 },
    },
];

/// Canvas program that paints the mouse photo + callouts.
pub struct MousePainter {
    image: canvas::Image,
    image_aspect: f32,
    callouts: &'static [Callout],
    accent: Color,
    chip_bg: Color,
    chip_border: Color,
    chip_text: Color,
    chip_shadow: Color,
}

impl MousePainter {
    /// Construct a painter for the MX Master 4. `image_path` should
    /// point at `assets/devices/logitechmouse.png`. Pixel dimensions
    /// drive the aspect ratio used in `draw()`; if the image can't
    /// be opened we fall back to a 4:3 box so the layout doesn't
    /// collapse.
    pub fn mx_master_4(palette: &Palette, image_path: std::path::PathBuf) -> Self {
        let aspect = probe_aspect(&image_path).unwrap_or(1.33);
        let handle = canvas::Image::new(iced_image::Handle::from_path(image_path));
        Self {
            image: handle,
            image_aspect: aspect,
            callouts: MX_MASTER_4_BUTTONS,
            accent: palette.accent,
            chip_bg: palette.mantle,
            chip_border: palette.hairline_strong,
            chip_text: palette.text,
            chip_shadow: Color::from_rgba(0.0, 0.0, 0.0, 0.4),
        }
    }
}

impl<Message> canvas::Program<Message> for MousePainter {
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

        // Compute the image rect — the largest rectangle of the
        // photo's aspect ratio that fits in `bounds`, centred.
        // Matches the legacy "fit + centre" placement.
        let img_rect = fit_image_rect(bounds.size(), self.image_aspect);

        frame.draw_image(img_rect, self.image.clone());

        for callout in self.callouts {
            draw_callout(&mut frame, &img_rect, callout, self);
        }

        vec![frame.into_geometry()]
    }
}

// ============================================================================
// Layout + drawing helpers
// ============================================================================

fn fit_image_rect(canvas_size: Size, aspect: f32) -> Rectangle {
    let canvas_aspect = canvas_size.width / canvas_size.height;
    if aspect > canvas_aspect {
        // Photo is wider than canvas — width-bound.
        let w = canvas_size.width;
        let h = w / aspect;
        Rectangle {
            x: 0.0,
            y: (canvas_size.height - h) / 2.0,
            width: w,
            height: h,
        }
    } else {
        // Photo is taller — height-bound.
        let h = canvas_size.height;
        let w = h * aspect;
        Rectangle {
            x: (canvas_size.width - w) / 2.0,
            y: 0.0,
            width: w,
            height: h,
        }
    }
}

fn draw_callout(frame: &mut Frame, img_rect: &Rectangle, c: &Callout, painter: &MousePainter) {
    let dot_x = img_rect.x + c.pos.0 * img_rect.width;
    let dot_y = img_rect.y + c.pos.1 * img_rect.height;

    // Estimate text width — iced's canvas text doesn't expose
    // measure mid-paint, so we approximate from char count. Slight
    // over-estimation; chips look generously padded which matches
    // the legacy look.
    let text_size = 11.0_f32;
    let est_text_w = c.label.chars().count() as f32 * text_size * 0.55;
    let pad_x = 12.0;
    let pad_y = 6.0;
    let chip_w = est_text_w + pad_x * 2.0;
    let chip_h = text_size + pad_y * 2.0;
    let line_length = 60.0;

    let (chip_x, chip_y, line_path) = match c.line_from {
        LineFrom::Top => {
            let cx = dot_x - chip_w / 2.0;
            let cy = dot_y - line_length - chip_h;
            let mut b = Builder::new();
            b.move_to(Point::new(dot_x, dot_y - 6.0));
            b.line_to(Point::new(dot_x, cy + chip_h));
            (cx, cy, b.build())
        }
        LineFrom::Left => {
            let cx = dot_x - line_length - chip_w;
            let cy = dot_y - chip_h / 2.0;
            let mut b = Builder::new();
            b.move_to(Point::new(dot_x - 6.0, dot_y));
            b.line_to(Point::new(cx + chip_w, dot_y));
            (cx, cy, b.build())
        }
        LineFrom::LeftUp { label_y_frac } => {
            let cx = dot_x - line_length - chip_w;
            let cy = img_rect.y + label_y_frac * img_rect.height - chip_h / 2.0;
            let mid_x = cx + chip_w + 15.0;
            let end_x = cx + chip_w;
            let end_y = cy + chip_h / 2.0;
            let mut b = Builder::new();
            b.move_to(Point::new(dot_x - 6.0, dot_y));
            b.line_to(Point::new(mid_x, dot_y));
            b.line_to(Point::new(mid_x, end_y));
            b.line_to(Point::new(end_x, end_y));
            (cx, cy, b.build())
        }
    };

    // Chip shadow.
    let shadow_rect = rounded_rect(chip_x + 2.0, chip_y + 3.0, chip_w, chip_h, 8.0);
    frame.fill(&shadow_rect, painter.chip_shadow);

    // Chip background.
    let chip_path = rounded_rect(chip_x, chip_y, chip_w, chip_h, 8.0);
    frame.fill(&chip_path, painter.chip_bg);
    frame.stroke(
        &chip_path,
        Stroke::default()
            .with_color(painter.chip_border)
            .with_width(1.0),
    );

    // Connector line.
    frame.stroke(
        &line_path,
        Stroke::default()
            .with_color(painter.accent)
            .with_width(1.5),
    );

    // Connector dot at the button position.
    let dot = Path::circle(Point::new(dot_x, dot_y), 4.0);
    frame.fill(&dot, painter.accent);
    let dot_inner = Path::circle(Point::new(dot_x, dot_y), 2.0);
    frame.fill(&dot_inner, painter.chip_bg);

    // Label text — centred in the chip. We anchor at the chip's
    // top-left + padding; iced's Text rendering doesn't have a
    // baseline-aware centring shortcut so this matches the
    // legacy padding model.
    frame.fill_text(Text {
        content: c.label.to_string(),
        position: Point::new(chip_x + pad_x, chip_y + pad_y - 1.0),
        color: painter.chip_text,
        size: text_size.into(),
        ..Text::default()
    });
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut b = Builder::new();
    b.move_to(Point::new(x + r, y));
    b.line_to(Point::new(x + w - r, y));
    b.arc_to(
        Point::new(x + w, y),
        Point::new(x + w, y + r),
        r,
    );
    b.line_to(Point::new(x + w, y + h - r));
    b.arc_to(
        Point::new(x + w, y + h),
        Point::new(x + w - r, y + h),
        r,
    );
    b.line_to(Point::new(x + r, y + h));
    b.arc_to(
        Point::new(x, y + h),
        Point::new(x, y + h - r),
        r,
    );
    b.line_to(Point::new(x, y + r));
    b.arc_to(Point::new(x, y), Point::new(x + r, y), r);
    b.close();
    b.build()
}

fn probe_aspect(path: &std::path::Path) -> Option<f32> {
    let bytes = std::fs::read(path).ok()?;
    let img = ::image::load_from_memory(&bytes).ok()?;
    let (w, h) = (img.width() as f32, img.height() as f32);
    if h <= 0.0 {
        None
    } else {
        Some(w / h)
    }
}

/// Convenience: a `Canvas` widget at the canonical size used in
/// the Buttons tab. Returns an `Element` ready to drop into the
/// page.
pub fn mouse_widget<'a, Message: 'a>(
    palette: &Palette,
    image_path: std::path::PathBuf,
    width_px: f32,
    height_px: f32,
) -> iced::Element<'a, Message> {
    iced::widget::canvas(MousePainter::mx_master_4(palette, image_path))
        .width(Length::Fixed(width_px))
        .height(Length::Fixed(height_px))
        .into()
}

// `Vector` import isn't used in this build but kept so future
// shadow-offset tweaks don't have to hunt for the import. Suppress
// dead-code warning at module load.
#[allow(dead_code)]
fn _vec_link() -> Vector {
    Vector::ZERO
}
