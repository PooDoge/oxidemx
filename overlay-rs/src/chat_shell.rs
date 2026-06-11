//! Disc → AI-chat "arc shell" morph: geometry, crossfade ramps, and
//! the canvas painter for the two travelling caps.
//!
//! When the active page becomes the AI Assistant page, the radial
//! disc cross-fades into two painted semicircle caps that travel
//! apart and flatten: the top cap becomes a slim header arc (title +
//! drag surface + × button) pinned at the window top, the bottom cap
//! becomes the input footer backdrop at the window bottom, and the
//! conversation fills the space between. One `Tween` on
//! `RadialState::ai_morph` (0.0 = disc, 1.0 = chat) drives the whole
//! timeline; the easing comes from `anim_config.ai_morph`.
//!
//! The 3D-framing shaders never learn about the split — they all
//! multiply by `disc_alpha(t)` and are gone by t ≈ 0.22, before the
//! caps start to travel meaningfully. See
//! `docs/superpowers/specs/2026-06-10-ai-chat-arc-shell-morph-design.md`.
//!
//! The window grows DOWNWARD only (484×484 → 484×760, resized once —
//! never per-frame): the disc's 484×484 layer stack stays anchored at
//! the window's top-left, so the disc centre never moves on screen and
//! no window repositioning is needed. The clamshell opens like a flip
//! phone lying on a table: the top arc rises a little, the chat and
//! footer extend downward.

use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Event, Point, Rectangle, Renderer, Theme};

use crate::geometry::{CENTER, MENU_RADIUS};
use crate::radial::RadialState;

/// Window height while the chat shell is active. Width stays
/// `WINDOW_SIZE`. The window is resized to this once at morph start
/// and restored once the morph fully reverses (or the menu hides).
/// Kept modest — a towering window reads as "stretched" next to the
/// 300 px disc it morphs from.
pub const CHAT_WINDOW_HEIGHT: f64 = 640.0;

/// Final header-arc height (the flattened top cap).
pub const HEADER_H: f32 = 44.0;
/// Final footer-arc height (the flattened bottom cap). Sized to
/// fully back the input zone — the 58 px editor row plus the
/// tool-activity label above it — with enough room below the editor
/// that its corners clear the arc's own rounded corners.
pub const FOOTER_H: f32 = 104.0;
/// Margin between the arcs and the window edges.
pub const EDGE_PAD: f32 = 8.0;
/// Radius of the × close-button hit circle.
pub const CLOSE_HIT_R: f32 = 16.0;
/// Morph progress above which the header interactions (drag, ×,
/// wheel-back over the arcs) are armed. Below this the chat shell is
/// still in flight and clicks would grab a moving target.
pub const INTERACTIVE_T: f32 = 0.95;

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Hermite smoothstep between edges. Standard shader-style ramp so
/// the three crossfades (disc out, caps in, chat in) ease at their
/// boundaries instead of kinking.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Alpha multiplier for the disc (canvas + every shader layer)
/// during the morph. Fully gone by t = 0.18 — before the caps have
/// travelled far enough for the seam to show.
pub fn disc_alpha(t: f32) -> f32 {
    1.0 - smoothstep(0.0, 0.22, t)
}

/// Alpha of the two painted caps. Fades in over the disc while the
/// disc fades out, so the swap reads as one object changing, not two
/// objects crossing.
pub fn cap_alpha(t: f32) -> f32 {
    smoothstep(0.04, 0.22, t)
}

/// Alpha of the chat content (history, input, header title/×).
/// Arrives last, once the arcs are nearly parked.
pub fn chat_alpha(t: f32) -> f32 {
    smoothstep(0.62, 0.97, t)
}

/// The two cap rectangles at morph progress `t` (any easing overshoot
/// is clamped so the caps never poke past the window edges).
///
/// Both caps start as the two halves of the disc (centre `CENTER`,
/// radius `MENU_RADIUS`, anchored in the top-left 484×484 region of
/// the tall window) and travel/flatten to the window edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapRects {
    pub top: Rectangle,
    pub bottom: Rectangle,
}

pub fn cap_rects(t: f32, win_w: f32, win_h: f32) -> CapRects {
    let t = t.clamp(0.0, 1.0);
    let cx = CENTER as f32;
    let cy = CENTER as f32;
    let r = MENU_RADIUS as f32;

    // Width grows from the disc diameter to nearly the full window.
    let w = lerp(2.0 * r, win_w - 2.0 * EDGE_PAD, t);
    let x = cx - w / 2.0;

    // Top cap: its bottom edge travels from the disc centre up to
    // just below the window's top padding while the cap flattens.
    let top_h = lerp(r, HEADER_H, t);
    let top_bottom = lerp(cy, EDGE_PAD + HEADER_H, t);
    let top = Rectangle {
        x,
        y: top_bottom - top_h,
        width: w,
        height: top_h,
    };

    // Bottom cap: its top edge travels from the disc centre down to
    // the window bottom (minus padding and its final height).
    let bot_h = lerp(r, FOOTER_H, t);
    let bot_top = lerp(cy, win_h - EDGE_PAD - FOOTER_H, t);
    let bottom = Rectangle {
        x,
        y: bot_top,
        width: w,
        height: bot_h,
    };

    CapRects { top, bottom }
}

