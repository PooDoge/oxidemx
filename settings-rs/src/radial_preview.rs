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
use iced::widget::canvas::{self, path::Builder, Frame, Geometry, Image, Path, Stroke, Text};
use iced::widget::image::Handle;
use iced::{mouse, Color, Length, Point, Rectangle, Renderer, Theme};
use juhradial_icons::{IconCache, RasterIcon};
use juhradial_shared::{ActionKind, Slice};
use std::cell::RefCell;
use std::collections::HashMap;

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
/// from State. The icon cache is wrapped in `Rc<RefCell<…>>` so
/// repeated draws share their resolved + tinted icons (the iced
/// canvas program is `&self`, so we need interior mutability).
pub struct RadialPreview {
    pub slices: Vec<Slice>,
    pub selected: Option<usize>,
    pub palette: Palette,
    pub icons: std::rc::Rc<IconCache>,
    pub iced_handles: std::rc::Rc<RefCell<HashMap<IconKey, Handle>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IconKey {
    source: String,
    size: u32,
    color: u32,
}

impl RadialPreview {
    /// Resolve the slice's icon to an iced Handle, going through
    /// the shared rasterised cache and a per-process iced handle
    /// cache to avoid re-uploading pixels per frame.
    fn resolve_icon(&self, source: &str, size_px: u32, tint: Color) -> Option<Handle> {
        if source.is_empty() || size_px == 0 {
            return None;
        }
        let color = pack_color((tint.r, tint.g, tint.b, tint.a));
        let key = IconKey {
            source: source.to_string(),
            size: size_px,
            color,
        };
        if let Some(h) = self.iced_handles.borrow().get(&key) {
            return Some(h.clone());
        }
        let icon: RasterIcon =
            self.icons.resolve(source, size_px, (tint.r, tint.g, tint.b, tint.a))?;
        let handle = Handle::from_rgba(icon.size, icon.size, icon.rgba);
        self.iced_handles
            .borrow_mut()
            .insert(key, handle.clone());
        Some(handle)
    }
}

fn pack_color((r, g, b, a): (f32, f32, f32, f32)) -> u32 {
    let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u32;
    (to_u8(a) << 24) | (to_u8(r) << 16) | (to_u8(g) << 8) | to_u8(b)
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
                self,
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

        // Centre label — show the selected slice's name (or the
        // currently-dragged slice's name as a stronger feedback
        // hint). Mirrors the legacy overlay's centre text. Truncate
        // to ~14 chars to stop long labels from spilling outside
        // the puck.
        let center_text = st
            .drag_from
            .or(self.selected)
            .and_then(|i| self.slices.get(i))
            .map(|s| short_label(&s.label));
        if let Some(label) = center_text {
            let approx_w = label.chars().count() as f32 * 6.5;
            frame.fill_text(Text {
                content: label,
                position: Point::new(center.x - approx_w / 2.0, center.y - 8.0),
                color: pal.text,
                size: 13.0.into(),
                ..Text::default()
            });
        }

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
    painter: &RadialPreview,
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
    // Icon disc — 36 % of the wedge thickness fits inside the slot
    // without crowding adjacent slices.
    let bg_radius = (outer_r - inner_r) * 0.36;
    if !being_dragged {
        let bg = Path::circle(icon_pos, bg_radius);
        if let Some(s) = slice {
            let (r, g, b) = slice_color(pal, s);
            frame.fill(&bg, Color::from_rgba(r, g, b, 0.92));
            frame.stroke(
                &bg,
                Stroke::default()
                    .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.35))
                    .with_width(1.0),
            );
            // Try to render the slice's actual freedesktop icon —
            // tinted white so it pops against the slice colour.
            // Falls back to a unicode glyph if the resolver can't
            // find an SVG/PNG (icon name unknown / no theme).
            // Sized to ~85 % of the disc diameter so there's a
            // visible coloured ring around the glyph.
            let glyph_size = bg_radius * 0.95;
            let icon_size_px = glyph_size.round().max(8.0) as u32;
            if let Some(handle) = painter.resolve_icon(s.icon.as_str(), icon_size_px, Color::WHITE)
            {
                let bounds = Rectangle::new(
                    Point::new(icon_pos.x - glyph_size / 2.0, icon_pos.y - glyph_size / 2.0),
                    iced::Size::new(glyph_size, glyph_size),
                );
                frame.draw_image(bounds, Image::new(handle));
            } else {
                // Fallback unicode glyph — bigger than before so
                // it actually reads at the new disc size.
                let glyph = glyph_for_slice(s);
                let size = bg_radius * 0.95;
                let approx_w = glyph.chars().count() as f32 * size * 0.55;
                frame.fill_text(Text {
                    content: glyph.to_string(),
                    position: Point::new(
                        icon_pos.x - approx_w / 2.0,
                        icon_pos.y - size / 2.0,
                    ),
                    color: Color::WHITE,
                    size: size.into(),
                    ..Text::default()
                });
            }
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

/// Convenience: ready-to-use Canvas widget. The icon + handle
/// caches are passed in by the parent (typically owned by `State`)
/// so they survive across re-renders — recreating the cache every
/// frame would force every icon back through resvg/tiny-skia.
pub fn radial_preview_widget<'a, Message>(
    palette: &Palette,
    slices: &[Slice],
    selected: Option<usize>,
    icons: std::rc::Rc<IconCache>,
    iced_handles: std::rc::Rc<RefCell<HashMap<IconKey, Handle>>>,
    size_px: f32,
) -> iced::Element<'a, Message>
where
    Message: 'a + Clone + From<Action>,
{
    let painter = RadialPreview {
        palette: palette.clone(),
        slices: slices.to_vec(),
        selected,
        icons,
        iced_handles,
    };
    iced::widget::canvas(painter)
        .width(Length::Fixed(size_px))
        .height(Length::Fixed(size_px))
        .into()
}
