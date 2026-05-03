//! Visual radial-menu preview.
//!
//! Canvas widget that draws the radial menu the same way the
//! overlay does (8 slots clockwise from the top, slice colour,
//! optional icon glyph), and translates clicks + drags into
//! editor messages:
//!   - `Message::SelectSlice(usize)` — click on a slice
//!   - `Message::DismissSliceSelection`  — click in the centre
//!   - `Message::SwapSlices(from, to)`   — drag a slice onto
//!     another slot
//!
//! Drag-and-drop is handled inline by the Canvas program: mouse-
//! down on a slice records it as `dragging`, subsequent moves
//! update the cursor position used in `draw()` to render a ghost
//! disc at the cursor, and release fires `SwapSlices` for the
//! slot under the release point.

use crate::palette::Palette;
use iced::widget::canvas::{self, path::Builder, Frame, Geometry, Path, Stroke, Text};
use iced::{mouse, Color, Length, Point, Rectangle, Renderer, Theme};
use juhradial_shared::{ActionKind, Slice};

const N_SLICES: usize = 8;
const SLICE_DEG: f32 = 360.0 / N_SLICES as f32;

/// External-facing messages the parent app interprets.
#[derive(Debug, Clone)]
pub enum Action {
    SelectSlice(usize),
    DismissSelection,
    SwapSlices { from: usize, to: usize },
}

#[derive(Debug, Default)]
pub struct InteractionState {
    /// Slot the user pressed on (if any).
    drag_from: Option<usize>,
    /// Last cursor position, in widget-local coords. Used to render
    /// the ghost while dragging.
    cursor: Option<Point>,
    /// Did the cursor leave the press slot? Used to distinguish
    /// click-from-drag on release.
    moved_out: bool,
}

/// Painter for the radial preview. Holds owned colour + slice data
/// so it can be recreated cheaply per render without borrowing
/// from State.
pub struct RadialPreview {
    pub slices: Vec<Slice>,
    pub selected: Option<usize>,
    pub palette: Palette,
}

impl<Message: Clone> canvas::Program<Message> for RadialPreview
where
    Message: From<Action>,
{
    type State = InteractionState;

    fn update(
        &self,
        st: &mut InteractionState,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let pos = cursor.position_in(bounds);
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(p) = pos {
                    st.cursor = Some(p);
                    if let Some(from) = st.drag_from {
                        if !st.moved_out {
                            let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
                            if hit_slot(p, center, bounds) != Some(from) {
                                st.moved_out = true;
                            }
                        }
                    }
                    return Some(canvas::Action::request_redraw());
                }
                None
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = pos?;
                let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
                if let Some(slot) = hit_slot(p, center, bounds) {
                    st.drag_from = Some(slot);
                    st.cursor = Some(p);
                    st.moved_out = false;
                    return Some(canvas::Action::request_redraw());
                }
                None
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let p = pos?;
                let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
                let target = hit_slot(p, center, bounds);
                let from = st.drag_from.take();
                let moved = st.moved_out;
                st.moved_out = false;
                st.cursor = None;
                let act = match (from, target, moved) {
                    (Some(from), Some(to), true) if from != to => {
                        Some(Action::SwapSlices { from, to })
                    }
                    (Some(from), _, false) => Some(Action::SelectSlice(from)),
                    (Some(_), None, true) => None, // dragged off, no target — discard
                    _ => {
                        // Clicked the empty centre — dismiss
                        // selection.
                        if hit_slot(p, center, bounds).is_none() {
                            Some(Action::DismissSelection)
                        } else {
                            None
                        }
                    }
                };
                act.map(|a| canvas::Action::publish(Message::from(a)))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        st: &InteractionState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let pal = &self.palette;
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let outer_r = (bounds.width.min(bounds.height) / 2.0) - 8.0;
        let inner_r = outer_r * 0.32;
        let icon_r = outer_r * 0.66;

        // Faint shadow halo.
        let halo = Path::circle(center, outer_r + 4.0);
        frame.fill(&halo, Color::from_rgba(0.0, 0.0, 0.0, 0.25));

        for slot in 0..N_SLICES {
            draw_slot(
                &mut frame,
                center,
                inner_r,
                outer_r,
                icon_r,
                slot,
                self.slices.get(slot),
                pal,
                self.selected == Some(slot),
                st.drag_from == Some(slot),
            );
        }

        // Centre puck.
        let puck = Path::circle(center, inner_r);
        frame.fill(&puck, with_alpha(pal.surface0, 0.95));
        frame.stroke(
            &puck,
            Stroke::default()
                .with_color(with_alpha(pal.accent_dim, 0.7))
                .with_width(1.5),
        );

        // Drag ghost — render the dragged slice's chip at the
        // cursor so the user gets clear feedback.
        if let (Some(from), Some(cursor)) = (st.drag_from, st.cursor) {
            if st.moved_out {
                if let Some(slice) = self.slices.get(from) {
                    let (r, g, b) = slice_color(pal, slice);
                    let ghost = Path::circle(cursor, 22.0);
                    frame.fill(&ghost, Color::from_rgba(r, g, b, 0.9));
                    let ring = Path::circle(cursor, 23.0);
                    frame.stroke(
                        &ring,
                        Stroke::default()
                            .with_color(Color::WHITE)
                            .with_width(2.0),
                    );
                    frame.fill_text(Text {
                        content: short_label(&slice.label),
                        position: Point::new(cursor.x - 12.0, cursor.y - 6.0),
                        color: Color::BLACK,
                        size: 11.0.into(),
                        ..Text::default()
                    });
                }
            }
        }

        vec![frame.into_geometry()]
    }
}

