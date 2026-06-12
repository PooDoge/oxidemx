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

/// Legacy/default window height while the chat shell is active —
/// used when no `overlay.chat_size` has been persisted yet. Kept
/// modest — a towering window reads as "stretched" next to the
/// 300 px disc it morphs from.
pub const CHAT_WINDOW_HEIGHT: f64 = 640.0;

/// Minimum chat size the resize grip will commit (design contract:
/// 420×560). Width is additionally floored at `WINDOW_SIZE` because
/// the disc needs its full 484 px square — the window IS the chat
/// surface and the disc surface at different morph phases.
pub const CHAT_MIN_W: f32 = 420.0;
pub const CHAT_MIN_H: f32 = 560.0;

/// Resolve the persisted `overlay.chat_size` into the actual window
/// size, clamping hand-edited or stale values so the disc square
/// and the chat minimums always fit. This is the single source of
/// truth for the window's created size and `RadialState::win_size`.
pub fn effective_window_size(chat_size: Option<(u32, u32)>) -> iced::Size {
    let (w, h) = chat_size.map(|(w, h)| (w as f32, h as f32)).unwrap_or((
        crate::geometry::WINDOW_SIZE as f32,
        CHAT_WINDOW_HEIGHT as f32,
    ));
    iced::Size::new(
        w.max(CHAT_MIN_W).max(crate::geometry::WINDOW_SIZE as f32),
        h.max(CHAT_MIN_H),
    )
}

/// Final header-arc height (the flattened top cap). 52 px per the
/// redesign — fits the 32 px page puck plus title + status line.
pub const HEADER_H: f32 = 52.0;
/// Final footer-arc height (the flattened bottom cap). Sized to
/// fully back the input zone — the 58 px editor row plus the
/// tool-activity label above it — with enough room below the editor
/// that its corners clear the arc's own rounded corners.
pub const FOOTER_H: f32 = 104.0;
/// Margin between the arcs and the window edges.
pub const EDGE_PAD: f32 = 8.0;
/// Morph progress above which the header interactions (drag, ×,
/// wheel-back over the arcs) are armed. Below this the chat shell is
/// still in flight and clicks would grab a moving target.
pub const INTERACTIVE_T: f32 = 0.95;

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Frame-parity cache-buster, 1.0 / 0.999 alternating roughly per
/// frame. iced 0.14's layer caching serves STALE GPU buffers for
/// layers whose content stops changing (frozen mid-morph meshes,
/// quads stuck at mid-fade alpha) while text layers stay fresh —
/// multiplying an imperceptible epsilon into a layer's colors (or
/// nudging a hidden vertex) forces its diff to register change
/// every frame. Remove once upstream layer caching is fixed.
pub fn cache_epsilon() -> f32 {
    let parity = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_millis())
        .unwrap_or(0)
        / 16)
        % 2;
    if parity == 0 {
        1.0
    } else {
        0.999
    }
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
    // The disc is anchored in the window's top-left 484 px square
    // (centering it trips an iced canvas-mesh clipping bug — see
    // app.rs). The caps start as the disc's halves there and sweep
    // right/down to fill the whole window; the clamshell opens
    // downward.
    let cx = CENTER as f32;
    let cy = CENTER as f32;
    let r = MENU_RADIUS as f32;

    // Width grows from the disc diameter to nearly the full window;
    // the left edge travels from the disc's rim to the window pad
    // (deriving x from a fixed centre would push it negative on
    // wide windows mid-morph).
    let w = lerp(2.0 * r, win_w - 2.0 * EDGE_PAD, t);
    let x = lerp(cx - r, EDGE_PAD, t);

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

