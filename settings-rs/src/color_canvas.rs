//! Traditional 2D color picker — saturation/value square plus a
//! hue strip. Renders via two `iced::canvas::Program` impls that
//! draw stacked linear gradients (no per-pixel work) and capture
//! mouse drag to emit HSV updates.
//!
//! The picker is intentionally HSV-driven rather than RGB-driven
//! because that's what users expect from "Photoshop-style" pickers
//! — pick the hue, then dial in saturation/value. The owning view
//! converts HSV → hex on each emitted message and the existing
//! `SetThemeColor` path handles the rest.

use iced::gradient::ColorStop;
use iced::widget::canvas::{self, gradient::Linear, Action, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Event, Point, Rectangle, Renderer, Theme};

/// Standard HSV → RGB conversion. `h`, `s`, `v` all in `[0, 1]`.
/// Returns `[r, g, b]` in `[0, 1]`. Alpha is left to the caller.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = (h.fract() + 1.0).fract();
    let h6 = h * 6.0;
    let c = v * s;
    let x = c * (1.0 - ((h6 % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h6 < 1.0 {
        (c, x, 0.0)
    } else if h6 < 2.0 {
        (x, c, 0.0)
    } else if h6 < 3.0 {
        (0.0, c, x)
    } else if h6 < 4.0 {
        (0.0, x, c)
    } else if h6 < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (r + m, g + m, b + m)
}

/// Standard RGB → HSV conversion. Inputs and outputs all `[0, 1]`.
/// Returns `(h, s, v)`. Hue is undefined for greys; we return 0 in
/// that case so the picker doesn't jump unpredictably when the
/// user lands on a fully-desaturated colour.
pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    let h = (h / 6.0 + 1.0).fract();

    let s = if max == 0.0 { 0.0 } else { delta / max };
    let v = max;
    (h, s, v)
}

/// Render the SV square at a fixed hue. Emits `(s, v)` updates on
/// click + drag. Stateless (shares ownership with parent through
/// `&self`); drag tracking lives in the canvas Program State so
/// the parent doesn't need to thread "is mouse down" through its
/// own `Message` enum.
pub struct HsvSquare<F>
where
    F: Fn(f32, f32) -> crate::Message + 'static,
{
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
    pub on_change: F,
}

#[derive(Debug, Default)]
pub struct DragState {
    pressed: bool,
}

impl<F> canvas::Program<crate::Message, Theme, Renderer> for HsvSquare<F>
where
    F: Fn(f32, f32) -> crate::Message + 'static,
{
    type State = DragState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        let origin = Point::ORIGIN;

        let (hr, hg, hb) = hsv_to_rgb(self.hue, 1.0, 1.0);
        let hue_color = Color::from_rgb(hr, hg, hb);

        // Layer 1: white → fully-saturated hue colour, left → right.
        // canvas::gradient::Linear is point-based: pass the start
        // and end points and stops project along that line.
        let h_grad = Linear::new(
            Point::new(0.0, size.height / 2.0),
            Point::new(size.width, size.height / 2.0),
        )
        .add_stops([
            ColorStop {
                offset: 0.0,
                color: Color::WHITE,
            },
            ColorStop {
                offset: 1.0,
                color: hue_color,
            },
        ]);
        frame.fill_rectangle(origin, size, h_grad);

        // Layer 2: transparent → black, top → bottom. Composites
        // over layer 1 so the bottom edge becomes pure black, the
        // top edge is unchanged, and the whole square is the
        // standard SV gamut.
        let v_grad = Linear::new(
            Point::new(size.width / 2.0, 0.0),
            Point::new(size.width / 2.0, size.height),
        )
        .add_stops([
            ColorStop {
                offset: 0.0,
                color: Color::TRANSPARENT,
            },
            ColorStop {
                offset: 1.0,
                color: Color::BLACK,
            },
        ]);
        frame.fill_rectangle(origin, size, v_grad);

        // Cursor crosshair — small white-ringed black-bordered
        // circle at the current (s, v). Two strokes give it
        // contrast against both bright and dark backgrounds.
        let cx = self.saturation * size.width;
        let cy = (1.0 - self.value) * size.height;
        let cursor_pos = Point::new(cx, cy);
        let r = 6.0;
        let outer = Path::circle(cursor_pos, r);
        frame.stroke(
            &outer,
            Stroke::default()
                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.85))
                .with_width(2.5),
        );
        frame.stroke(
            &outer,
            Stroke::default().with_color(Color::WHITE).with_width(1.0),
        );

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<crate::Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                state.pressed = true;
                let (s, v) = sv_from_pos(pos, bounds);
                Some(Action::publish((self.on_change)(s, v)).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.pressed {
                    state.pressed = false;
                }
                None
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.pressed => {
                // Drag continues even when the cursor leaves the
                // canvas — clamp to bounds so the user can pin the
                // edges by dragging past them.
                let pos = match cursor.position_in(bounds) {
                    Some(p) => p,
                    None => match cursor.position() {
                        Some(p) => Point::new(p.x - bounds.x, p.y - bounds.y),
                        None => return None,
                    },
                };
                let (s, v) = sv_from_pos(pos, bounds);
                Some(Action::publish((self.on_change)(s, v)).and_capture())
            }
            _ => None,
        }
    }
}