fn draw_slot(
    frame: &mut Frame,
    center: Point,
    inner_r: f32,
    outer_r: f32,
    icon_r: f32,
    slot: usize,
    slice: Option<&Slice>,
    pal: &Palette,
    selected: bool,
    being_dragged: bool,
) {
    let start_deg = slot as f32 * SLICE_DEG - SLICE_DEG / 2.0 - 90.0;
    let end_deg = start_deg + SLICE_DEG;
    let wedge = wedge_path(center, inner_r, outer_r, start_deg, end_deg);

    // Base wedge fill — surface0 with low alpha, brighter when
    // selected. Dragged slot fades out (its content is shown as
    // a ghost at the cursor).
    let base_alpha = if being_dragged {
        0.10
    } else if selected {
        0.36
    } else {
        0.20
    };
    frame.fill(&wedge, with_alpha(pal.surface0, base_alpha));

    let stroke_color = if selected {
        pal.accent
    } else {
        with_alpha(pal.surface2, 0.7)
    };
    frame.stroke(
        &wedge,
        Stroke::default()
            .with_color(stroke_color)
            .with_width(if selected { 2.0 } else { 1.0 }),
    );

    // Icon disc on the bisector.
    let icon_angle = (slot as f32 * SLICE_DEG - 90.0).to_radians();
    let icon_pos = Point::new(
        center.x + icon_r * icon_angle.cos(),
        center.y + icon_r * icon_angle.sin(),
    );
    let bg_radius = (outer_r - inner_r) * 0.32;
    if !being_dragged {
        let bg = Path::circle(icon_pos, bg_radius);
        if let Some(s) = slice {
            let (r, g, b) = slice_color(pal, s);
            frame.fill(&bg, Color::from_rgba(r, g, b, 0.85));
            frame.stroke(
                &bg,
                Stroke::default()
                    .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.35))
                    .with_width(1.0),
            );
            // Pick a recognisable glyph: prefer one mapped from the
            // slice's freedesktop icon name, fall back to the
            // action kind. Real SVG/PNG icon rendering is a
            // separate follow-up (would need an XDG resolver
            // similar to overlay-rs/src/render/icons.rs).
            let glyph = glyph_for_slice(s);
            // Wider chars (emoji) need a smaller font so they don't
            // spill outside the disc; ASCII glyphs can go bigger.
            let (size, x_off, y_off) = if glyph.chars().count() > 1 {
                (12.0, 8.0, 7.0)
            } else if glyph.chars().any(|c| (c as u32) > 0x2000) {
                (14.0, 7.0, 8.0)
            } else {
                (16.0, 5.0, 8.0)
            };
            frame.fill_text(Text {
                content: glyph.to_string(),
                position: Point::new(icon_pos.x - x_off, icon_pos.y - y_off),
                color: Color::WHITE,
                size: size.into(),
                ..Text::default()
            });
        } else {
            frame.fill(&bg, with_alpha(pal.surface1, 0.7));
        }
    }

    // Slot label — draw the slice's label arched outside the
    // ring. iced's canvas text doesn't do arc layout; we just
    // anchor straight at a polar offset.
    if let Some(s) = slice {
        let label_r = outer_r + 12.0;
        let label_pos = Point::new(
            center.x + label_r * icon_angle.cos() - 28.0,
            center.y + label_r * icon_angle.sin() - 6.0,
        );
        frame.fill_text(Text {
            content: short_label(&s.label),
            position: label_pos,
            color: if selected { pal.text } else { pal.subtext0 },
            size: 10.0.into(),
            ..Text::default()
        });
    }
}