/// Centre + radius of the travelling page puck at morph progress
/// `t`. The puck is the disc's centre circle surviving the morph:
/// it starts at the disc centre (radius `CENTER_ZONE_RADIUS`) and
/// flies into the header arc's left slot, shrinking to the 32 px
/// header puck. Driven by the same tween as `cap_rects` so the
/// whole shell reads as one object rearranging.
pub fn puck_geom(t: f32, win_w: f32, win_h: f32) -> (Point, f32) {
    let t = t.clamp(0.0, 1.0);
    let parked = cap_rects(1.0, win_w, win_h);
    let end = Point::new(
        parked.top.x + 14.0 + crate::handoff::HEADER_PUCK_R,
        parked.top.y + parked.top.height / 2.0,
    );
    let start = Point::new(CENTER as f32, CENTER as f32);
    let r = lerp(
        crate::geometry::CENTER_ZONE_RADIUS as f32,
        crate::handoff::HEADER_PUCK_R,
        t,
    );
    (
        Point::new(lerp(start.x, end.x, t), lerp(start.y, end.y, t)),
        r,
    )
}

/// Hit-test the draggable header surface. The header's buttons are
/// widgets above the canvas and capture their own presses, so the
/// whole arc is fair game here.
pub fn hit_drag(p: Point, top: &Rectangle) -> bool {
    top.contains(p)
}

/// Side of the square corner zone (window bottom-right) that grabs
/// the resize grip.
pub const GRIP_ZONE: f32 = 22.0;

/// Hit-test the bottom-right resize grip.
pub fn hit_grip(p: Point, win_w: f32, win_h: f32) -> bool {
    p.x >= win_w - GRIP_ZONE && p.y >= win_h - GRIP_ZONE
}