fn sv_from_pos(pos: Point, bounds: Rectangle) -> (f32, f32) {
    let s = (pos.x / bounds.width).clamp(0.0, 1.0);
    let v = 1.0 - (pos.y / bounds.height).clamp(0.0, 1.0);
    (s, v)
}

/// Render a rainbow hue strip with a horizontal indicator line at
/// the current hue. Emits `h` updates on click + drag. Vertical
/// orientation — caller stretches it to whatever height matches
/// the SV square.
pub struct HueStrip<F>
where
    F: Fn(f32) -> crate::Message + 'static,
{
    pub hue: f32,
    pub on_change: F,
}

impl<F> canvas::Program<crate::Message, Theme, Renderer> for HueStrip<F>
where
    F: Fn(f32) -> crate::Message + 'static,
{
    type State = DragState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        let origin = Point::ORIGIN;

        // Vertical rainbow — 6 evenly-spaced hue stops + one extra
        // at the bottom to wrap back to red so the strip closes
        // cleanly. iced::Linear caps at 8 stops so this fits.
        let hue_at = |t: f32| {
            let (r, g, b) = hsv_to_rgb(t, 1.0, 1.0);
            Color::from_rgb(r, g, b)
        };
        let grad = Linear::new(
            Point::new(size.width / 2.0, 0.0),
            Point::new(size.width / 2.0, size.height),
        )
        .add_stops([
            ColorStop {
                offset: 0.0,
                color: hue_at(0.0),
            },
            ColorStop {
                offset: 1.0 / 6.0,
                color: hue_at(1.0 / 6.0),
            },
            ColorStop {
                offset: 2.0 / 6.0,
                color: hue_at(2.0 / 6.0),
            },
            ColorStop {
                offset: 3.0 / 6.0,
                color: hue_at(3.0 / 6.0),
            },
            ColorStop {
                offset: 4.0 / 6.0,
                color: hue_at(4.0 / 6.0),
            },
            ColorStop {
                offset: 5.0 / 6.0,
                color: hue_at(5.0 / 6.0),
            },
            ColorStop {
                offset: 1.0,
                color: hue_at(1.0),
            },
        ]);
        frame.fill_rectangle(origin, size, grad);

        // Cursor: short horizontal line at the current hue.
        let y = self.hue.clamp(0.0, 1.0) * size.height;
        let line = Path::new(|p| {
            p.move_to(Point::new(0.0, y));
            p.line_to(Point::new(size.width, y));
        });
        frame.stroke(
            &line,
            Stroke::default()
                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.85))
                .with_width(3.0),
        );
        frame.stroke(
            &line,
            Stroke::default().with_color(Color::WHITE).with_width(1.5),
        );

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<crate::Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                state.pressed = true;
                let h = (pos.y / bounds.height).clamp(0.0, 1.0);
                Some(Action::publish((self.on_change)(h)).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.pressed = false;
                None
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.pressed => {
                let y = match cursor.position_in(bounds) {
                    Some(p) => p.y,
                    None => match cursor.position() {
                        Some(p) => p.y - bounds.y,
                        None => return None,
                    },
                };
                let h = (y / bounds.height).clamp(0.0, 1.0);
                Some(Action::publish((self.on_change)(h)).and_capture())
            }
            _ => None,
        }
    }
}
