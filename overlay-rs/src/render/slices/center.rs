//! Centre puck rendering: `draw_center`, page-name flash,
//! page-indicator dots, and the travelling chat-shell puck.

use iced::widget::canvas::{Frame, Path, Stroke};
use iced::{Point, Radians, Vector};
use oxidemx_shared::theme::{parse_hex_rgba, ThemeColors};

use super::{polar, rgba};

/// Draw the page-name flash inside the centre puck — handles the
/// slide-in / cross-slide-out / fade-out sequence in one place.
/// Caller passes the current and (optional) previous page name,
/// the slide direction (+1 forward / -1 backward / 0 no-slide),
/// timing knobs, and the elapsed-since-trigger clock.
///
/// Timeline:
///   * `0..transition_ms` — incoming slides + fades in from
///     `direction × slide_distance` to 0; outgoing (if any)
///     slides + fades out from 0 to `−direction × slide_distance`.
///   * `transition_ms..(transition_ms + visible_ms)` — incoming
///     at full opacity, no slide.
///   * `(transition_ms + visible_ms)..total` — incoming fades
///     out to 0 over the same `transition_ms` window.
///
/// `total = 2 × transition_ms + visible_ms`. Returns `false` when
/// `elapsed >= total` so the caller can clear its timer.
#[allow(clippy::too_many_arguments)]
pub fn draw_page_name_transition(
    frame: &mut Frame,
    center: Point,
    radius: f32,
    palette: &ThemeColors,
    menu_opacity: f32,
    current_name: &str,
    previous_name: Option<&str>,
    direction: i32,
    elapsed_ms: u64,
    visible_ms: u32,
    transition_ms: u32,
    slide_distance_px: f32,
    label_size: f32,
    label_font: iced::Font,
    arced: bool,
) -> bool {
    let mo = menu_opacity.clamp(0.0, 1.0);
    let trans = transition_ms.max(1) as f32;
    let visible = visible_ms as f32;
    let total = trans + visible + trans;

    if (elapsed_ms as f32) >= total {
        return false;
    }
    let t = elapsed_ms as f32;

    // Incoming alpha + x_offset.
    // Phase 1 (0..trans): alpha ramps 0 → 1, x_offset ramps
    //   `direction * slide` → 0 (eased ease-out).
    // Phase 2 (trans..trans+visible): alpha 1, offset 0.
    // Phase 3 (trans+visible..total): alpha 1 → 0, offset 0.
    let (in_alpha, in_x) = if t < trans {
        let p = t / trans;
        let eased = 1.0 - (1.0 - p).powi(3); // ease-out cubic
        (eased, direction as f32 * slide_distance_px * (1.0 - eased))
    } else if t < trans + visible {
        (1.0, 0.0)
    } else {
        let p = (t - trans - visible) / trans;
        let eased = 1.0 - (1.0 - p).powi(3);
        (1.0 - eased, 0.0)
    };

    // Outgoing alpha + x_offset (only during phase 1).
    let outgoing = if t < trans && previous_name.is_some() && direction != 0 {
        let p = t / trans;
        let eased = 1.0 - (1.0 - p).powi(3);
        let alpha = 1.0 - eased;
        let x = -(direction as f32) * slide_distance_px * eased;
        Some((alpha, x))
    } else {
        None
    };

    let (tr, tg, tb, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));

    // Approximate visible-character width — same constant the
    // hover label uses (`label_size * 0.55`).
    let approx_w = |s: &str| s.chars().count() as f32 * label_size * 0.55;
    // Aggressive truncate so long page names fit the puck.
    let truncate = |s: &str| -> String {
        let max_chars = ((radius * 2.0 / (label_size * 0.55)).max(4.0)) as usize;
        if s.chars().count() > max_chars {
            let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
            out.push('…');
            out
        } else {
            s.to_string()
        }
    };

    let txt_color = |alpha: f32| {
        iced::Color::from_rgba(
            tr as f32,
            tg as f32,
            tb as f32,
            (mo * alpha).clamp(0.0, 1.0),
        )
    };

    let draw_label = |frame: &mut Frame, content: &str, alpha: f32, x_off: f32| {
        if alpha <= 0.001 {
            return;
        }
        let display = truncate(content);
        if arced {
            // Arced layout: each character on a small arc lifted
            // above the puck so the cursor (which sits on the
            // centre during page-cycle scrolling) doesn't sit
            // behind the label. `arc_radius` clears the puck
            // edge by ~font_size * 0.5; the slide translates the
            // whole arc horizontally so animations stay coherent
            // with the flat layout's behaviour.
            let arc_radius = radius + label_size * 0.9;
            let cell_w = label_size * 0.6;
            let chars: Vec<char> = display.chars().collect();
            let count = chars.len() as f32;
            let step = (cell_w / arc_radius).max(0.001);
            let centre_angle = -std::f32::consts::FRAC_PI_2;
            for (i, ch) in chars.iter().enumerate() {
                // Distribute chars symmetrically around 12 o'clock.
                let centred = i as f32 - (count - 1.0) / 2.0;
                let angle = centre_angle + centred * step;
                let pos = Point::new(
                    center.x + arc_radius * angle.cos() + x_off,
                    center.y + arc_radius * angle.sin(),
                );
                // Tangent so chars rotate to follow the arc.
                // Top of menu → tangent = angle + π/2 keeps the
                // top of each glyph pointing outward (away from
                // the puck).
                let tangent = angle + std::f32::consts::FRAC_PI_2;
                let s: String = ch.to_string();
                frame.with_save(|f| {
                    f.translate(Vector::new(pos.x, pos.y));
                    f.rotate(Radians(tangent));
                    f.fill_text(iced::widget::canvas::Text {
                        content: s,
                        position: iced::Point::new(-cell_w / 2.0, -label_size / 2.0),
                        color: txt_color(alpha),
                        size: label_size.into(),
                        font: label_font,
                        ..iced::widget::canvas::Text::default()
                    });
                });
            }
        } else {
            // Flat layout: single fill_text centred in the puck.
            let w = approx_w(&display);
            frame.fill_text(iced::widget::canvas::Text {
                content: display,
                position: iced::Point::new(center.x - w / 2.0 + x_off, center.y - label_size / 2.0),
                color: txt_color(alpha),
                size: label_size.into(),
                font: label_font,
                ..iced::widget::canvas::Text::default()
            });
        }
    };

    if let (Some(prev), Some((alpha, x_off))) = (previous_name, outgoing) {
        draw_label(frame, prev, alpha, x_off);
    }
    draw_label(frame, current_name, in_alpha, in_x);

    true
}

