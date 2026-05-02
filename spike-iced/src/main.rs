//! Iced canvas spike for the radial menu.
//!
//! Loads the user's real config via juhradial-shared, then renders a
//! radial of 8 wedges using iced's Canvas widget. Each wedge:
//!
//!   * filled donut arc using `Path` builder (`move_to`, `line_to`,
//!     `arc`, `close`),
//!   * stroked outline,
//!   * coloured icon-background disc on the slice's bisector.
//!
//! When this paints correctly inside the distrobox, the iced pivot
//! is validated and overlay-rs gets the matching refactor.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::window;
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use juhradial_shared::theme::{parse_hex_rgba, BUNDLED_THEME_JSON};
use juhradial_shared::{config, AppConfig, RadialMenuConfig, Theme as PaletteTheme};

const WINDOW_PX: f32 = 484.0;
const MENU_RADIUS: f32 = 150.0;
const CENTER_RADIUS: f32 = 45.0;
const ICON_RADIUS: f32 = 100.0;
const SLICE_DEGREES: f32 = 45.0;
const RING_OUTER_INSET: f32 = 6.0;
const RING_INNER_INSET: f32 = 6.0;
const ICON_BG_RADIUS: f32 = 26.0;

#[derive(Debug, Clone, Copy)]
enum Message {
    NextSlice,
}

#[derive(Debug)]
struct Spike {
    palette: PaletteTheme,
    menu: RadialMenuConfig,
    highlighted: usize,
}

impl Default for Spike {
    fn default() -> Self {
        let cfg = config::default_config_path()
            .and_then(|p| AppConfig::load_from(&p).ok())
            .unwrap_or_default();
        let palette = PaletteTheme::load(&cfg.theme).unwrap_or_else(|| {
            let (_, json) = BUNDLED_THEME_JSON[0];
            serde_json::from_str(json).expect("bundled theme parses")
        });
        Spike {
            palette,
            menu: cfg.radial_menu,
            highlighted: 0,
        }
    }
}

fn update(spike: &mut Spike, msg: Message) {
    match msg {
        Message::NextSlice => spike.highlighted = (spike.highlighted + 1) % 8,
    }
}

fn view(spike: &Spike) -> Element<'_, Message> {
    let painter = Painter {
        palette: spike.palette.clone(),
        menu: spike.menu.clone(),
        highlighted: spike.highlighted,
    };
    Canvas::new(painter)
        .width(Length::Fixed(WINDOW_PX))
        .height(Length::Fixed(WINDOW_PX))
        .into()
}

#[derive(Debug)]
struct Painter {
    palette: PaletteTheme,
    menu: RadialMenuConfig,
    highlighted: usize,
}

impl<Message> canvas::Program<Message> for Painter {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let cx = bounds.size().width / 2.0;
        let cy = bounds.size().height / 2.0;
        let center = Point::new(cx, cy);

        // Faint shadow halo so the menu reads as a disc against the
        // (possibly transparent) compositor background.
        let halo = Path::circle(center, MENU_RADIUS + 6.0);
        frame.fill(&halo, Color::from_rgba(0.0, 0.0, 0.0, 0.35));

        let palette = &self.palette.colors;
        let outer_r = MENU_RADIUS - RING_OUTER_INSET;
        let inner_r = CENTER_RADIUS + RING_INNER_INSET;

