//! `view` — the widget tree: disc canvas + shader layer assembly
//! and the chat-shell composition.

use iced::widget::canvas::Canvas;
use iced::widget::{container, Space};
use iced::{Color, Element, Length};

use super::Message;
use crate::geometry::WINDOW_SIZE;
use crate::radial::{Painter, RadialState};

pub(super) fn view(state: &RadialState) -> Element<'_, Message> {
    let canvas = Canvas::new(Painter::new(state))
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));

    // Aurora backdrop — only stacked when the user has it on
    // (intensity > 0) AND the menu is at least partly visible.
    // Closed/dismissed menus skip the shader entirely so the
    // overlay window costs zero GPU when idle.
    let intensity = state.visuals.aurora_intensity.clamp(0.0, 1.0);
    // Every disc layer (shaders + canvas) fades out at the start of
    // the AI-chat morph — the painted caps take over visually. By
    // multiplying here, none of the 12 shader programs need to know
    // a split is happening; they just see the menu going to alpha 0.
    let morph = state.ai_morph_progress();
    let menu_alpha = state.menu.current.clamp(0.0, 1.0) * crate::chat_shell::disc_alpha(morph);
    let palette = &state.theme.theme.colors;
    let accent_rgba = oxidemx_shared::theme::parse_hex_rgba(&palette.accent)
        .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
        .unwrap_or([0.5, 0.5, 1.0, 1.0]);
    let mut layers: Vec<iced::Element<Message>> = Vec::with_capacity(8);
    // Pre-computed once so the drop-shadow block (and any later
    // shader that needs to normalise pixel radii) can reach it
    // without redundant arithmetic.
    let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
    // Single source of truth for the 3D-framing shaders' virtual
    // light direction. drop_shadow / disc_bevel / slice_bevel /
    // center_dome all read this so highlights and shadows stay
    // consistent across the disc when the user rotates the knob
    // in settings.
    let light_angle = state.visuals.light_angle_rad;

    // Drop shadow — bottom-most layer. Paints outside the disc
    // boundary, offset away from the virtual light source, so
    // the menu reads as a floating physical object instead of
    // pixels painted onto the screen. Goes BEFORE the aurora so
    // the aurora sits on top (the aurora paints inside the disc
    // anyway; the shadow is purely for the area outside).
    let drop_shadow_intensity = state.visuals.drop_shadow_intensity.clamp(0.0, 1.0);
    if drop_shadow_intensity > 0.001 && menu_alpha > 0.001 {
        let outer_norm = (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let shadow = iced::widget::Shader::new(crate::render::drop_shadow::DropShadowProgram {
            outer_r: outer_norm,
            intensity: drop_shadow_intensity * menu_alpha,
            // Spread the shadow ~25% past the disc edge.
            spread: 0.25,
            // Sharpish inner edge so the disc reads as
            // sitting clearly above its shadow.
            falloff: 0.55,
            // Read from the shared light direction (above).
            light_angle,
            // Offset the shadow centre 6% of half-extent
            // toward the lower-right so it reads as a cast
            // shadow, not a glow.
            offset_dist: 0.06,
            shadow_color: [0.0, 0.0, 0.0, 0.65],
        })
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(shadow.into());
    }

    // Compute the menu's `ComposedTransform` once and lift to the
    // shader-side `MenuXformRaw` block so menu-tracking shaders
    // (aurora + sdf_ring today; ripple/hover_glow/dispatch_burst
    // pending) inverse-transform their UVs and visually follow
    // the canvas when custom translate/rotate/flip tracks are set
    // on the menu element. Identity unless tracks are active —
    // zero perf impact on the preset path.
    let menu_t = crate::anim::evaluate_composed(
        &state.menu,
        &state.anim_config.menu.enter,
        &state.anim_config.menu.exit,
    );
    let menu_xform_raw =
        crate::render::animation::MenuXformRaw::from_composed(&menu_t, half_extent);

    // Aurora backdrop — bottom layer when enabled + menu visible.
    if intensity > 0.001 && menu_alpha > 0.001 {
        let accent2 = oxidemx_shared::theme::parse_hex_rgba(&palette.accent2)
            .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
            .unwrap_or(accent_rgba);
        let accent_dim = oxidemx_shared::theme::parse_hex_rgba(&palette.accent_dim)
            .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
            .unwrap_or(accent_rgba);
        let effective = intensity * menu_alpha;
        let aurora = iced::widget::Shader::new(crate::render::aurora::AuroraProgram::new(
            state.show_time.unwrap_or_else(std::time::Instant::now),
            accent_rgba,
            accent2,
            accent_dim,
            effective,
            menu_xform_raw,
        ))
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(aurora.into());
    }

    // SDF wedge ring spike — sits between aurora and the canvas
    // so the canvas's icons + text paint on top. The canvas's
    // wedge fills fade via `wedge_fill_mul` (hooked in
    // radial.rs::draw) so users can A/B by sliding intensity 0
    // → 1 and watching the canvas wedges fade out as the SDF
    // wedges fade in. Hover highlights + stroke + wash all live
    // in the SDF too — see `sdf_ring.wgsl` for the layered
    // composition.
    let sdf_intensity = state.visuals.sdf_ring_intensity.clamp(0.0, 1.0);
    if sdf_intensity > 0.001 && menu_alpha > 0.001 {
        let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
        let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32 + 6.0) / half_extent;
        let outer_norm = (crate::geometry::MENU_RADIUS as f32 - 6.0) / half_extent;
        // Icon centres sit at ICON_ZONE_RADIUS (100 px) from
        // the menu centre; the icon background disc is
        // ICON_BG_RADIUS (26 px). Both normalised to half-extent
        // for the shader.
        let icon_r_norm = crate::geometry::ICON_ZONE_RADIUS as f32 / half_extent;
        // ICON_BG_RADIUS lives in radial.rs as 26.0; replicate the
        // constant here rather than re-exporting it — adding a
        // pub constant exposes a tiny piece of the renderer's
        // internals that nothing else needs.
        let icon_bg_norm = 26.0_f32 / half_extent;
        let scale = menu_alpha;
        let palette = &state.theme.theme.colors;
        let (sr, sg, sb, _) = oxidemx_shared::theme::parse_hex_rgba(&palette.surface0)
            .unwrap_or((0.18, 0.18, 0.20, 1.0));
        let surface0 = [sr as f32, sg as f32, sb as f32, 1.0];
        let colors = [surface0; 8];
        let (s1r, s1g, s1b, _) = oxidemx_shared::theme::parse_hex_rgba(&palette.surface1)
            .unwrap_or((0.25, 0.25, 0.28, 1.0));
        let surface1_color = [s1r as f32, s1g as f32, s1b as f32, 1.0];
        let (s2r, s2g, s2b, _) = oxidemx_shared::theme::parse_hex_rgba(&palette.surface2)
            .unwrap_or((0.4, 0.4, 0.45, 1.0));
        let surface2_color = [s2r as f32, s2g as f32, s2b as f32, 1.0];
        // Canvas's stroke uses surface2 as its base colour, then
        // lerps to accent on hover. Same here.
        let stroke_color = surface2_color;
        let (ar, ag, ab, _) =
            oxidemx_shared::theme::parse_hex_rgba(&palette.accent).unwrap_or((0.5, 0.5, 1.0, 1.0));
        let accent_color = [ar as f32, ag as f32, ab as f32, 1.0];
        let highlights = state.highlights.map(|t| t.current);
        // Canvas wedges meet at exactly the slice-boundary angle —
        // no transparent gap, just the stroke band painting a thin
        // separator line. Match that here.
        let gap_rad: f32 = 0.0;
        let bg_op = state.visuals.menu_background_opacity.clamp(0.0, 1.0);
        let highlight_op = state.visuals.slice_highlight_opacity.clamp(0.0, 1.0);
        let sdf = iced::widget::Shader::new(crate::render::sdf_ring::SdfRingProgram {
            inner_r: inner_norm * scale,
            outer_r: outer_norm * scale,
            gap_rad,
            intensity: sdf_intensity * menu_alpha,
            slot_count: state.active_slot_count() as u32,
            base_alpha: bg_op,
            stroke_half_px: 1.2,
            hover_wash_peak: 0.275 * highlight_op,
            icon_r: icon_r_norm * scale,
            icon_bg_radius: icon_bg_norm * scale,
            colors,
            highlights,
            stroke_color,
            accent_color,
            surface1_color,
            surface2_color,
            menu_xform: menu_xform_raw,
        })
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(sdf.into());
    }

    layers.push(canvas.into());

    // Disc bevel — paints a rim light along the outer ring +
    // carved inset shadow at the inner ring, framing the disc
    // in 3D regardless of hover state. Layered ABOVE the canvas
    // because the lighting needs to sit on top of slice fills,
    // but only paints in thin bands at the boundaries (the wedge
    // interiors stay transparent so the canvas slice colours
    // come through unaffected).
    let disc_bevel_intensity = state.visuals.disc_bevel_intensity.clamp(0.0, 1.0);
    if disc_bevel_intensity > 0.001 && menu_alpha > 0.001 {
        let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
        // Multiply by menu_alpha so the bevel grows along with
        // the menu's open animation — same pattern the SDF ring
        // shader uses to stay in lockstep with the canvas.
        let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let outer_norm = (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let bevel = iced::widget::Shader::new(crate::render::disc_bevel::DiscBevelProgram {
            inner_r: inner_norm,
            outer_r: outer_norm,
            intensity: disc_bevel_intensity * menu_alpha,
            rim_width: 0.045,
            inset_width: 0.035,
            light_angle,
            shadow_strength: 0.7,
            rim_color: [1.0, 1.0, 1.0, 0.85],
            shadow_color: [0.0, 0.0, 0.0, 0.65],
        })
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(bevel.into());
    }

    // Specular sweep — animated narrow band of light rotating
    // around the disc rim. Sits on top of disc_bevel (which is
    // static) so the rotating gleam reads as additional motion
    // catching the rim, not as a new structural element. Lit-
    // side gated so the sweep fades on the shadow hemisphere.
    let sweep_intensity = state.visuals.specular_sweep_intensity.clamp(0.0, 1.0);
    if sweep_intensity > 0.001 && menu_alpha > 0.001 {
        let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let outer_norm = (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let sweep =
            iced::widget::Shader::new(crate::render::specular_sweep::SpecularSweepProgram {
                start: state.show_time.unwrap_or_else(std::time::Instant::now),
                inner_r: inner_norm,
                outer_r: outer_norm,
                intensity: sweep_intensity * menu_alpha,
                period_s: state.visuals.specular_sweep_period_s.max(0.5),
                // ~25° wide sweep — wide enough to read as
                // "polished surface" and not "laser pointer".
                half_width_rad: std::f32::consts::PI / 7.0,
                light_angle,
                sweep_color: [1.0, 1.0, 1.0, 0.85],
            })
            .width(Length::Fixed(WINDOW_SIZE as f32))
            .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(sweep.into());
    }

    // Slice bevel — paints the radial dividers between wedges
    // as carved grooves with directional rim lighting on the lit
    // side. Each slice ends up reading as its own raised 3D
    // button. Sits between disc_bevel (outer/inner ring) and
    // centre_dome (puck) in the visual stack.
    let slice_bevel_intensity = state.visuals.slice_bevel_intensity.clamp(0.0, 1.0);
    if slice_bevel_intensity > 0.001 && menu_alpha > 0.001 {
        let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
        let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let outer_norm = (crate::geometry::MENU_RADIUS as f32) / half_extent * menu_alpha;
        let slot_count = state.active_slot_count().max(1) as u32;
        let bevel = iced::widget::Shader::new(crate::render::slice_bevel::SliceBevelProgram {
            inner_r: inner_norm,
            outer_r: outer_norm,
            intensity: slice_bevel_intensity * menu_alpha,
            slot_count,
            // Width relative to half-extent. ~1.5% reads as
            // a subtle bevel at typical menu sizes.
            groove_width: 0.018,
            light_angle,
            rim_brightness: 0.9,
            shadow_amount: 0.7,
        })
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(bevel.into());
    }

    // Centre dome — Phong-shaded sphere over the centre puck.
    // Like the bevel: layered above the canvas because the
    // shader's lighting needs to sit on top of the puck's base
    // colour. Edge-faded so it doesn't form a hard ring.
    let center_dome_intensity = state.visuals.center_dome_intensity.clamp(0.0, 1.0);
    if center_dome_intensity > 0.001 && menu_alpha > 0.001 {
        let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
        let radius_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32) / half_extent * menu_alpha;
        let dome = iced::widget::Shader::new(crate::render::center_dome::CenterDomeProgram {
            radius: radius_norm,
            intensity: center_dome_intensity * menu_alpha,
            light_angle,
            shininess: 32.0,
            rim_brightness: 0.6,
            shadow_amount: 0.5,
            specular_color: [1.0, 1.0, 1.0, 0.95],
        })
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32));
        layers.push(dome.into());
    }

    // Hover glow — overlaid above the canvas when a slice is
    // hovered AND the highlight tween is non-zero. Uses the
    // hovered slice's colour (or accent fallback) and scales
    // with the tween's progress so the glow eases in/out
    // synchronously with the canvas-side wedge highlight.
    let hover_glow_intensity = state.visuals.hover_glow_intensity.clamp(0.0, 1.0);
    if hover_glow_intensity > 0.001 && menu_alpha > 0.001 {
        if let Some(idx) = state.target_slice() {
            let progress = state.highlights[idx].current.clamp(0.0, 1.0);
            if progress > 0.001 {
                let n = state.active_slot_count().max(1) as f32;
                let slice_degrees = std::f32::consts::PI * 2.0 / n;
                let bisector = (idx as f32) * slice_degrees - std::f32::consts::FRAC_PI_2;
                let half_sweep = slice_degrees / 2.0;
                // Wedge radii in normalised half-extent units.
                // Geometry::default() gives MENU_RADIUS=150 and
                // CENTER_ZONE_RADIUS=45 in a 484-px window
                // (half-extent = 242). Hard-coded here because
                // the shader works in clip-space [-1,1] and the
                // values aren't going to drift between frames.
                let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
                let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32 + 6.0) / half_extent;
                let outer_norm = (crate::geometry::MENU_RADIUS as f32 - 6.0) / half_extent;
                let slice = state.slices.get(idx);
                let color_key = slice
                    .and_then(|s| {
                        let c = s.color.trim();
                        if c.is_empty() {
                            None
                        } else {
                            Some(c)
                        }
                    })
                    .unwrap_or("accent");
                let palette = &state.theme.theme.colors;
                let (cr, cg, cb, _) = palette.slice_color_rgba(color_key);
                let color = [cr as f32, cg as f32, cb as f32, 1.0];
                let glow = iced::widget::Shader::new(crate::render::hover_glow::HoverGlowProgram {
                    bisector_rad: bisector,
                    half_sweep,
                    inner_r: inner_norm,
                    outer_r: outer_norm,
                    progress,
                    intensity: hover_glow_intensity * menu_alpha,
                    color,
                })
                .width(Length::Fixed(WINDOW_SIZE as f32))
                .height(Length::Fixed(WINDOW_SIZE as f32));
                layers.push(glow.into());
            }
        }
    }

    // Hover-tilt — paints inside-the-wedge directional lighting
    // (specular spot near cursor + soft shadow opposite). Layered
    // ABOVE hover_glow so its in-wedge tinting reads on top of
    // the edge aura. Uses the per-slice colour for the lit side
    // and the active accent for the specular dot.
    let hover_tilt_intensity = state.visuals.hover_tilt_intensity.clamp(0.0, 1.0);
    if hover_tilt_intensity > 0.001 && menu_alpha > 0.001 {
        if let Some(idx) = state.target_slice() {
            let progress = state.highlights[idx].current.clamp(0.0, 1.0);
            if progress > 0.001 {
                let n = state.active_slot_count().max(1) as f32;
                let slice_degrees = std::f32::consts::PI * 2.0 / n;
                let bisector = (idx as f32) * slice_degrees - std::f32::consts::FRAC_PI_2;
                let half_sweep = slice_degrees / 2.0;
                let half_extent = (crate::geometry::WINDOW_SIZE as f32) / 2.0;
                let inner_norm = (crate::geometry::CENTER_ZONE_RADIUS as f32 + 6.0) / half_extent;
                let outer_norm = (crate::geometry::MENU_RADIUS as f32 - 6.0) / half_extent;
                // Cursor in clip-UV [-1, 1]. RadialState stores
                // it in canvas pixels relative to centre, so just
                // divide by half_extent. Clamp generously so an
                // out-of-bounds cursor doesn't push the highlight
                // off-shader (it's gated by the inside-wedge SDF
                // anyway, but a sane uniform value avoids float
                // weirdness in the falloff math).
                let cursor_uv = [
                    (state.pointer_dx / half_extent as f64).clamp(-2.0, 2.0) as f32,
                    (state.pointer_dy / half_extent as f64).clamp(-2.0, 2.0) as f32,
                ];
                // Slice colour for the side wash; accent for the
                // specular dot. White-ish highlight reads as
                // light-source agnostic and pops against any
                // theme.
                let slice = state.slices.get(idx);
                let color_key = slice
                    .and_then(|s| {
                        let c = s.color.trim();
                        if c.is_empty() {
                            None
                        } else {
                            Some(c)
                        }
                    })
                    .unwrap_or("accent");
                let palette = &state.theme.theme.colors;
                let (ar, ag, ab, _) = palette.slice_color_rgba(color_key);
                let highlight_color = [1.0, 1.0, 1.0, 0.85];
                let accent_color = [ar as f32, ag as f32, ab as f32, 1.0];
                let tilt = iced::widget::Shader::new(crate::render::hover_tilt::HoverTiltProgram {
                    bisector_rad: bisector,
                    half_sweep,
                    inner_r: inner_norm,
                    outer_r: outer_norm,
                    progress,
                    intensity: hover_tilt_intensity * menu_alpha,
                    shadow_amount: state.visuals.hover_tilt_shadow.clamp(0.0, 1.0),
                    sharpness: state.visuals.hover_tilt_sharpness.clamp(0.0, 1.0),
                    cursor_uv,
                    highlight_color,
                    accent_color,
                })
                .width(Length::Fixed(WINDOW_SIZE as f32))
                .height(Length::Fixed(WINDOW_SIZE as f32));
                layers.push(tilt.into());
            }
        }
    }

    // Ripple — top layer when an event has triggered one and the
    // user has the effect on. Skipped when ripple_started is
    // None (no event fired) or the duration has elapsed (the
    // advance loop clears it).
    let ripple_intensity = state.visuals.ripple_intensity.clamp(0.0, 1.0);
    if let Some(started) = state.ripple_started {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if ripple_intensity > 0.001
            && menu_alpha > 0.001
            && elapsed_ms < crate::radial::RIPPLE_DURATION_MS
        {
            let progress = elapsed_ms as f32 / crate::radial::RIPPLE_DURATION_MS as f32;
            let ripple = iced::widget::Shader::new(crate::render::ripple::RippleProgram::new(
                progress,
                ripple_intensity * menu_alpha,
                accent_rgba,
            ))
            .width(Length::Fixed(WINDOW_SIZE as f32))
            .height(Length::Fixed(WINDOW_SIZE as f32));
            layers.push(ripple.into());
        }
    }

    // Page-transition shader overlay — runs whenever the
    // resolved shader style is non-None and the page-cycle
    // tween is active. `effective_style` combines the legacy
    // `style: Dissolve|Plasma` (canvas-only configs) with the
    // new explicit `shader.style` (orthogonal to whatever
    // canvas style is doing) so users can either keep their
    // old config OR layer e.g. SpinCrossfade canvas + Plasma
    // shader.
    let pt_cfg = &state.anim_config.page_transition;
    let shader_style = pt_cfg.shader.effective_style(pt_cfg.style);
    if !matches!(
        shader_style,
        oxidemx_shared::PageTransitionShaderStyle::None
    ) && state.previous_slices.is_some()
    {
        let progress = state.page_transition.current.clamp(0.0, 1.0);
        if progress > 0.001 && progress < 0.999 {
            let palette = &state.theme.theme.colors;
            let color_a = oxidemx_shared::theme::parse_hex_rgba(&palette.accent)
                .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
                .unwrap_or([0.5, 0.5, 1.0, 1.0]);
            let color_b = oxidemx_shared::theme::parse_hex_rgba(&palette.accent2)
                .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
                .unwrap_or(color_a);
            let style = match shader_style {
                oxidemx_shared::PageTransitionShaderStyle::Dissolve => {
                    crate::render::page_fx::PageFxStyle::Dissolve
                }
                _ => crate::render::page_fx::PageFxStyle::Plasma,
            };
            let s = &pt_cfg.shader;
            let layer = iced::widget::Shader::new(crate::render::page_fx::PageFxProgram {
                progress,
                style,
                intensity: menu_alpha * s.intensity.clamp(0.0, 1.0),
                dissolve_noise_scale: s.dissolve_noise_scale,
                dissolve_band_softness: s.dissolve_band_softness,
                plasma_wave_scale: s.plasma_wave_scale,
                plasma_wave_speed: s.plasma_wave_speed,
                color_a,
                color_b,
            })
            .width(Length::Fixed(WINDOW_SIZE as f32))
            .height(Length::Fixed(WINDOW_SIZE as f32));
            layers.push(layer.into());
        }
    }

    // Dispatch burst — top layer when an action just fired.
    // Deliberately NOT gated on `menu_alpha` so the burst keeps
    // playing as the menu fades out (and even after it's fully
    // closed): the user's last visible feedback should be the
    // celebratory flourish, not the ring sliding away.
    let burst_intensity = state.visuals.dispatch_burst_intensity.clamp(0.0, 1.0);
    if let (Some(started), Some(origin_idx)) = (state.dispatch_started, state.dispatch_origin) {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if burst_intensity > 0.001 && elapsed_ms < crate::radial::BURST_DURATION_MS {
            let progress = elapsed_ms as f32 / crate::radial::BURST_DURATION_MS as f32;
            let slot_count = state.active_slot_count();
            let origin = crate::render::dispatch_burst::slice_origin(origin_idx, slot_count);
            let palette = &state.theme.theme.colors;
            let slice = state.slices.get(origin_idx);
            let color_key = slice
                .and_then(|s| {
                    let c = s.color.trim();
                    if c.is_empty() {
                        None
                    } else {
                        Some(c)
                    }
                })
                .unwrap_or("accent");
            let (cr, cg, cb, _) = palette.slice_color_rgba(color_key);
            let style = crate::render::dispatch_burst::DispatchBurstStyleGpu::from(
                state.visuals.dispatch_burst_style,
            );
            let burst = iced::widget::Shader::new(
                crate::render::dispatch_burst::DispatchBurstProgram::new(
                    progress,
                    burst_intensity,
                    style,
                    origin,
                    [cr as f32, cg as f32, cb as f32, 1.0],
                ),
            )
            .width(Length::Fixed(WINDOW_SIZE as f32))
            .height(Length::Fixed(WINDOW_SIZE as f32));
            layers.push(burst.into());
        }
    }

    let main_stack: Element<'_, Message> = if layers.len() == 1 {
        layers.into_iter().next().unwrap()
    } else {
        iced::widget::Stack::with_children(layers).into()
    };
    // The 484 px disc stack stays anchored at the window's top-left.
    // Centering it via an aligned container looks nicer for wide
    // persisted chat sizes, but iced 0.14 clips canvas MESH layers
    // inside an offset container with a doubled offset — wedge and
    // dome fills vanish while icons/text survive. The morph instead
    // sweeps from the disc's square to the full window.

    // Chat shell composition: while the disc ↔ chat morph is in
    // flight (or parked at the chat end), stack the caps painter
    // over the (fading) disc layers, and the chat widgets over the
    // caps once the arcs are nearly parked. Outside the morph this
    // branch costs nothing — the disc path is exactly as before.
    if morph > 0.001 {
        let caps = Canvas::new(crate::chat_shell::CapsPainter::new(state))
            .width(Length::Fill)
            .height(Length::Fill);
        let open_alpha = state.menu_open_alpha();
        // The cache epsilon keeps every colour-derived quad layer
        // registering as changed each frame — see
        // chat_shell::cache_epsilon for the stale-layer story.
        let eps = crate::chat_shell::cache_epsilon();
        let chat_a = crate::chat_shell::chat_alpha(morph) * open_alpha * eps;
        let body_a = crate::chat_shell::cap_alpha(morph) * open_alpha * eps;

        // Window body — the chat's full-window chrome per the
        // redesign: 24 px outer radius, near-opaque `base` fill,
        // hairline `surface1` border, accent-tinted shadow. Fades
        // in on the cap ramp so the chrome arrives as the disc
        // hands over.
        let base_c = to_iced_color(&palette.base, Color::from_rgba(0.07, 0.08, 0.09, 1.0));
        let surface1_c = to_iced_color(&palette.surface1, Color::from_rgba(0.14, 0.16, 0.20, 1.0));
        let accent_c = to_iced_color(&palette.accent, Color::from_rgb(0.5, 0.5, 1.0));
        let yellow_c = to_iced_color(&palette.yellow, Color::from_rgb(1.0, 0.84, 0.31));

        // AI status cues (the 60 Hz tick keeps redrawing while the
        // menu is up, so a wall-clock sine reads as a smooth pulse):
        //   * thinking  → aurora brightens + breathes, footer dot
        //     pulses (chat_ui reads the same phase via Kit);
        //   * awaiting a choice → the window border breathes in the
        //     theme's yellow to pull the eye to the question card;
        //   * idle → the calm chrome below, exactly as before.
        let pulse = state
            .show_time
            .map(|t| {
                let secs = t.elapsed().as_secs_f32();
                ((secs * std::f32::consts::TAU / 1.4).sin() + 1.0) / 2.0
            })
            .unwrap_or(0.0);
        let awaiting_choice = state.ai_pending_question.is_some();
        let thinking = state.ai_loading && !awaiting_choice;

        let (border_c, border_w) = if awaiting_choice {
            (
                Color {
                    a: (0.45 + 0.55 * pulse) * body_a,
                    ..yellow_c
                },
                1.5,
            )
        } else {
            (
                Color {
                    a: body_a,
                    ..surface1_c
                },
                1.0,
            )
        };
        let body = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(Color {
                    a: body_a,
                    ..base_c
                })),
                border: iced::border::Border {
                    color: border_c,
                    width: border_w,
                    radius: 24.0.into(),
                },
                shadow: iced::Shadow {
                    color: Color {
                        a: 0.08 * body_a,
                        ..accent_c
                    },
                    offset: iced::Vector::new(0.0, 8.0),
                    blur_radius: 42.0,
                },
                ..Default::default()
            });

        let mut children: Vec<Element<'_, Message>> = vec![body.into()];

        // Aurora backdrop for the chat — the same theme-tinted
        // shader the disc uses, stretched over the whole window.
        // Its radial falloff discards past the inscribed ellipse,
        // so nothing spills into the transparent rounded corners.
        // OXIDEMX_NO_CHAT_AURORA=1 disables this layer — diagnostic
        // escape hatch for isolating post-resize compositing issues.
        let chat_aurora_enabled = std::env::var_os("OXIDEMX_NO_CHAT_AURORA").is_none();
        if chat_aurora_enabled && intensity > 0.001 && body_a > 0.001 {
            let accent2 = oxidemx_shared::theme::parse_hex_rgba(&palette.accent2)
                .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
                .unwrap_or(accent_rgba);
            let accent_dim = oxidemx_shared::theme::parse_hex_rgba(&palette.accent_dim)
                .map(|(r, g, b, a)| [r as f32, g as f32, b as f32, a as f32])
                .unwrap_or(accent_rgba);
            let chat_aurora = iced::widget::Shader::new(crate::render::aurora::ChatAuroraProgram(
                crate::render::aurora::AuroraProgram::new(
                    state.show_time.unwrap_or_else(std::time::Instant::now),
                    accent_rgba,
                    accent2,
                    accent_dim,
                    // Quiet backdrop at rest; brightens and breathes
                    // while a turn is in flight so "the AI is working"
                    // is visible from across the room.
                    intensity * if thinking { 0.5 + 0.3 * pulse } else { 0.22 } * body_a,
                    crate::render::animation::MenuXformRaw::IDENTITY,
                ),
            ))
            .width(Length::Fill)
            .height(Length::Fill);
            children.push(chat_aurora.into());
        }

        children.push(main_stack);
        children.push(caps.into());
        if chat_a > 0.01 {
            children.push(crate::chat_ui::view(state, chat_a));
        }
        iced::widget::Stack::with_children(children)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        main_stack
    }
}

// =============================================================================
// AI ASSISTANT PANEL DRAWING & HELPERS
// =============================================================================

fn to_iced_color(hex: &str, default: Color) -> Color {
    oxidemx_shared::theme::parse_hex_rgba(hex)
        .map(|(r, g, b, a)| Color::from_rgba(r as f32, g as f32, b as f32, a as f32))
        .unwrap_or(default)
}