/// Centre of the × close button inside the header arc.
pub fn close_center(top: &Rectangle) -> Point {
    Point::new(
        top.x + top.width - top.height * 0.5 - 6.0,
        top.y + top.height * 0.5,
    )
}

/// Hit-test the × close button.
pub fn hit_close(p: Point, top: &Rectangle) -> bool {
    let c = close_center(top);
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    dx * dx + dy * dy <= CLOSE_HIT_R * CLOSE_HIT_R
}

/// Hit-test the draggable header surface (the arc minus the ×).
pub fn hit_drag(p: Point, top: &Rectangle) -> bool {
    top.contains(p) && !hit_close(p, top)
}

/// True when the point sits on either arc — the wheel-back region.
/// (Wheel over the chat middle scrolls the conversation instead.)
pub fn in_caps(p: Point, rects: &CapRects) -> bool {
    rects.top.contains(p) || rects.bottom.contains(p)
}

/// Canvas painter for the two caps + header furniture. Stacked above
/// the disc layers and below the chat widgets, covering the full
/// (tall) window.
pub struct CapsPainter<'a> {
    state: &'a RadialState,
}

impl<'a> CapsPainter<'a> {
    pub fn new(state: &'a RadialState) -> Self {
        Self { state }
    }

    fn palette_color(&self, hex: &str, fallback: Color) -> Color {
        oxidemx_shared::theme::parse_hex_rgba(hex)
            .map(|(r, g, b, a)| Color::from_rgba(r as f32, g as f32, b as f32, a as f32))
            .unwrap_or(fallback)
    }
}

impl<'a> canvas::Program<crate::app::Message> for CapsPainter<'a> {
    type State = ();