/// Centre puck — small filled circle with stroked accent ring,
/// optionally with a label drawn inside (the hovered slice's name).
/// Ports `_draw_center` from the Python overlay; the centre-text
/// rendering is the long-promised "/* text overlay lands in a
/// follow-up */" finally landing here.
/// `label_alpha_mul` — extra opacity multiplier applied **only**
/// to the centre label and description. Lets transient labels
/// (e.g. the page-name flash on a cycle) fade out independently
/// of the puck fill / rim. Pass `1.0` for the standard
/// hover-label path; the page-name flash passes a ramped value
/// while it fades out.
#[allow(clippy::too_many_arguments)]
pub fn draw_center(
    frame: &mut Frame,
    center: Point,
    radius: f32,
    palette: &ThemeColors,
    menu_opacity: f32,
    bg_opacity: f32,
    label: Option<&str>,
    description: Option<&str>,
    label_size: f32,
    label_font: iced::Font,
    accent_flash: f32,
    label_alpha_mul: f32,
) {
    let mo = menu_opacity.clamp(0.0, 1.0);
    let bgo = bg_opacity.clamp(0.0, 1.0);
    let flash = accent_flash.clamp(0.0, 1.0);
    let puck = Path::circle(center, radius);
    // Puck fill tracks the same opacity slider as the wedge fill,
    // so the user gets one consistent "how see-through is the
    // menu" knob instead of the previous split where the puck
    // had its own 86 % cap.
    frame.fill(&puck, rgba(&palette.surface0, mo * bgo));
    // Default rim stroke — uses accent_dim. During an accent flash
    // (CenterPulse page transition) the rim brightens up to the
    // full accent colour and thickens slightly so the swap reads
    // as "the centre just clicked into a new page".
    let base_rim = rgba(&palette.accent_dim, (140.0 / 255.0) * mo * bgo);
    let rim_color = if flash > 0.0 {
        let bright = rgba(&palette.accent, mo * bgo);
        // Linear blend in unpremultiplied RGBA — close enough at
        // these alphas, and lerp() is local to this module's hover
        // code so we'd be reaching past visibility.
        iced::Color {
            r: base_rim.r + (bright.r - base_rim.r) * flash,
            g: base_rim.g + (bright.g - base_rim.g) * flash,
            b: base_rim.b + (bright.b - base_rim.b) * flash,
            a: base_rim.a + (bright.a - base_rim.a) * flash,
        }
    } else {
        base_rim
    };
    let rim_width = 2.0 + 2.0 * flash;
    frame.stroke(
        &puck,
        Stroke::default()
            .with_color(rim_color)
            .with_width(rim_width),
    );
    // Outer halo ring — only during a flash. Sits just outside the
    // puck and fades in/out with the pulse. Gives the swap a
    // visible "ripple" rather than a silent radius bump.
    if flash > 0.0 {
        let halo = Path::circle(center, radius + 6.0);
        let (ar, ag, ab, _) = parse_hex_rgba(&palette.accent).unwrap_or((1.0, 1.0, 1.0, 1.0));
        frame.stroke(
            &halo,
            Stroke::default()
                .with_color(iced::Color::from_rgba(
                    ar as f32,
                    ag as f32,
                    ab as f32,
                    0.55 * flash * mo,
                ))
                .with_width(2.0),
        );
    }

    let has_description = description.map(|d| !d.trim().is_empty()).unwrap_or(false);
    let description_size = (label_size * 0.62).max(8.0);

    if let Some(text) = label {
        if !text.is_empty() {
            // Truncate aggressively so long labels don't run off the
            // puck. The puck is ~90 px in diameter at the default
            // CENTER_ZONE_RADIUS=45, so ~10 chars max keeps
            // everything inside.
            let max_chars = ((radius * 2.0 / (label_size * 0.55)).max(4.0)) as usize;
            let display: String = if text.chars().count() > max_chars {
                let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
                out.push('…');
                out
            } else {
                text.to_string()
            };
            let approx_w = display.chars().count() as f32 * label_size * 0.55;
            let (tr, tg, tb, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));
            // Lift the label slightly when a description is also
            // shown so the two lines stack symmetrically across the
            // puck centre instead of the label sitting dead-centre
            // and the description hanging below.
            let label_y_offset = if has_description {
                -(description_size * 0.65)
            } else {
                0.0
            };
            let label_alpha = (mo * label_alpha_mul).clamp(0.0, 1.0);
            frame.fill_text(iced::widget::canvas::Text {
                content: display,
                position: iced::Point::new(
                    center.x - approx_w / 2.0,
                    center.y - label_size / 2.0 + label_y_offset,
                ),
                color: iced::Color::from_rgba(tr as f32, tg as f32, tb as f32, label_alpha),
                size: label_size.into(),
                font: label_font,
                ..iced::widget::canvas::Text::default()
            });
        }
    }

    if let Some(text) = description {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            // Description sits a half-line below the label; truncate
            // a bit more aggressively because the smaller font fits
            // more glyphs across the puck.
            let max_chars = ((radius * 2.0 / (description_size * 0.55)).max(6.0)) as usize;
            let display: String = if trimmed.chars().count() > max_chars {
                let mut out: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
                out.push('…');
                out
            } else {
                trimmed.to_string()
            };
            let approx_w = display.chars().count() as f32 * description_size * 0.55;
            let (tr, tg, tb, _) = parse_hex_rgba(&palette.subtext0).unwrap_or((0.7, 0.7, 0.7, 1.0));
            // Anchor description below the label baseline. The label
            // (when present) was nudged up by ~0.65× description
            // size; place description ~0.85× description size below
            // centre so the gap reads as a natural line break.
            frame.fill_text(iced::widget::canvas::Text {
                content: display,
                position: iced::Point::new(center.x - approx_w / 2.0, center.y + label_size * 0.05),
                color: iced::Color::from_rgba(
                    tr as f32,
                    tg as f32,
                    tb as f32,
                    (mo * label_alpha_mul * 0.85).clamp(0.0, 1.0),
                ),
                size: description_size.into(),
                font: label_font,
                ..iced::widget::canvas::Text::default()
            });
        }
    }
}