fn wedge_path(
    center: Point,
    inner_r: f32,
    outer_r: f32,
    start_deg: f32,
    end_deg: f32,
) -> Path {
    let start = start_deg.to_radians();
    let end = end_deg.to_radians();
    let mut b = Builder::new();
    let inner_start = polar(center, inner_r, start);
    let outer_start = polar(center, outer_r, start);
    let inner_end = polar(center, inner_r, end);
    b.move_to(inner_start);
    b.line_to(outer_start);
    b.arc(canvas::path::Arc {
        center,
        radius: outer_r,
        start_angle: iced::Radians(start),
        end_angle: iced::Radians(end),
    });
    b.line_to(inner_end);
    b.arc(canvas::path::Arc {
        center,
        radius: inner_r,
        start_angle: iced::Radians(end),
        end_angle: iced::Radians(start),
    });
    b.close();
    b.build()
}

fn polar(center: Point, r: f32, angle_rad: f32) -> Point {
    Point::new(
        center.x + r * angle_rad.cos(),
        center.y + r * angle_rad.sin(),
    )
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// Resolve a slice's colour key (e.g. "green", "sapphire") to the
/// palette colour. Falls back to the accent so something always
/// renders.
fn slice_color(pal: &Palette, slice: &Slice) -> (f32, f32, f32) {
    let key = slice.color.as_str();
    let c = match key {
        "green" => pal.green,
        "yellow" => pal.yellow,
        "red" => pal.red,
        "blue" => pal.blue,
        "mauve" => pal.mauve,
        "pink" => pal.pink,
        "peach" => pal.peach,
        "teal" => pal.teal,
        "sapphire" => pal.sapphire,
        "lavender" => pal.lavender,
        _ => pal.accent,
    };
    (c.r, c.g, c.b)
}

/// Pick a recognisable glyph for a slice. Prefers a mapping of the
/// freedesktop icon name (so "media-playback-start-symbolic"
/// renders as ▶, "folder-symbolic" as 🗀, etc.). Falls back to the
/// action kind when the icon is missing or unmapped, then to a
/// dot. Real PNG/SVG icon rendering is a separate follow-up.
fn glyph_for_slice(s: &Slice) -> &'static str {
    let icon = s.icon.as_str();
    if let Some(g) = ICON_GLYPH_MAP
        .iter()
        .find(|(name, _)| {
            icon == *name
                // Treat the trailing "-symbolic" as optional so the
                // map covers both "folder" and "folder-symbolic".
                || icon == name.trim_end_matches("-symbolic")
        })
        .map(|(_, glyph)| *glyph)
    {
        return g;
    }
    match s.kind {
        ActionKind::Submenu => "▸",
        ActionKind::Macro => "M",
        ActionKind::EasySwitch => "⇄",
        ActionKind::Settings => "⚙",
        ActionKind::Emoji => "☻",
        ActionKind::Shortcut => "⌘",
        ActionKind::Exec => "▶",
        ActionKind::None => "•",
    }
}