    fn update(
        &self,
        _canvas_state: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<crate::app::Message>> {
        let t = self.state.ai_morph_progress();
        if t < INTERACTIVE_T || !self.state.is_open() {
            return None;
        }

        // Escape works regardless of cursor position — mirrors the
        // main painter's dismiss path (which is gated off while the
        // chat shell is up).
        if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event {
            if matches!(
                key,
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
            ) {
                return Some(Action::publish(crate::app::Message::ToggleDismiss));
            }
            return None;
        }

        let p = cursor.position_in(bounds)?;
        let rects = cap_rects(t, bounds.width, bounds.height);
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if hit_close(p, &rects.top) {
                    // × dismisses the overlay entirely — same path
                    // as Escape / right-click on the disc.
                    Some(Action::publish(crate::app::Message::ToggleDismiss))
                } else if hit_drag(p, &rects.top) {
                    // Native compositor move, like grabbing a
                    // titlebar.
                    Some(Action::publish(crate::app::Message::ChatHeaderPressed))
                } else {
                    None
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                // Wheel over either arc cycles back toward the disc
                // (the same gesture that brought the user here).
                // Wheel over the middle belongs to the conversation
                // scrollable, which sits above this canvas.
                if !in_caps(p, &rects) {
                    return None;
                }
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                if dy.abs() < f32::EPSILON {
                    return None;
                }
                let direction = if dy > 0.0 { 1 } else { -1 };
                Some(Action::publish(crate::app::Message::CyclePage(direction)))
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        _canvas_state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let t = self.state.ai_morph_progress();
        if t < INTERACTIVE_T {
            return mouse::Interaction::default();
        }
        let Some(p) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        let rects = cap_rects(t, bounds.width, bounds.height);
        if hit_close(p, &rects.top) {
            mouse::Interaction::Pointer
        } else if hit_drag(p, &rects.top) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let t = self.state.ai_morph_progress();
        if t <= 0.001 || !self.state.is_drawable() {
            return vec![frame.into_geometry()];
        }

        // Everything fades with the whole-menu tween too, so a
        // dismiss from chat mode fades the shell out exactly like
        // the disc would.
        let open_alpha = self.state.menu_open_alpha();
        let shell_alpha = cap_alpha(t) * open_alpha;
        if shell_alpha <= 0.001 {
            return vec![frame.into_geometry()];
        }

        let palette = &self.state.theme.theme.colors;
        let accent = self.palette_color(&palette.accent, Color::from_rgb(0.5, 0.5, 1.0));
        let surface0 =
            self.palette_color(&palette.surface0, Color::from_rgba(0.12, 0.12, 0.15, 1.0));
        let text_color = self.palette_color(&palette.text, Color::WHITE);

        // Cap fill: theme accent blended toward the surface colour so
        // the caps read as "made from the disc" without shouting.
        let blend = |a: Color, b: Color, k: f32| {
            Color::from_rgba(
                lerp(a.r, b.r, k),
                lerp(a.g, b.g, k),
                lerp(a.b, b.b, k),
                1.0,
            )
        };
        let cap_main = blend(accent, surface0, 0.55);
        let cap_deep = blend(accent, surface0, 0.82);

        let rects = cap_rects(t, bounds.width, bounds.height);
        let chat_a = chat_alpha(t) * open_alpha;

        // Bottom corner radius of the top cap (and mirrored for the
        // bottom cap) grows from 0 (a true half-disc) to a slight
        // rounding so the parked arcs don't have razor corners.
        let minor_r = lerp(0.0, 10.0, t);

        for (rect, is_top) in [(rects.top, true), (rects.bottom, false)] {
            // Rounded corner radius on the "outer" side: starts
            // equal to the cap height (height = MENU_RADIUS = half
            // the width at t=0, i.e. exactly a semicircle) but
            // shrinks FASTER than the height as the cap flattens —
            // a radius equal to the full height all the way down
            // renders the parked arcs as bulgy half-pills (reads
            // as vertical stretching) instead of slim bars. The
            // footer is taller than the header (it backs the whole
            // input zone), so its end factor is halved to keep it
            // reading as a bar rather than a bowl.
            let end_factor = if is_top { 0.55 } else { 0.275 };
            let major_r = (rect.height * lerp(1.0, end_factor, t)).min(rect.width / 2.0);
            let radius = if is_top {
                iced::border::Radius::default()
                    .top_left(major_r)
                    .top_right(major_r)
                    .bottom_left(minor_r)
                    .bottom_right(minor_r)
            } else {
                iced::border::Radius::default()
                    .bottom_left(major_r)
                    .bottom_right(major_r)
                    .top_left(minor_r)
                    .top_right(minor_r)
            };
            let path = Path::new(|b| {
                b.rounded_rectangle(rect.position(), rect.size(), radius);
            });
            frame.fill(&path, scale_alpha(cap_deep, shell_alpha));

            // Lit band along the split seam (where the disc centre
            // used to be) — fakes the dome's lighting falloff
            // without a per-path gradient: a thinner inset rounded
            // rect in the lighter blend, hugging the seam edge.
            let band_h = (rect.height * 0.42).min(40.0);
            let band_rect = if is_top {
                Rectangle {
                    x: rect.x + 2.0,
                    y: rect.y + rect.height - band_h,
                    width: rect.width - 4.0,
                    height: band_h,
                }
            } else {
                Rectangle {
                    x: rect.x + 2.0,
                    y: rect.y,
                    width: rect.width - 4.0,
                    height: band_h,
                }
            };
            let band_r = (band_h * 0.5).min(12.0);
            let band = Path::new(|b| {
                b.rounded_rectangle(
                    band_rect.position(),
                    band_rect.size(),
                    band_r.into(),
                );
            });
            // The band sells the dome's lighting while the caps
            // travel, but parked arcs look cleaner flat — its
            // internal edge otherwise reads as a second, stretched
            // outline. Fade it out as the chat content arrives.
            let band_a = 0.6 * shell_alpha * (1.0 - chat_alpha(t));
            frame.fill(&band, scale_alpha(cap_main, band_a));

            // Hairline accent stroke along the flat (seam) edge —
            // reads as the glowing split line while the caps travel,
            // settles into a subtle divider when parked.
            let seam_y = if is_top { rect.y + rect.height } else { rect.y };
            let seam = Path::line(
                Point::new(rect.x + minor_r, seam_y),
                Point::new(rect.x + rect.width - minor_r, seam_y),
            );
            frame.stroke(
                &seam,
                Stroke::default()
                    .with_color(scale_alpha(accent, 0.55 * shell_alpha))
                    .with_width(1.5),
            );
        }

        // Header furniture appears with the chat content.
        if chat_a > 0.001 {
            let top = rects.top;

            // Page title, left-aligned inside the arc.
            frame.fill_text(iced::widget::canvas::Text {
                content: "AI Assistant".to_string(),
                position: Point::new(top.x + 24.0, top.y + top.height / 2.0 - 8.0),
                color: scale_alpha(text_color, chat_a),
                size: 15.0.into(),
                font: iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                },
                ..iced::widget::canvas::Text::default()
            });

            // Drag handle: small centred bar, the universal
            // "grab here" affordance.
            let handle_w = 44.0;
            let handle = Path::new(|b| {
                b.rounded_rectangle(
                    Point::new(top.x + (top.width - handle_w) / 2.0, top.y + 10.0),
                    iced::Size::new(handle_w, 4.0),
                    2.0.into(),
                );
            });
            frame.fill(&handle, scale_alpha(text_color, 0.35 * chat_a));

            // × close button.
            let c = close_center(&top);
            let ring = Path::circle(c, CLOSE_HIT_R - 3.0);
            frame.fill(&ring, scale_alpha(surface0, 0.85 * chat_a));
            let k = 4.5;
            for (sx, sy) in [(1.0_f32, 1.0_f32), (1.0, -1.0)] {
                let line = Path::line(
                    Point::new(c.x - k * sx, c.y - k * sy),
                    Point::new(c.x + k * sx, c.y + k * sy),
                );
                frame.stroke(
                    &line,
                    Stroke::default()
                        .with_color(scale_alpha(text_color, chat_a))
                        .with_width(2.0),
                );
            }
        }

        vec![frame.into_geometry()]
    }
}

fn scale_alpha(c: Color, a: f32) -> Color {
    Color {
        a: c.a * a.clamp(0.0, 1.0),
        ..c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = crate::geometry::WINDOW_SIZE as f32;
    const H: f32 = CHAT_WINDOW_HEIGHT as f32;

    #[test]
    fn caps_start_as_disc_halves() {
        let r = cap_rects(0.0, W, H);
        let cx = CENTER as f32;
        let cy = CENTER as f32;
        let rad = MENU_RADIUS as f32;
        // Top cap: the disc's upper half.
        assert!((r.top.x - (cx - rad)).abs() < 0.01);
        assert!((r.top.y - (cy - rad)).abs() < 0.01);
        assert!((r.top.width - 2.0 * rad).abs() < 0.01);
        assert!((r.top.height - rad).abs() < 0.01);
        // Bottom cap: the disc's lower half — seam edges touch.
        assert!((r.bottom.y - cy).abs() < 0.01);
        assert!((r.top.y + r.top.height - r.bottom.y).abs() < 0.01);
    }

    #[test]
    fn caps_end_parked_at_edges() {
        let r = cap_rects(1.0, W, H);
        assert!((r.top.y - EDGE_PAD).abs() < 0.01);
        assert!((r.top.height - HEADER_H).abs() < 0.01);
        assert!((r.top.width - (W - 2.0 * EDGE_PAD)).abs() < 0.01);
        let bottom_edge = r.bottom.y + r.bottom.height;
        assert!((bottom_edge - (H - EDGE_PAD)).abs() < 0.01);
        assert!((r.bottom.height - FOOTER_H).abs() < 0.01);
    }

    #[test]
    fn midpoint_is_between_endpoints() {
        let r0 = cap_rects(0.0, W, H);
        let r5 = cap_rects(0.5, W, H);
        let r1 = cap_rects(1.0, W, H);
        assert!(r5.top.y < r0.top.y && r5.top.y > r1.top.y);
        assert!(r5.bottom.y > r0.bottom.y && r5.bottom.y < r1.bottom.y);
        assert!(r5.top.height < r0.top.height && r5.top.height > r1.top.height);
    }

    #[test]
    fn overshoot_is_clamped() {
        // Spring easing can push t past 1.0 — the caps must not
        // leave the window.
        let r = cap_rects(1.2, W, H);
        assert!((r.top.y - EDGE_PAD).abs() < 0.01);
        let bottom_edge = r.bottom.y + r.bottom.height;
        assert!(bottom_edge <= H - EDGE_PAD + 0.01);
    }

    #[test]
    fn close_hit_and_drag_are_disjoint() {
        let r = cap_rects(1.0, W, H);
        let c = close_center(&r.top);
        assert!(hit_close(c, &r.top));
        assert!(!hit_drag(c, &r.top));
        // A point on the far left of the header drags, not closes.
        let left = Point::new(r.top.x + 12.0, r.top.y + r.top.height / 2.0);
        assert!(hit_drag(left, &r.top));
        assert!(!hit_close(left, &r.top));
        // Outside both caps: neither.
        let mid = Point::new(W / 2.0, H / 2.0);
        assert!(!hit_drag(mid, &r.top));
        assert!(!in_caps(mid, &r) || r.top.contains(mid) || r.bottom.contains(mid));
    }

    #[test]
    fn alpha_ramps_have_correct_endpoints() {
        assert!((disc_alpha(0.0) - 1.0).abs() < 1e-6);
        assert!(disc_alpha(0.25) < 0.01);
        assert!(cap_alpha(0.0) < 0.01);
        assert!((cap_alpha(0.25) - 1.0).abs() < 0.01);
        assert!(chat_alpha(0.6) < 0.01);
        assert!((chat_alpha(1.0) - 1.0).abs() < 1e-6);
    }
}