        for i in 0..8usize {
            let highlight = if i == self.highlighted { 1.0_f32 } else { 0.0 };
            let start_deg = (i as f32) * SLICE_DEGREES - SLICE_DEGREES / 2.0 - 90.0;
            let end_deg = start_deg + SLICE_DEGREES;
            let start_rad = start_deg.to_radians();
            let end_rad = end_deg.to_radians();

            let wedge = Path::new(|p| {
                let inner_start = polar(center, inner_r, start_rad);
                let outer_start = polar(center, outer_r, start_rad);
                let inner_end = polar(center, inner_r, end_rad);
                p.move_to(inner_start);
                p.line_to(outer_start);
                p.arc(canvas::path::Arc {
                    center,
                    radius: outer_r,
                    start_angle: iced::Radians(start_rad),
                    end_angle: iced::Radians(end_rad),
                });
                p.line_to(inner_end);
                p.arc(canvas::path::Arc {
                    center,
                    radius: inner_r,
                    start_angle: iced::Radians(end_rad),
                    end_angle: iced::Radians(start_rad),
                });
                p.close();
            });

            // Base fill — surface0 @ low alpha.
            frame.fill(&wedge, rgba_iced(&palette.surface0, 80.0 / 255.0));

            // Border — interpolate surface2 → white on hover.
            let stroke_color =
                lerp_color(rgba_iced(&palette.surface2, 1.0), Color::WHITE, highlight);
            let alpha = (60.0 + 60.0 * highlight) / 255.0;
            frame.stroke(
                &wedge,
                Stroke::default()
                    .with_color(Color { a: alpha, ..stroke_color })
                    .with_width(1.0 + 0.5 * highlight),
            );

            if highlight > 0.0 {
                frame.fill(
                    &wedge,
                    Color::from_rgba(1.0, 1.0, 1.0, 45.0 / 255.0 * highlight),
                );
            }

            // Icon position + background disc.
            let icon_angle = ((i as f32) * SLICE_DEGREES - 90.0).to_radians();
            let icon_pos = polar(center, ICON_RADIUS, icon_angle);

            if highlight > 0.0 {
                let glow = Path::circle(icon_pos, ICON_BG_RADIUS + 2.0);
                frame.stroke(
                    &glow,
                    Stroke::default()
                        .with_color(Color::from_rgba(
                            1.0, 1.0, 1.0, 40.0 / 255.0 * highlight,
                        ))
                        .with_width(3.0),
                );
            }

            let s1 = rgba_iced(&palette.surface1, 1.0);
            let s2 = rgba_iced(&palette.surface2, 1.0);
            let bg = lerp_color(s1, s2, highlight);
            let bg_alpha = (230.0 + 25.0 * highlight) / 255.0;
            frame.fill(
                &Path::circle(icon_pos, ICON_BG_RADIUS),
                Color { a: bg_alpha, ..bg },
            );

            // Slice colour placeholder dot.
            let slot_color = self
                .menu
                .slices
                .get(i)
                .map(|s| s.color.as_str())
                .unwrap_or("accent");
            let (sr, sg, sb, _) = palette.slice_color_rgba(slot_color);
            let dot_color = Color::from_rgb(sr as f32, sg as f32, sb as f32);
            frame.fill(&Path::circle(icon_pos, ICON_BG_RADIUS * 0.35), dot_color);
        }

        // Centre puck.
        let puck = Path::circle(center, CENTER_RADIUS);
        frame.fill(&puck, rgba_iced(&palette.surface0, 220.0 / 255.0));
        frame.stroke(
            &puck,
            Stroke::default()
                .with_color(rgba_iced(&palette.accent_dim, 140.0 / 255.0))
                .with_width(2.0),
        );

        vec![frame.into_geometry()]
    }
}

fn polar(center: Point, radius: f32, angle_rad: f32) -> Point {
    Point::new(
        center.x + radius * angle_rad.cos(),
        center.y + radius * angle_rad.sin(),
    )
}

fn rgba_iced(hex: &str, override_alpha: f32) -> Color {
    let (r, g, b, _a) = parse_hex_rgba(hex).unwrap_or((1.0, 1.0, 1.0, 1.0));
    Color::from_rgba(r as f32, g as f32, b as f32, override_alpha)
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn main() -> iced::Result {
    iced::application(Spike::default, update, view)
        .title("JuhRadial — iced spike")
        .window_size(Size::new(WINDOW_PX, WINDOW_PX))
        // Decorations off + transparent surface + transparent root
        // style so we get just the radial wheel on screen, no
        // titlebar, no chrome, no theme background painted behind
        // the canvas. .transparent() alone opens a transparent
        // *Wayland surface* but iced's default theme still paints
        // an opaque background colour over it; the .style() closure
        // overrides that with Color::TRANSPARENT so only what the
        // canvas draws is visible. Always-on-top so the menu floats.
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .level(window::Level::AlwaysOnTop)
        .style(|_state, _theme| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: Color::WHITE,
        })
        .run()
}