/// Draw the multi-page indicator — small dots arrayed in a shallow
/// arc that hugs the bottom rim of the centre puck. The arc is
/// anchored at 90° (straight down) and fans symmetrically left/right
/// from there, so when the active page is the middle of the cycle
/// the active dot sits at dead-bottom. No-op for single-page menus.
///
/// Sits along the puck's inside edge (just inboard of the rim
/// stroke) so the dots and the centre-hover label don't fight for
/// the same pixels.
pub fn draw_page_indicator(
    frame: &mut Frame,
    center: Point,
    radius: f32,
    palette: &ThemeColors,
    menu_opacity: f32,
    page_count: usize,
    active: Option<usize>,
) {
    if page_count < 2 {
        return;
    }
    let mo = menu_opacity.clamp(0.0, 1.0);
    let dot_r: f32 = 2.5;

    // Place the dots on a circle slightly inside the puck rim so
    // they read as "on the puck" without clipping the stroke.
    let arc_radius = (radius - dot_r * 2.5).max(dot_r * 2.0);

    // Angular step between adjacent dots. Aim for ~8 px chord
    // distance so the spacing visually matches the old straight
    // strip; clamp to a minimum so two-page menus don't crowd.
    let target_chord: f32 = 8.0;
    let mut step_rad = (target_chord / arc_radius.max(1.0)).max(0.22); // ≈ 12.6° min
                                                                       // Cap the total arc so the strip never sweeps past ~±45° from
                                                                       // straight-down — beyond that the dots start overlapping the
                                                                       // hover label and the page-cycle reads as a curve rather than
                                                                       // an indicator.
    let max_total_rad: f32 = std::f32::consts::FRAC_PI_2; // 90° total
    if (page_count as f32 - 1.0) * step_rad > max_total_rad {
        step_rad = max_total_rad / (page_count as f32 - 1.0).max(1.0);
    }
    let center_idx = (page_count as f32 - 1.0) / 2.0;
    // Bottom of the puck in iced canvas coords is +Y, which is
    // angle = π/2 (90°) from polar() since polar() uses
    // sin(angle) for Y with the canvas Y-down convention.
    let base_angle = std::f32::consts::FRAC_PI_2;

    let (ar, ag, ab, _) = parse_hex_rgba(&palette.accent).unwrap_or((1.0, 1.0, 1.0, 1.0));
    let (tr, tg, tb, _) = parse_hex_rgba(&palette.text).unwrap_or((1.0, 1.0, 1.0, 1.0));

    for i in 0..page_count {
        // Negate the offset so dot 0 lands on the LEFT and dot
        // N-1 on the RIGHT (Western reading order). Iced's
        // canvas Y is down, so a positive angle offset moves the
        // sample point counter-clockwise (toward the left at the
        // bottom of the puck). We want the opposite: dot index
        // grows left-to-right, so flip.
        let offset = (center_idx - i as f32) * step_rad;
        let angle = base_angle + offset;
        let pos = polar(center, arc_radius, angle);
        let path = Path::circle(pos, dot_r);
        let is_active = active == Some(i);
        let color = if is_active {
            iced::Color::from_rgba(ar as f32, ag as f32, ab as f32, mo)
        } else {
            iced::Color::from_rgba(tr as f32, tg as f32, tb as f32, 0.35 * mo)
        };
        frame.fill(&path, color);
    }
}

