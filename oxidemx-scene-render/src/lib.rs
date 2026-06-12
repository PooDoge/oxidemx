//! Replay a plugin widget's retained [`Scene`] onto an iced canvas
//! [`Frame`]. The scene was decoded + validated on the host worker
//! thread; this walk is pure canvas drawing — no wasm, no allocation
//! beyond iced paths (spec §8 frame-path rule).
//!
//! Lifted verbatim from `overlay-rs/src/render/slices/widgets.rs`
//! (Plan 3 Task 5) so the settings app's options-card preview and the
//! overlay's ring replay are the same code path: same colour
//! resolution, same text heuristics, same Group transforms.

use iced::widget::canvas::{Frame, Path, Stroke};
use iced::{Color, Point, Size};
use oxidemx_shared::theme::{parse_hex_rgba, ThemeColors};
use oxidemx_widget_proto::{Prim, Scene, TextAlign, TextWeight};

/// Replay a Scene anchored at `anchor` (the wedge's icon point).
///
/// Scene coordinates are wedge-local: origin at the icon anchor,
/// +y down (`oxidemx_widget_proto::scene` docs), so the whole walk
/// runs inside one translated `with_save` scope. `Color::Palette`
/// keys resolve through the active theme; `"accent"` and unknown
/// keys resolve to the slice's configured colour — same fallback
/// chain the overlay's `draw_slice` uses for icon tinting.
///
/// Hover and scaling are intentionally NOT applied here:
///   - Hover state is delivered to the widget guest as
///     `WedgeGeom::hovered` on each event, letting the widget choose
///     its own hover response in the Scene it returns.
///   - Per-slot scale transforms are the caller's business (the
///     overlay wraps the whole draw_slice call in its composed
///     transform; the settings preview draws unscaled).
pub fn draw_custom_widget(
    frame: &mut Frame,
    scene: &Scene,
    anchor: Point,
    palette: &ThemeColors,
    slice_color: Color,
    mo: f32,
) {
    frame.with_save(|f| {
        f.translate(iced::Vector::new(anchor.x, anchor.y));
        draw_prims(f, &scene.prims, palette, slice_color, mo);
    });
}

