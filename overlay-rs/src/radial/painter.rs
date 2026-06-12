//! `Painter` — the per-frame `canvas::Program` that renders the
//! wedges / icons / centre puck and forwards toggle-mode mouse
//! events into the app loop.

use iced::widget::canvas::{self, Action, Frame, Geometry, Path};
use iced::{mouse, Color, Event, Point, Rectangle, Renderer, Theme};
use oxidemx_shared::{theme::parse_hex_rgba, Slice};

use super::RadialState;
use crate::geometry::{Geometry as RadialGeometry, CENTER_ZONE_RADIUS, MENU_RADIUS, WINDOW_SIZE};

const RING_OUTER_INSET: f32 = 6.0;
const RING_INNER_INSET: f32 = 6.0;
const ICON_BG_RADIUS: f32 = 26.0;

/// Per-frame canvas state.
pub struct Painter<'a> {
    state: &'a RadialState,
}

impl<'a> Painter<'a> {
    pub fn new(state: &'a RadialState) -> Self {
        Self { state }
    }
}

impl<'a> canvas::Program<crate::app::Message> for Painter<'a> {
    type State = ();

    /// Forward toggle-mode mouse events as `Message`s the iced
    /// app loop dispatches into `RadialState`. We only react when
    /// the menu is in toggle mode — drag mode is driven entirely
    /// by the daemon's CursorMoved signals (see app.rs).
    fn update(
        &self,
        _canvas_state: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<crate::app::Message>> {
        if !self.state.is_open() || !self.state.is_toggle_mode() {
            return None;
        }
        // While the chat shell is up (or in flight) the disc has no
        // hit surface — clicks belong to the chat widgets and the
        // caps painter. Without this gate a click on empty chat
        // background would fall through to ToggleClickSelect and
        // close the menu.
        if self.state.ai_morph_progress() > 0.01 {
            return None;
        }
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let p = cursor.position_in(bounds)?;
                Some(Action::publish(crate::app::Message::ToggleCursor {
                    x: p.x as f64,
                    y: p.y as f64,
                }))
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                // Page cycling fires only when the cursor is sitting
                // over the centre puck — keeps the user from
                // accidentally swapping pages while drag-aiming a
                // slice. Drag mode skips this branch entirely
                // (drag has no real cursor; the daemon sends REL_X
                // / REL_Y deltas). With fewer than two cycle pages
                // the cycle is a no-op, so we don't publish.
                if self.state.cycle_page_count() < 2 {
                    return None;
                }
                let p = cursor.position_in(bounds)?;
                let dx = p.x as f64 - WINDOW_SIZE / 2.0;
                let dy = p.y as f64 - WINDOW_SIZE / 2.0;
                let dist_sq = dx * dx + dy * dy;
                let dy_scroll = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                if dy_scroll.abs() < f32::EPSILON {
                    return None;
                }
                if dist_sq > CENTER_ZONE_RADIUS * CENTER_ZONE_RADIUS {
                    // Outside the page-cycle zone: wheel over a Dial
                    // wedge adjusts its value instead (brightness /
                    // volume quick set, no click needed).
                    let idx = crate::input::slice_index_at(
                        dx,
                        dy,
                        CENTER_ZONE_RADIUS,
                        MENU_RADIUS,
                        self.state.active_slot_count(),
                    )?;
                    if self
                        .state
                        .slices
                        .get(idx)
                        .map(|s| s.dial.is_some())
                        .unwrap_or(false)
                    {
                        let direction = if dy_scroll > 0.0 { 1 } else { -1 };
                        return Some(Action::publish(crate::app::Message::DialAdjust {
                            idx,
                            direction,
                        }));
                    }
                    return None;
                }
                // Scroll up (positive y) → next page; scroll down →
                // previous page. Matches the way the radial reads
                // visually: pages "stack downward" with the active
                // page on top, so flicking the wheel up advances
                // through the stack (same direction the user's
                // finger is moving).
                let direction = if dy_scroll > 0.0 { 1 } else { -1 };
                Some(Action::publish(crate::app::Message::CyclePage(direction)))
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) => match button {
                mouse::Button::Left => {
                    Some(Action::publish(crate::app::Message::ToggleClickSelect))
                }
                _ => Some(Action::publish(crate::app::Message::ToggleDismiss)),
            },
            Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) => {
                if matches!(
                    key,
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                ) {
                    Some(Action::publish(crate::app::Message::ToggleDismiss))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        if !self.state.is_drawable() {
            // Fully closed (no in-flight exit fade) — render a clear
            // frame and bail.
            return vec![frame.into_geometry()];
        }

        let geom = RadialGeometry::default();
        let center = Point::new(geom.cx as f32, geom.cy as f32);
        let palette = &self.state.theme.theme.colors;

        // Whole-menu visual modulation. `evaluate_composed` returns
        // a `ComposedTransform` carrying alpha + translate + rotate
        // + scale + flip-scale. The preset path (no custom_tracks)
        // produces `(alpha, scale)` matching the legacy
        // `Visual { scale, opacity }` and zero translate/rotate/flip,
        // so existing user configs render exactly as before.
        let menu_t = crate::anim::evaluate_composed(
            &self.state.menu,
            &self.state.anim_config.menu.enter,
            &self.state.anim_config.menu.exit,
        );
        let mscale = menu_t.scale.max(0.0);
        // The disc cross-fades into the chat shell's caps at the
        // start of the AI morph — same multiplier every shader
        // layer applies in app.rs::view.
        let mopacity = menu_t.alpha.clamp(0.0, 1.0)
            * crate::chat_shell::disc_alpha(self.state.ai_morph_progress());

        // Apply the menu-level translate / rotate / flip-scale to
        // the frame BEFORE any drawing. Identity transforms (the
        // preset path) bypass the matrix updates entirely. Scale
        // (uniform) keeps living in `mscale` so radii baked into
        // the wedge math still work without doubling up.
        crate::render::animation::apply_composed_transform(&mut frame, center, &menu_t);

        // The user's static "background opacity" knob multiplies
        // into the halo + base wedge fills (everything that draws
        // the wheel substrate). Highlights/icons are gated by a
        // separate knob below.
        let bg_op = self.state.visuals.menu_background_opacity.clamp(0.0, 1.0);
        // SDF wedge spike: when the SDF layer is active, fade the
        // canvas-side wedge fills so the SDF is what the user
        // actually sees. `wfm` (wedge_fill_mul) feeds through
        // draw_ring_transformed → draw_slice → fill alphas. At
        // SDF intensity 0 this is 1.0 and the canvas behaves
        // exactly as it always has.
        let sdf_intensity = self.state.visuals.sdf_ring_intensity.clamp(0.0, 1.0);
        let wfm = 1.0 - sdf_intensity;
        let highlight_op = self.state.visuals.slice_highlight_opacity.clamp(0.0, 1.0);

        // Faint shadow halo so the disc reads against transparent
        // backgrounds.
        let halo = Path::circle(center, (MENU_RADIUS as f32 + 6.0) * mscale);
        frame.fill(
            &halo,
            Color::from_rgba(0.0, 0.0, 0.0, 0.35 * mopacity * bg_op),
        );

        let outer_r = ((MENU_RADIUS as f32) - RING_OUTER_INSET) * mscale;
        let inner_r = ((geom.center_radius as f32) + RING_INNER_INSET) * mscale;
        let icon_r = (geom.icon_radius as f32) * mscale;
        let icon_bg_r = ICON_BG_RADIUS * mscale;

        // Page-transition state: when previous_slices is Some we're
        // mid-cycle and need to render BOTH rings (old fading out,
        // new fading in). The tween's eased current value drives
        // the crossfade and any per-ring transform.
        //
        // Per-ring transforms are computed as `ComposedTransform`
        // values so the preset path and the custom-track path
        // share one renderer signature. When the user has set
        // custom tracks on `page_transition.animation.enter` /
        // `.exit`, those override the preset; otherwise the
        // preset's `style` (None / CrossfadeScale / SpinCrossfade
        // / CenterPulse / Flip / Dissolve / Plasma) feeds the
        // ComposedTransform fields directly.
        let pt_progress = self.state.page_transition.current.clamp(0.0, 1.0);
        let pt_active = self.state.previous_slices.is_some() && pt_progress < 1.0;
        let pt_cfg = &self.state.anim_config.page_transition;
        let pt_style = pt_cfg.style;
        let pt_dir = self.state.page_transition_dir;
        let pt_rot_max = pt_cfg.rotation_deg.to_radians();

        let pt_uses_custom_tracks =
            pt_cfg.animation.enter.is_custom() || pt_cfg.animation.exit.is_custom();

        let (old_t, new_t) = if !pt_active {
            // No transition in flight — incoming ring at rest, no
            // outgoing ring (its alpha is 0 so it won't render).
            let mut hidden = oxidemx_shared::ComposedTransform::IDENTITY;
            hidden.alpha = 0.0;
            (hidden, oxidemx_shared::ComposedTransform::IDENTITY)
        } else if pt_uses_custom_tracks {
            // Track-based: outgoing ring uses Exit semantics,
            // incoming uses Enter. Same tween drives both — just
            // evaluate twice with explicit directions.
            let new_t = crate::anim::evaluate_composed_for(
                &self.state.page_transition,
                &pt_cfg.animation.enter,
                oxidemx_shared::TransitionDirection::Enter,
            );
            let old_t = crate::anim::evaluate_composed_for(
                &self.state.page_transition,
                &pt_cfg.animation.exit,
                oxidemx_shared::TransitionDirection::Exit,
            );
            (old_t, new_t)
        } else {
            // Preset path. The shape of each style matches the
            // legacy code that returned (rot, scale, alpha) tuples —
            // we just lift them into ComposedTransform.
            use oxidemx_shared::ComposedTransform as CT;
            use oxidemx_shared::PageTransitionStyle as PTS;
            let p = pt_progress;
            let inv = 1.0 - p;
            let mut old_t = CT::IDENTITY;
            let mut new_t = CT::IDENTITY;
            match pt_style {
                PTS::None => {
                    old_t.alpha = 0.0;
                    new_t.alpha = 1.0;
                }
                PTS::CrossfadeScale => {
                    let scale_min = 0.92_f32;
                    old_t.scale = 1.0 + (scale_min - 1.0) * p; // 1.0 → 0.92
                    old_t.alpha = inv;
                    new_t.scale = scale_min + (1.0 - scale_min) * p;
                    new_t.alpha = p;
                }
                PTS::SpinCrossfade => {
                    old_t.rotate_rad = pt_rot_max * pt_dir * p;
                    old_t.alpha = inv;
                    new_t.rotate_rad = -pt_rot_max * pt_dir * inv;
                    new_t.alpha = p;
                }
                PTS::CenterPulse => {
                    // Slices don't move; centre puck pulses.
                    old_t.alpha = 0.0;
                    new_t.alpha = 1.0;
                }
                PTS::Dissolve | PTS::Plasma => {
                    // Shader styles paint their own thing on top
                    // via the page-fx layer; canvas does a plain
                    // crossfade underneath.
                    old_t.alpha = inv;
                    new_t.alpha = p;
                }
                PTS::Flip => {
                    // Card-flip around the Y axis. Old ring scales
                    // its X axis from 1 → 0 over the FIRST HALF of
                    // the transition (rotating 0° → 90°). New ring
                    // scales X from 0 → 1 over the SECOND HALF
                    // (rotating 90° → 0°). cos/sin of (p * π/2)
                    // gives the natural physical-flip feel.
                    let half_pi = std::f32::consts::FRAC_PI_2;
                    let cos_p = (p * half_pi).cos();
                    let sin_p = (p * half_pi).sin();
                    old_t.flip_scale = cos_p;
                    old_t.flip_axis = oxidemx_shared::Axis::Y;
                    old_t.alpha = cos_p;
                    new_t.flip_scale = sin_p;
                    new_t.flip_axis = oxidemx_shared::Axis::Y;
                    new_t.alpha = sin_p;
                }
            }
            (old_t, new_t)
        };

        // Outgoing ring — only when actively transitioning.
        if pt_active {
            if let Some(old_slices) = self.state.previous_slices.as_ref() {
                if old_t.alpha > 0.001 {
                    crate::render::slices::draw_ring_transformed(
                        &mut frame,
                        center,
                        inner_r,
                        outer_r,
                        icon_r,
                        icon_bg_r,
                        old_slices,
                        palette,
                        // No hover highlights on the outgoing ring.
                        &[0.0; 8],
                        mopacity,
                        bg_op,
                        highlight_op,
                        &self.state.icons,
                        &old_t,
                        None,
                        self.state.active_slot_count(),
                        wfm,
                        &self.state.widgets,
                    );
                }
            }
        }

        // Per-slot transforms drive the slice-highlight custom-track
        // path. Each slot's tween + slice_highlight enter/exit
        // animate independently, so the per-slot transform is
        // wrapped inside `draw_ring_transformed` per slice. With
        // no custom tracks the array is all-identity and the inner
        // loop skips the per-slot `with_save` entirely (None branch).
        let slot_transforms: [oxidemx_shared::ComposedTransform; 8] =
            self.state.highlights.map(|tween| {
                crate::anim::evaluate_composed(
                    &tween,
                    &self.state.anim_config.slice_highlight.enter,
                    &self.state.anim_config.slice_highlight.exit,
                )
            });
        let slice_uses_custom_tracks = self.state.anim_config.slice_highlight.enter.is_custom()
            || self.state.anim_config.slice_highlight.exit.is_custom();
        let slot_transforms_arg = if slice_uses_custom_tracks {
            Some(&slot_transforms)
        } else {
            None
        };

        // Incoming / current ring. Outside a transition `new_t` is
        // identity, so this is the normal single-ring path with
        // no extra transform cost.
        crate::render::slices::draw_ring_transformed(
            &mut frame,
            center,
            inner_r,
            outer_r,
            icon_r,
            icon_bg_r,
            &self.state.slices,
            palette,
            &self.state.highlights.map(|t| t.current),
            mopacity,
            bg_op,
            highlight_op,
            &self.state.icons,
            &new_t,
            slot_transforms_arg,
            self.state.active_slot_count(),
            wfm,
            &self.state.widgets,
        );

        // Centre label + description: prefer the hovered submenu
        // item (deepest selection wins) → the hovered top-level
        // slice → nothing. Description is the slice's optional
        // longer-form notes field — when present it renders as a
        // smaller subtitle under the label inside the puck.
        let hovered_slice: Option<&oxidemx_shared::config::Slice> = self
            .state
            .submenu
            .as_ref()
            .and_then(|sub| {
                sub.highlighted.and_then(|child_idx| {
                    self.state
                        .slices
                        .get(sub.parent)
                        .and_then(|p| p.submenu.get(child_idx))
                })
            })
            .or_else(|| {
                self.state
                    .target_slice
                    .and_then(|i| self.state.slices.get(i))
            });
        // Centre-label resolution: hovered slice always wins (the
        // user's pointer intent is the strongest signal). When
        // no slice is hovered, the page-name flash takes the
        // centre — but it's drawn by `draw_page_name_transition`
        // *after* `draw_center` so the slide-in / cross-slide
        // animation happens out-of-band. Pass center_label=None
        // here when the flash is active so draw_center doesn't
        // also render a static label that fights with it.
        let page_name_active = self
            .state
            .page_name_flash
            .as_ref()
            .filter(|_| self.state.visuals.page_name_show)
            .map(|f| {
                let elapsed = f.started_at.elapsed().as_millis() as u64;
                let total_ms = (2 * self.state.visuals.page_name_transition_ms
                    + self.state.visuals.page_name_visible_ms)
                    as u64;
                elapsed < total_ms
            })
            .unwrap_or(false);
        // Hover labels take precedence; with nothing hovered the
        // puck label stays empty (the page-name flash below paints
        // the centre itself while it is active).
        let center_label: Option<String> = hovered_slice.map(|s| s.label.clone());
        let center_label_alpha_mul: f32 = 1.0;
        // Description used to render as a centre-puck subtitle;
        // moved to an arced tooltip around the outer ring (see
        // below). Pass `None` so the puck stays clean when the
        // user is dwelling on a slice with notes.
        let center_description: Option<String> = None;
        // Tooltip description: pulled from `target_slice`, NOT
        // `hovered_slice`. With the submenu-keep-parent-lit
        // logic in `update_pointer`, target_slice stays the
        // parent slot while a submenu is open, so the tooltip
        // arc shows the parent's description while the user
        // navigates into sub-items. hovered_slice (which prefers
        // sub-item) drives the centre puck label instead.
        let hovered_description: Option<String> = self
            .state
            .target_slice
            .and_then(|i| self.state.slices.get(i))
            .map(|s| s.description.clone())
            .filter(|d| !d.trim().is_empty());

        // CenterPulse style: brief radius bulge + accent flash on
        // the puck during a page-cycle transition. Peaks at the
        // halfway point so the eye lands on the centre as the new
        // ring's first frame appears. No effect for other styles.
        let pulse_factor =
            if pt_active && matches!(pt_style, oxidemx_shared::PageTransitionStyle::CenterPulse) {
                // Triangle wave: 0 at p=0, 1 at p=0.5, 0 at p=1.
                1.0 - (2.0 * pt_progress - 1.0).abs()
            } else {
                0.0
            };
        let center_radius = (geom.center_radius as f32) * mscale * (1.0 + 0.18 * pulse_factor);
        crate::render::slices::draw_center(
            &mut frame,
            center,
            center_radius,
            palette,
            mopacity,
            bg_op,
            center_label.as_deref(),
            center_description.as_deref(),
            self.state.visuals.center_label_size,
            crate::fonts::resolve(&self.state.visuals.font_family),
            pulse_factor,
            center_label_alpha_mul,
        );

        // Page-name transition: drawn AFTER the puck so the
        // slide-in / cross-slide-out happens on top of the
        // surface0 fill + accent rim. Skipped when the user is
        // hovering a slice (hover label takes priority) or when
        // the flash has elapsed / is disabled in settings.
        if hovered_slice.is_none() && page_name_active {
            if let Some(flash) = self.state.page_name_flash.as_ref() {
                let elapsed = flash.started_at.elapsed().as_millis() as u64;
                let v = &self.state.visuals;
                // Font resolution mirrors the arc-tooltip path:
                // monospace toggle wins (forces platform mono),
                // else use the page-name-specific override, else
                // inherit the menu font_family. Arc layouts
                // really need monospace because every angular
                // slot has uniform width — proportional fonts
                // create visible "gaps" around narrow glyphs.
                let font = if v.page_name_use_monospace {
                    iced::Font::MONOSPACE
                } else {
                    let family = if v.page_name_font_family.trim().is_empty() {
                        v.font_family.as_str()
                    } else {
                        v.page_name_font_family.as_str()
                    };
                    crate::fonts::resolve(family)
                };
                crate::render::slices::draw_page_name_transition(
                    &mut frame,
                    center,
                    center_radius,
                    palette,
                    mopacity,
                    &flash.current_name,
                    flash.previous_name.as_deref(),
                    flash.direction,
                    elapsed,
                    v.page_name_visible_ms,
                    v.page_name_transition_ms,
                    v.page_name_slide_distance_px,
                    v.center_label_size,
                    font,
                    v.page_name_arced,
                );
            }
        }

        // Page indicator dots — only appear when the menu has more
        // than one page in the scroll cycle. Sits inside the centre
        // puck, below any hover label.
        crate::render::slices::draw_page_indicator(
            &mut frame,
            center,
            center_radius,
            palette,
            mopacity,
            self.state.cycle_page_count(),
            self.state.cycle_page_position(),
        );

        // Submenu pop-out (drawn AFTER the centre so its sub-items
        // sit cleanly on top of the ring instead of being clipped
        // by the wedges they're popping out of). Custom-track
        // transforms apply to the submenu independently of the
        // menu's own transform — wrapping in `with_save` keeps
        // them scoped to this block.
        if let Some(sub) = self.state.submenu.as_ref() {
            let submenu_t = crate::anim::evaluate_composed(
                &sub.progress,
                &self.state.anim_config.submenu.enter,
                &self.state.anim_config.submenu.exit,
            );
            frame.with_save(|f| {
                crate::render::animation::apply_composed_transform(f, center, &submenu_t);
                crate::render::slices::draw_submenu(
                    f,
                    center,
                    sub,
                    &self.state.slices,
                    palette,
                    &self.state.anim_config.submenu,
                    mopacity * submenu_t.alpha.clamp(0.0, 1.0),
                    &self.state.icons,
                );
            });
        }

        // Arced tooltip — only when the user has dwelled on a
        // slice for at least `tooltip_delay_ms` AND that slice
        // has a non-empty description. Skip during page
        // transitions (the moving ring would drag the tooltip
        // along with it visually).
        if !pt_active {
            if let (Some(idx), Some(desc), Some(since)) = (
                self.state.target_slice,
                hovered_description.as_deref(),
                self.state.target_slice_since,
            ) {
                let elapsed_ms = since.elapsed().as_millis() as u32;
                let delay = self.state.visuals.tooltip_delay_ms;
                let font_size = self.state.visuals.tooltip_font_size;
                if elapsed_ms >= delay && font_size > 0.5 {
                    // Fade in over 150 ms once the delay expires.
                    const FADE_MS: f32 = 150.0;
                    let alpha = ((elapsed_ms - delay) as f32 / FADE_MS).clamp(0.0, 1.0);
                    let outer_r = ((MENU_RADIUS as f32) - RING_OUTER_INSET) * mscale;
                    let v = &self.state.visuals;
                    let font = if v.tooltip_use_monospace {
                        iced::Font::MONOSPACE
                    } else {
                        let family = if v.tooltip_font_family.trim().is_empty() {
                            v.font_family.as_str()
                        } else {
                            v.tooltip_font_family.as_str()
                        };
                        crate::fonts::resolve(family)
                    };
                    let bg_hex = palette
                        .lookup(&v.tooltip_bg_color)
                        .unwrap_or(palette.crust.as_str());
                    let fg_hex = palette
                        .lookup(&v.tooltip_text_color)
                        .unwrap_or(palette.text.as_str());
                    let (br, bg_g, bb, ba) = oxidemx_shared::theme::parse_hex_rgba(bg_hex)
                        .unwrap_or((0.0, 0.0, 0.0, 1.0));
                    let (fr, fg_g, fb, fa) = oxidemx_shared::theme::parse_hex_rgba(fg_hex)
                        .unwrap_or((1.0, 1.0, 1.0, 1.0));
                    let style = crate::render::slices::ArcTooltipStyle {
                        font,
                        monospace: v.tooltip_use_monospace,
                        fg: iced::Color::from_rgba(fr as f32, fg_g as f32, fb as f32, fa as f32),
                        bg: iced::Color::from_rgba(br as f32, bg_g as f32, bb as f32, ba as f32),
                        bg_alpha: v.tooltip_bg_alpha,
                    };
                    crate::render::slices::draw_arc_tooltip(
                        &mut frame,
                        center,
                        outer_r,
                        idx,
                        self.state.active_slot_count(),
                        desc,
                        palette,
                        mopacity,
                        alpha,
                        font_size * mscale,
                        style,
                    );
                }
            }
        }

        vec![frame.into_geometry()]
    }
}

#[allow(dead_code)]
fn _link_window_size() -> u32 {
    WINDOW_SIZE as u32
}

#[allow(dead_code)]
fn _link_parse() -> Option<(f64, f64, f64, f64)> {
    parse_hex_rgba("#000000")
}

// Slice references kept private but visible to the painter via the
// state borrow above; this `_use` ensures cargo doesn't drop the
// type from the public surface when we add it elsewhere.
#[allow(dead_code)]
fn _slice_ref(_s: &Slice) {}