/// Resolve a wheel delta to a page-cycle direction (+1 next, -1
/// previous), or `None` for a pure-horizontal / zero scroll.
fn wheel_direction(delta: &mouse::ScrollDelta) -> Option<i32> {
    let dy = match delta {
        mouse::ScrollDelta::Lines { y, .. } => *y,
        mouse::ScrollDelta::Pixels { y, .. } => *y,
    };
    if dy.abs() < f32::EPSILON {
        return None;
    }
    Some(if dy > 0.0 { 1 } else { -1 })
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
        let armed = self.state.ai_handoff.is_armed();
        if !self.state.is_open() || t <= 0.001 {
            return None;
        }

        // Grip grab — checked before the handoff branches so the
        // grip works in both the armed and active phases. Only once
        // the shell is parked: a moving grip is not a target. The
        // gesture itself is a native compositor resize, so no
        // client-side drag tracking is needed.
        if t >= INTERACTIVE_T {
            if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
                if let Some(p) = cursor.position_in(bounds) {
                    if hit_grip(p, bounds.width, bounds.height) {
                        return Some(Action::publish(crate::app::Message::ChatResizeStart));
                    }
                }
            }
        }

        // Escape closes the chat at any morph stage once the shell
        // owns the page (armed or parked) — mirrors the main
        // painter's dismiss path, which is gated off during the
        // morph.
        if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event {
            if (armed || t >= INTERACTIVE_T)
                && matches!(
                    key,
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                )
            {
                return Some(Action::publish(crate::app::Message::ToggleDismiss));
            }
            return None;
        }

        let p = cursor.position_in(bounds)?;
        let rects = cap_rects(t, bounds.width, bounds.height);

        // While the puck is armed the chat renders but isn't the
        // interaction target yet: wheel input anywhere keeps cycling
        // pages (scrolling away reverses the morph), pointer motion
        // feeds the deliberate-travel activation rule, and a click
        // outside the puck activates the chat.
        if armed {
            return match event {
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    Some(Action::publish(crate::app::Message::HandoffPointer {
                        x: p.x as f64,
                        y: p.y as f64,
                    }))
                }
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    // The × close button is a widget above this
                    // canvas — it captures its own presses (and
                    // closes without requiring activation). Anything
                    // that reaches here feeds the activation rule.
                    Some(Action::publish(crate::app::Message::HandoffClick {
                        x: p.x as f64,
                        y: p.y as f64,
                    }))
                }
                Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                    let direction = wheel_direction(delta)?;
                    Some(Action::publish(crate::app::Message::CyclePage(direction)))
                }
                _ => None,
            };
        }

        if self.state.ai_handoff.is_chat_active() && t >= INTERACTIVE_T {
            return match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    // The × is a widget above this canvas; empty
                    // header surface starts a native compositor
                    // move, like grabbing a titlebar.
                    if hit_drag(p, &rects.top) {
                        Some(Action::publish(crate::app::Message::ChatHeaderPressed))
                    } else {
                        None
                    }
                }
                Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                    // After activation only the header puck keeps
                    // cycling pages — wheel over the body belongs to
                    // the conversation scrollable above this canvas.
                    let (puck_c, puck_r) = puck_geom(t, bounds.width, bounds.height);
                    let (dx, dy) = (p.x - puck_c.x, p.y - puck_c.y);
                    let hit_r = puck_r + crate::handoff::HEADER_PUCK_HIT_SLOP;
                    if dx * dx + dy * dy > hit_r * hit_r {
                        return None;
                    }
                    let direction = wheel_direction(delta)?;
                    Some(Action::publish(crate::app::Message::CyclePage(direction)))
                }
                _ => None,
            };
        }

        // Morph in flight without an armed puck (reversing toward
        // the disc after cycling away): keep the old disc-centre
        // wheel zone live so continued scrolling keeps stepping
        // through pages.
        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
            let cx = CENTER as f32;
            let cy = CENTER as f32;
            let (dx, dy) = (p.x - cx, p.y - cy);
            let zone = crate::geometry::CENTER_ZONE_RADIUS as f32;
            if dx * dx + dy * dy <= zone * zone {
                let direction = wheel_direction(delta)?;
                return Some(Action::publish(crate::app::Message::CyclePage(direction)));
            }
        }
        None
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
        if hit_grip(p, bounds.width, bounds.height) {
            return mouse::Interaction::ResizingDiagonallyDown;
        }
        let rects = cap_rects(t, bounds.width, bounds.height);
        // The header puck is a wheel target in every phase — show a
        // pointer so it reads as interactive.
        let (puck_c, puck_r) = puck_geom(t, bounds.width, bounds.height);
        let (dx, dy) = (p.x - puck_c.x, p.y - puck_c.y);
        let hit_r = puck_r + crate::handoff::HEADER_PUCK_HIT_SLOP;
        if dx * dx + dy * dy <= hit_r * hit_r {
            return mouse::Interaction::Pointer;
        }
        if self.state.ai_handoff.is_chat_active() && hit_drag(p, &rects.top) {
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

        // Cache-buster FIRST — even for "empty" frames: iced 0.14's
        // layer diffing keeps presenting STALE buffers for layers
        // that stop changing, and re-draws remnants of layers that
        // left the tree (the dismissed chat's body quad showed as a
        // lingering black box). An invisible sub-pixel vertex that
        // alternates every frame keeps this mesh layer alive and
        // fresh so the stale present can never win.
        frame.fill(
            &Path::circle(Point::new((1.0 - cache_epsilon()) * 250.0, 0.0), 0.1),
            Color::from_rgba(0.0, 0.0, 0.0, 0.004),
        );

        let t = self.state.ai_morph_progress();
        if t <= 0.001 || !self.state.is_drawable() {
            return vec![frame.into_geometry()];
        }

        // Everything fades with the whole-menu tween too, so a
        // dismiss from chat mode fades the shell out exactly like
        // the disc would.
        let open_alpha = self.state.menu_open_alpha();
        let shell_alpha = cap_alpha(t) * open_alpha;
        if shell_alpha <= 0.001 && open_alpha <= 0.001 {
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
            Color::from_rgba(lerp(a.r, b.r, k), lerp(a.g, b.g, k), lerp(a.b, b.b, k), 1.0)
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
                b.rounded_rectangle(band_rect.position(), band_rect.size(), band_r.into());
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

        // Header furniture: only the drag pill stays canvas-drawn —
        // the title, status line, and action buttons are widgets in
        // `chat_ui::header` layered above (so they're clickable),
        // and the puck is drawn below as the travelling object.
        if chat_a > 0.001 {
            let top = rects.top;
            let handle_w = 44.0;
            let handle = Path::new(|b| {
                b.rounded_rectangle(
                    Point::new(top.x + (top.width - handle_w) / 2.0, top.y + 7.0),
                    iced::Size::new(handle_w, 4.0),
                    2.0.into(),
                );
            });
            frame.fill(&handle, scale_alpha(text_color, 0.35 * chat_a));
        }

        // Resize grip — bottom-right corner, visible once the chat
        // content is up. Accent while a drag is in flight, muted
        // otherwise.
        if chat_a > 0.001 {
            let overlay0 =
                self.palette_color(&palette.overlay0, Color::from_rgba(0.4, 0.42, 0.47, 1.0));
            let grip_color = if self.state.chat_size_pending_save.is_some() {
                scale_alpha(accent, chat_a)
            } else {
                scale_alpha(overlay0, 0.9 * chat_a)
            };
            let gx = bounds.width - 5.0;
            let gy = bounds.height - 5.0;
            // Two nested corner brackets, per the design's grip glyph.
            for inset in [0.0_f32, 4.5] {
                let arm = 10.0 - inset;
                let path = Path::new(|b| {
                    b.move_to(Point::new(gx - inset, gy - inset - arm));
                    b.line_to(Point::new(gx - inset, gy - inset));
                    b.line_to(Point::new(gx - inset - arm, gy - inset));
                });
                frame.stroke(
                    &path,
                    Stroke::default().with_color(grip_color).with_width(1.5),
                );
            }
        }

        // Live `W × H` mono badge while a native grip resize is in
        // flight (the compositor streams configure events; the badge
        // reads the real window size and fades once the stream goes
        // quiet and the size persists).
        if self.state.chat_size_pending_save.is_some() {
            let label = format!(
                "{} × {}",
                bounds.width.round() as u32,
                bounds.height.round() as u32
            );
            frame.fill_text(iced::widget::canvas::Text {
                content: label,
                position: Point::new(bounds.width - 96.0, bounds.height - 36.0),
                color: scale_alpha(accent, 1.0),
                size: 11.0.into(),
                font: iced::Font::MONOSPACE,
                ..iced::widget::canvas::Text::default()
            });
        }

        // The travelling page puck — the disc's centre circle
        // surviving the morph. Drawn last so it rides above the
        // caps; deliberately NOT multiplied by `cap_alpha`, only by
        // the whole-menu open fade: the puck must stay solid while
        // the disc fades out underneath it.
        let (puck_c, puck_r) = puck_geom(t, bounds.width, bounds.height);
        let ring = if self.state.ai_handoff.is_armed() {
            crate::render::slices::PuckRing::Armed
        } else {
            crate::render::slices::PuckRing::Dimmed
        };
        crate::render::slices::draw_puck(
            &mut frame,
            puck_c,
            puck_r,
            palette,
            open_alpha,
            ring,
            self.state.cycle_page_count(),
            self.state.cycle_page_position(),
        );

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
    fn wide_window_caps_park_at_edges_without_going_negative() {
        // 1200-wide chat window: the disc square sits top-left, so
        // the caps must sweep from the disc rim to EDGE_PAD without
        // x ever going negative mid-morph.
        let w = 1200.0;
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let r = cap_rects(t, w, H);
            assert!(r.top.x >= 0.0, "x negative at t={t}");
        }
        let parked = cap_rects(1.0, w, H);
        assert!((parked.top.x - EDGE_PAD).abs() < 0.01);
        assert!((parked.top.width - (w - 2.0 * EDGE_PAD)).abs() < 0.01);
        let start = cap_rects(0.0, w, H);
        assert!((start.top.x - (CENTER as f32 - MENU_RADIUS as f32)).abs() < 0.01);
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
    fn drag_covers_header_only() {
        let r = cap_rects(1.0, W, H);
        let left = Point::new(r.top.x + 12.0, r.top.y + r.top.height / 2.0);
        assert!(hit_drag(left, &r.top));
        let mid = Point::new(W / 2.0, H / 2.0);
        assert!(!hit_drag(mid, &r.top));
        assert!(!r.top.contains(mid) && !r.bottom.contains(mid));
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