/// Recursive prim walk (groups translate/scale their children).
fn draw_prims(frame: &mut Frame, prims: &[Prim], palette: &ThemeColors, slice_color: Color, mo: f32) {
    for prim in prims {
        match prim {
            Prim::Path { ops, stroke, fill } => {
                use oxidemx_widget_proto::PathOp;
                let path = Path::new(|b| {
                    for op in ops {
                        match op {
                            PathOp::MoveTo(x, y) => b.move_to(Point::new(*x, *y)),
                            PathOp::LineTo(x, y) => b.line_to(Point::new(*x, *y)),
                            PathOp::QuadTo(cx, cy, x, y) => {
                                b.quadratic_curve_to(Point::new(*cx, *cy), Point::new(*x, *y))
                            }
                            PathOp::CubicTo(c1x, c1y, c2x, c2y, x, y) => b.bezier_curve_to(
                                Point::new(*c1x, *c1y),
                                Point::new(*c2x, *c2y),
                                Point::new(*x, *y),
                            ),
                            PathOp::Close => b.close(),
                        }
                    }
                });
                paint_path(frame, &path, stroke.as_ref(), fill.as_ref(), palette, slice_color, mo);
            }
            Prim::Arc { cx, cy, radius, start_angle, end_angle, stroke, fill } => {
                let path = Path::new(|b| {
                    b.arc(iced::widget::canvas::path::Arc {
                        center: Point::new(*cx, *cy),
                        radius: *radius,
                        start_angle: iced::Radians(*start_angle),
                        end_angle: iced::Radians(*end_angle),
                    })
                });
                paint_path(frame, &path, stroke.as_ref(), fill.as_ref(), palette, slice_color, mo);
            }
            Prim::Text { x, y, content, size, color, weight, align } => {
                // 0.55 em/char width heuristic + vertical centring —
                // the canvas API has no measure pass; matches the
                // overlay's `draw_centered_text`.
                let approx_w = content.chars().count() as f32 * size * 0.55;
                let px = match align {
                    TextAlign::Left => *x,
                    TextAlign::Center => x - approx_w / 2.0,
                    TextAlign::Right => x - approx_w,
                };
                frame.fill_text(iced::widget::canvas::Text {
                    content: content.clone(),
                    position: Point::new(px, y - size / 2.0),
                    color: resolve_scene_color(color, palette, slice_color, mo),
                    size: (*size).into(),
                    font: iced::Font {
                        weight: match weight {
                            TextWeight::Regular => iced::font::Weight::Normal,
                            TextWeight::Medium => iced::font::Weight::Medium,
                            TextWeight::Semibold => iced::font::Weight::Semibold,
                            TextWeight::Bold => iced::font::Weight::Bold,
                        },
                        ..Default::default()
                    },
                    ..iced::widget::canvas::Text::default()
                });
            }
            Prim::Sparkline { x, y, w, h, points, color } => {
                // Normalised 0..=1 samples drawn like the overlay's
                // built-in CPU sparkline: polyline across `w`, peak
                // at the top of the `h` band.
                if points.len() >= 2 {
                    let step = w / (points.len() - 1) as f32;
                    let path = Path::new(|b| {
                        for (j, v) in points.iter().enumerate() {
                            let px = x + j as f32 * step;
                            let py = y + h - v.clamp(0.0, 1.0) * h;
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
                            .with_color(resolve_scene_color(color, palette, slice_color, mo))
                            .with_width(1.5),
                    );
                }
            }
            Prim::Image { x, y, w, h, asset } => {
                // Canvas-side image replay needs a decode + handle
                // cache that doesn't exist yet — Plan 3 follow-up.
                // Draw a quiet placeholder so the layout reads, and
                // say so once per session, not per frame.
                static IMAGE_ONCE: std::sync::Once = std::sync::Once::new();
                IMAGE_ONCE.call_once(|| {
                    tracing::warn!(
                        asset,
                        "Prim::Image replay is not implemented yet — rendering a \
                         placeholder rect (Plan 3 follow-up)"
                    );
                });
                let rect = Path::new(|b| {
                    b.rounded_rectangle(Point::new(*x, *y), Size::new(*w, *h), 3.0.into());
                });
                frame.fill(&rect, Color { a: 0.25 * mo, ..slice_color });
                frame.stroke(
                    &rect,
                    Stroke::default()
                        .with_color(Color { a: 0.6 * mo, ..slice_color })
                        .with_width(1.0),
                );
            }
            Prim::Group { dx, dy, scale, children } => {
                frame.with_save(|f| {
                    f.translate(iced::Vector::new(*dx, *dy));
                    if (*scale - 1.0).abs() > 1e-4 {
                        f.scale(*scale);
                    }
                    draw_prims(f, children, palette, slice_color, mo);
                });
            }
            // `Prim` is #[non_exhaustive]: a newer proto revision can
            // add variants this renderer doesn't know. Skip them, say
            // so once per session.
            other => {
                static UNKNOWN_ONCE: std::sync::Once = std::sync::Once::new();
                UNKNOWN_ONCE.call_once(|| {
                    tracing::warn!(
                        ?other,
                        "unknown scene primitive — skipped (update OxideMX to render it)"
                    );
                });
            }
        }
    }
}

/// Shared fill/stroke application for Path + Arc prims.
fn paint_path(
    frame: &mut Frame,
    path: &Path,
    stroke: Option<&oxidemx_widget_proto::Stroke>,
    fill: Option<&oxidemx_widget_proto::Color>,
    palette: &ThemeColors,
    slice_color: Color,
    mo: f32,
) {
    if let Some(c) = fill {
        frame.fill(path, resolve_scene_color(c, palette, slice_color, mo));
    }
    if let Some(s) = stroke {
        frame.stroke(
            path,
            Stroke::default()
                .with_color(resolve_scene_color(&s.color, palette, slice_color, mo))
                .with_width(s.width),
        );
    }
}

/// `oxidemx_widget_proto::Color` → iced colour, modulated by the
/// menu opacity. `Palette("accent")` and unknown palette keys map
/// to the slice's configured colour — the same resolution chain the
/// overlay runs for icon tints, so a default `tile()` scene matches
/// the built-in wedge styling.
pub fn resolve_scene_color(
    color: &oxidemx_widget_proto::Color,
    palette: &ThemeColors,
    slice_color: Color,
    mo: f32,
) -> Color {
    match color {
        oxidemx_widget_proto::Color::Rgba(r, g, b, a) => Color::from_rgba(
            *r as f32 / 255.0,
            *g as f32 / 255.0,
            *b as f32 / 255.0,
            (*a as f32 / 255.0) * mo,
        ),
        oxidemx_widget_proto::Color::Palette(key) => {
            if key == "accent" {
                return Color { a: mo, ..slice_color };
            }
            match palette.lookup(key).and_then(parse_hex_rgba) {
                Some((r, g, b, a)) => {
                    Color::from_rgba(r as f32, g as f32, b as f32, a as f32 * mo)
                }
                None => Color { a: mo, ..slice_color },
            }
        }
    }
}