/// Mapping table — freedesktop icon name → unicode glyph. Add
/// entries as users hit defaults that look generic. Anything
/// not listed falls back to the kind glyph.
const ICON_GLYPH_MAP: &[(&str, &str)] = &[
    // Media
    ("media-playback-start-symbolic", "▶"),
    ("media-playback-pause-symbolic", "❚❚"),
    ("media-playback-stop-symbolic", "■"),
    ("media-skip-forward-symbolic", "⏭"),
    ("media-skip-backward-symbolic", "⏮"),
    // Audio
    ("audio-volume-high-symbolic", "🔊"),
    ("audio-volume-medium-symbolic", "🔉"),
    ("audio-volume-low-symbolic", "🔈"),
    ("audio-volume-muted-symbolic", "🔇"),
    ("audio-speakers-symbolic", "🔉"),
    // Documents / files
    ("document-new-symbolic", "📝"),
    ("document-open-symbolic", "📂"),
    ("document-save-symbolic", "💾"),
    ("folder-symbolic", "🗀"),
    ("system-file-manager-symbolic", "🗀"),
    // System / settings
    ("emblem-system-symbolic", "⚙"),
    ("preferences-system-symbolic", "⚙"),
    ("system-lock-screen-symbolic", "🔒"),
    ("computer-symbolic", "🖥"),
    // Comms / smileys
    ("face-smile-symbolic", "☻"),
    ("dialog-information-symbolic", "ⓘ"),
    // Apps
    ("utilities-terminal-symbolic", ">_"),
    ("camera-photo-symbolic", "📷"),
    ("applications-science-symbolic", "🧪"),
    ("applications-development-symbolic", "🔧"),
    ("applications-graphics-symbolic", "🖼"),
    ("input-mouse-symbolic", "🖱"),
    ("input-gaming-symbolic", "🎮"),
    ("input-keyboard-symbolic", "⌨"),
    // Editing
    ("edit-copy-symbolic", "⎘"),
    ("edit-paste-symbolic", "⎗"),
    ("edit-undo-symbolic", "↶"),
    ("edit-redo-symbolic", "↷"),
    ("edit-cut-symbolic", "✂"),
    ("edit-select-all-symbolic", "≡"),
    // Window
    ("window-close-symbolic", "✕"),
    ("window-minimize-symbolic", "—"),
    ("window-maximize-symbolic", "□"),
    // Network / sharing
    ("network-wireless-symbolic", "📶"),
    ("view-grid-symbolic", "▦"),
    ("view-list-symbolic", "≡"),
    ("view-dual-symbolic", "▤"),
];

fn short_label(s: &str) -> String {
    if s.chars().count() > 10 {
        let mut out: String = s.chars().take(9).collect();
        out.push('…');
        out
    } else {
        s.to_string()
    }
}

/// Pixel hit-test: which slot (0..7), if any, is under `p`?
fn hit_slot(p: Point, center: Point, bounds: Rectangle) -> Option<usize> {
    let dx = p.x - center.x;
    let dy = p.y - center.y;
    let dist = (dx * dx + dy * dy).sqrt();
    let outer_r = (bounds.width.min(bounds.height) / 2.0) - 8.0;
    let inner_r = outer_r * 0.32;
    if dist < inner_r || dist > outer_r {
        return None;
    }
    let mut angle = dx.atan2(-dy).to_degrees();
    if angle < 0.0 {
        angle += 360.0;
    }
    Some(((angle + SLICE_DEG / 2.0) / SLICE_DEG) as usize % N_SLICES)
}

/// Convenience: ready-to-use Canvas widget with a sensible default
/// size for the Buttons tab right column.
pub fn radial_preview_widget<'a, Message>(
    palette: &Palette,
    slices: &[Slice],
    selected: Option<usize>,
    size_px: f32,
) -> iced::Element<'a, Message>
where
    Message: 'a + Clone + From<Action>,
{
    let painter = RadialPreview {
        palette: palette.clone(),
        slices: slices.to_vec(),
        selected,
    };
    iced::widget::canvas(painter)
        .width(Length::Fixed(size_px))
        .height(Length::Fixed(size_px))
        .into()
}