/// Ring treatment for the travelling page puck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuckRing {
    /// Accent stroke + outer glow — the puck is armed (page cycle
    /// live, chat render-only).
    Armed,
    /// Subtle inactive stroke — the chat owns input; the puck is
    /// still a wheel target but visually parked.
    Dimmed,
}

/// Draw the centre puck as a standalone object: dome-ish fill, ring
/// stroke per handoff phase, live page dots. Used by the chat
/// shell's `CapsPainter` to keep the puck visible (and travelling)
/// through the disc → chat morph; visually consistent with
/// `draw_center` + `draw_page_indicator` at the morph's t = 0
/// boundary so there's no pop when the painters swap.
#[allow(clippy::too_many_arguments)]
pub fn draw_puck(
    frame: &mut Frame,
    center: Point,
    radius: f32,
    palette: &ThemeColors,
    alpha: f32,
    ring: PuckRing,
    page_count: usize,
    active: Option<usize>,
) {
    let a = alpha.clamp(0.0, 1.0);
    if a <= 0.001 || radius <= 1.0 {
        return;
    }
    let body = Path::circle(center, radius);
    // Dome fill: crust base + an offset surface2 highlight fakes the
    // design's "radial-gradient(circle at 36% 30%, surface2, crust)"
    // within the canvas API's solid fills.
    frame.fill(&body, rgba(&palette.crust, 0.96 * a));
    let highlight = Path::circle(
        Point::new(center.x - radius * 0.28, center.y - radius * 0.40),
        radius * 0.50,
    );
    frame.fill(&highlight, rgba(&palette.surface2, 0.30 * a));

    match ring {
        PuckRing::Armed => {
            // Outer glow first so the crisp ring paints over it.
            let glow = Path::circle(center, radius + 2.5);
            frame.stroke(
                &glow,
                Stroke::default()
                    .with_color(rgba(&palette.accent, 0.30 * a))
                    .with_width(5.0),
            );
            frame.stroke(
                &body,
                Stroke::default()
                    .with_color(rgba(&palette.accent, a))
                    .with_width(2.5),
            );
        }
        PuckRing::Dimmed => {
            frame.stroke(
                &body,
                Stroke::default()
                    .with_color(rgba(&palette.overlay0, 0.9 * a))
                    .with_width(1.5),
            );
        }
    }

    draw_page_indicator(frame, center, radius, palette, a, page_count, active);
}

// =============================================================================
// helpers
// =============================================================================
