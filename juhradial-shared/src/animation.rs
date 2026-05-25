//! User-tweakable animation parameters for the radial overlay.
//!
//! Inspired by Motion.dev's React API: every transition is described
//! by a *kind* (what visual property to interpolate — fade, grow,
//! grow+fade), a *duration* in milliseconds, an *easing* curve
//! (linear / ease-in / ease-out / ease-in-out / spring), and a
//! handful of element-specific knobs (initial scale / opacity, end
//! opacity, pre-roll delay).
//!
//! Each animated *element* (the menu itself, the submenu pop-out,
//! a slice's hover highlight) carries an independent enter/exit
//! transition pair plus an optional `chain` block that controls
//! per-item stagger when the element renders multiple sub-pieces
//! (e.g. submenu sub-items).
//!
//! Config lives in `~/.config/juhradial/config.json` →
//! `radial_menu.animation` and is reloaded live by the overlay's
//! inotify watcher, so users can iterate without restarting.

use serde::{Deserialize, Serialize};

// ============================================================================
// Easing curves
// ============================================================================

/// Easing curve applied to the [0, 1] normalised time of a
/// transition. `Linear` and the `Ease*` cubic curves are exact
/// closed-form expressions; `Spring` is a damped harmonic
/// oscillator simulated against a normalised time axis (so the
/// transition still respects its `duration_ms` window — the spring
/// just shapes what happens within it).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    /// Spring physics — `stiffness` controls oscillation frequency
    /// (higher = snappier), `damping` controls overshoot (lower =
    /// bouncier). Defaults match Motion.dev's "gentle" preset.
    Spring {
        #[serde(default = "default_spring_stiffness")]
        stiffness: f32,
        #[serde(default = "default_spring_damping")]
        damping: f32,
    },
}

fn default_spring_stiffness() -> f32 {
    100.0
}
fn default_spring_damping() -> f32 {
    12.0
}

impl Default for Easing {
    fn default() -> Self {
        Easing::EaseOut
    }
}

impl Easing {
    /// Map normalised time `t` ∈ [0, 1] to a [0, 1+overshoot]
    /// progress value. The renderer uses this to interpolate the
    /// element's start/end values.
    pub fn eval(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match *self {
            Easing::Linear => t,
            Easing::EaseIn => t * t * t,
            Easing::EaseOut => {
                let p = 1.0 - t;
                1.0 - p * p * p
            }
            Easing::EaseInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let p = -2.0 * t + 2.0;
                    1.0 - (p * p * p) / 2.0
                }
            }
            Easing::Spring { stiffness, damping } => spring_eval(t, stiffness, damping),
        }
    }
}

/// Damped harmonic oscillator solution, parameterised so that the
/// transition completes (current ≈ target) within the [0, 1]
/// normalised time window for a wide range of stiffness / damping
/// values. We scale physical time by ~6× the normalised time —
/// roughly five oscillator time-constants — which is enough for
/// even a soft spring (low stiffness) to land near 1.0 by t = 1.0,
/// while still letting a stiff one finish its visible bounce well
/// before the end of the window.
fn spring_eval(t: f32, stiffness: f32, damping: f32) -> f32 {
    let mass = 1.0_f32;
    let k = stiffness.max(1.0);
    let c = damping.max(0.0);
    let omega = (k / mass).sqrt();
    let zeta = c / (2.0 * (k * mass).sqrt());

    // Map [0,1] of transition time onto the oscillator's natural
    // time axis. Constant chosen so duration_ms behaves like an
    // intuitive "total duration" knob — at t=1 the spring has
    // settled within ~0.5 % for the default stiffness/damping.
    let phys_t = t * 6.0 / omega.sqrt().max(0.1);

    if (zeta - 1.0).abs() < 1e-3 {
        // Critically damped — smooth, no overshoot.
        let e = (-omega * phys_t).exp();
        1.0 - e * (1.0 + omega * phys_t)
    } else if zeta < 1.0 {
        // Underdamped — overshoots and oscillates. The bouncy case.
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let e = (-zeta * omega * phys_t).exp();
        let cos_t = (omega_d * phys_t).cos();
        let sin_t = (omega_d * phys_t).sin();
        1.0 - e * (cos_t + (zeta * omega / omega_d) * sin_t)
    } else {
        // Overdamped — slow, smooth approach (sluggish).
        let r = (zeta * zeta - 1.0).sqrt();
        let r1 = -omega * (zeta - r);
        let r2 = -omega * (zeta + r);
        let denom = (r1 - r2).max(1e-6);
        let c1 = -r2 / denom;
        let c2 = r1 / denom;
        1.0 - c1 * (r1 * phys_t).exp() - c2 * (r2 * phys_t).exp()
    }
}

// ============================================================================
// TransitionKind
// ============================================================================

/// What property a transition interpolates. Every kind respects the
/// same duration / easing / delay knobs; the kind only changes
/// *what* the eased progress controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    /// No animation — value snaps directly to the target.
    None,
    /// Opacity-only ramp from `initial_opacity` to `final_opacity`.
    Fade,
    /// Scale ramp from `initial_scale` to 1.0. Opacity stays at
    /// `final_opacity` (default 1.0) for the whole transition.
    Grow,
    /// Scale + opacity together — `initial_scale` → 1.0 with
    /// `initial_opacity` → `final_opacity` on the same eased curve.
    /// The Motion.dev "scale + fade" combo.
    GrowAndFade,
}

impl Default for TransitionKind {
    /// `None` so any field a user *omits* from a partial config
    /// block doesn't accidentally introduce an animation. Element-
    /// level "recommended" presets in `ElementAnimation::*_default`
    /// are what you get when the whole `animation` block is
    /// missing.
    fn default() -> Self {
        TransitionKind::None
    }
}

// ============================================================================
// Custom track-based animations (Motion-style, composable)
// ============================================================================
//
// Each TransitionConfig (enter or exit) optionally carries a list of
// `AnimationTrack`s. When the list is non-empty, it overrides the
// preset (`kind` + `initial_*` / `final_*`). Tracks run *simultaneously*
// — each has its own delay, duration, and easing curve, all driven
// off the parent element's transition clock.
//
// The four basic kinds map to a single contribution to the composed
// per-frame transform: opacity multiplier, translation in pixels,
// rotation in degrees, or scale percent. Plus a `Flip` kind that
// approximates a 3D card flip via scale-on-axis on the canvas side
// (true perspective requires shader work, deferred).
//
// Convention for Enter vs Exit:
//   * Enter: animates `off_state` → resting (the visible default).
//   * Exit:  animates resting → `off_state`.
// `off_state` is what the user picks per track ("starting position"
// for Enter, "ending position" for Exit) and the resting state is
// always the implicit identity (alpha 1, no translate/rotate, scale
// 100%, no flip).

/// Axis for direction-sensitive track kinds (Translate, Flip).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
}

impl Default for Axis {
    fn default() -> Self {
        Axis::X
    }
}

/// One animation track contributing a single property to the
/// composed transform. Multiple tracks per direction compose
/// element-wise (alpha multiplies, translates / rotates / scales
/// add).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrackKind {
    /// Opacity ramp. Enter: `off_alpha` → 1.0. Exit: 1.0 →
    /// `off_alpha`. Default 0.0 = full fade in/out.
    Fade {
        #[serde(default)]
        off_alpha: f32,
    },
    /// Translation along an axis, in logical pixels. Enter:
    /// element starts at `offset_px` and slides to 0. Exit:
    /// element slides from 0 to `offset_px`. Sign convention:
    /// positive = right (X) / down (Y).
    Translate {
        #[serde(default)]
        axis: Axis,
        #[serde(default)]
        offset_px: f32,
    },
    /// Rotation around the element centre, in degrees. Enter:
    /// `offset_deg` → 0. Exit: 0 → `offset_deg`.
    Rotate {
        #[serde(default)]
        offset_deg: f32,
    },
    /// Scale, in percent. 100 = identity. Enter: `offset_pct`
    /// → 100. Exit: 100 → `offset_pct`. Common: 0 (pop in/out
    /// from a point), 80 (subtle squish), 120 (overshoot).
    Scale {
        #[serde(default = "default_scale_offset_pct")]
        offset_pct: f32,
    },
    /// 3D card-flip approximation. Rotates around the chosen
    /// `axis` by `offset_deg` (typically ±180). Canvas renders
    /// this as a `|cos(angle)|` scale on the perpendicular axis
    /// — reads as the element rotating edge-on, no true
    /// perspective foreshortening (would need a shader pass).
    /// Enter: `offset_deg` → 0. Exit: 0 → `offset_deg`.
    Flip {
        #[serde(default)]
        axis: Axis,
        #[serde(default = "default_flip_offset_deg")]
        offset_deg: f32,
    },
}

fn default_scale_offset_pct() -> f32 {
    0.0
}

fn default_flip_offset_deg() -> f32 {
    180.0
}

impl TrackKind {
    /// Stable identifier used by the settings UI to render the
    /// type combobox + per-kind parameter sliders. Keep in sync
    /// with the WGSL / canvas side that consumes these.
    pub fn variant_name(&self) -> &'static str {
        match self {
            TrackKind::Fade { .. } => "Fade",
            TrackKind::Translate { .. } => "Translate",
            TrackKind::Rotate { .. } => "Rotate",
            TrackKind::Scale { .. } => "Scale",
            TrackKind::Flip { .. } => "Flip",
        }
    }

    /// Default-shaped variants used by "Add track" buttons in the
    /// settings UI. Each is what the user sees when they pick
    /// the variant fresh — sensible non-trivial values that
    /// produce a *visible* animation without the user touching
    /// any sliders first.
    pub fn default_for(name: &str) -> Self {
        match name {
            "Fade" => TrackKind::Fade { off_alpha: 0.0 },
            "Translate" => TrackKind::Translate {
                axis: Axis::Y,
                offset_px: -32.0,
            },
            "Rotate" => TrackKind::Rotate { offset_deg: -45.0 },
            "Scale" => TrackKind::Scale { offset_pct: 60.0 },
            "Flip" => TrackKind::Flip {
                axis: Axis::Y,
                offset_deg: 180.0,
            },
            _ => TrackKind::Fade { off_alpha: 0.0 },
        }
    }
}

/// One animation track. Tracks within the same direction (enter
/// or exit) run simultaneously, each on its own clock derived
/// from the element's transition start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationTrack {
    pub kind: TrackKind,
    /// Pre-roll delay in milliseconds. The track holds at its
    /// off-state for this long after the element transition
    /// starts before beginning to interpolate.
    #[serde(default)]
    pub delay_ms: u32,
    /// Track duration in milliseconds. The element's overall
    /// transition lasts at least `max(delay + duration)` across
    /// all tracks.
    #[serde(default = "default_track_duration_ms")]
    pub duration_ms: u32,
    /// Easing curve. Fade ignores `Spring` (the docstring on
    /// `Easing` notes that fade with a spring isn't useful);
    /// motion-based kinds (Translate/Rotate/Scale/Flip) accept
    /// all variants.
    #[serde(default)]
    pub easing: Easing,
}

fn default_track_duration_ms() -> u32 {
    250
}

impl Default for AnimationTrack {
    fn default() -> Self {
        AnimationTrack {
            kind: TrackKind::Fade { off_alpha: 0.0 },
            delay_ms: 0,
            duration_ms: default_track_duration_ms(),
            easing: Easing::EaseOut,
        }
    }
}

/// Direction discriminator. Enter applies `off_state → rest`;
/// Exit applies `rest → off_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionDirection {
    Enter,
    Exit,
}

/// Per-frame composite transform produced by evaluating all
/// tracks of a TransitionConfig at a given progress. Renderers
/// apply translate → rotate → scale → flip-scale around the
/// element centre, then multiply alpha into final fills.
#[derive(Debug, Clone, Copy)]
pub struct ComposedTransform {
    /// Multiplier on the element's natural alpha [0, 1].
    pub alpha: f32,
    /// Pixel translation, applied before rotation/scale.
    pub translate_x_px: f32,
    pub translate_y_px: f32,
    /// Rotation in radians around the element's centre.
    pub rotate_rad: f32,
    /// Scale multiplier (1.0 = identity).
    pub scale: f32,
    /// Flip-scale multiplier (1.0 = no flip, |cos(θ)| at angle
    /// θ) applied to `flip_axis` only — separate from the
    /// general `scale` so a Scale + Flip combo doesn't have to
    /// merge into a single number on the renderer side.
    pub flip_scale: f32,
    /// Axis along which `flip_scale` applies. Ignored when
    /// `flip_scale == 1.0`.
    pub flip_axis: Axis,
}

impl ComposedTransform {
    pub const IDENTITY: ComposedTransform = ComposedTransform {
        alpha: 1.0,
        translate_x_px: 0.0,
        translate_y_px: 0.0,
        rotate_rad: 0.0,
        scale: 1.0,
        flip_scale: 1.0,
        flip_axis: Axis::Y,
    };
}

// ============================================================================
// TransitionConfig — one direction (enter OR exit)
// ============================================================================

/// One direction of a transition. Lives inside an
/// `ElementAnimation` so each element has independent enter/exit
/// behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionConfig {
    /// What visual property the transition interpolates.
    #[serde(default)]
    pub kind: TransitionKind,

    /// Total duration of the transition, in milliseconds. Larger =
    /// slower / smoother. For `Spring` easing this is the upper
    /// bound — the spring may settle visibly before the duration
    /// elapses depending on stiffness / damping.
    #[serde(default = "default_duration_ms")]
    pub duration_ms: u32,

    /// Easing curve applied to normalised time. Defaults to
    /// `EaseOut` — the standard "snappy at the start, smooth at
    /// the end" curve used in most UI animations.
    #[serde(default)]
    pub easing: Easing,

    /// Pre-roll delay in milliseconds before the transition
    /// actually starts moving. 0 = immediate (default). Useful for
    /// sequencing two related elements without writing a full
    /// chain.
    #[serde(default)]
    pub delay_ms: u32,

    /// Starting scale for `Grow` / `GrowAndFade`. 0.0 = pop in
    /// from a single point, 1.0 = no scale change. Ignored for
    /// `None` and `Fade`.
    #[serde(default = "default_initial_scale")]
    pub initial_scale: f32,

    /// Starting opacity for `Fade` / `GrowAndFade`. 0.0 = fully
    /// invisible at the start of an enter (or end of an exit),
    /// 1.0 = no opacity change. Ignored for `None` and `Grow`.
    #[serde(default = "default_initial_opacity")]
    pub initial_opacity: f32,

    /// Final opacity reached by `Fade` / `GrowAndFade`. 1.0 =
    /// fully visible (the common case). Lowering it lets users
    /// peg an element to e.g. 80 % alpha at "fully open" — useful
    /// for the menu-background-opacity story.
    #[serde(default = "default_final_opacity")]
    pub final_opacity: f32,

    /// Custom animation tracks. When non-empty, takes precedence
    /// over the preset (`kind` + `initial_*` / `final_*`) — each
    /// track contributes one channel to the composed per-frame
    /// transform (alpha, translate, rotate, scale, flip), all
    /// running simultaneously on their own delay/duration/easing.
    /// Empty = use the preset path; the settings UI flips between
    /// the two via the "Customize" button.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_tracks: Vec<AnimationTrack>,
}

fn default_duration_ms() -> u32 {
    250
}
fn default_initial_scale() -> f32 {
    0.5
}
fn default_initial_opacity() -> f32 {
    0.0
}
fn default_final_opacity() -> f32 {
    1.0
}

impl Default for TransitionConfig {
    fn default() -> Self {
        TransitionConfig {
            kind: TransitionKind::default(),
            duration_ms: default_duration_ms(),
            easing: Easing::default(),
            delay_ms: 0,
            initial_scale: default_initial_scale(),
            initial_opacity: default_initial_opacity(),
            final_opacity: default_final_opacity(),
            custom_tracks: Vec::new(),
        }
    }
}

impl TransitionConfig {
    /// True when this config uses the custom-tracks path (i.e.
    /// the user clicked "Customize" and built a track list). The
    /// preset path is the default and what every existing config
    /// uses — back-compat is automatic.
    pub fn is_custom(&self) -> bool {
        !self.custom_tracks.is_empty()
    }

    /// Total duration the element should reserve for the
    /// transition. With tracks, that's the latest `delay +
    /// duration` across all tracks; otherwise the preset's
    /// configured `duration_ms`.
    pub fn effective_duration_ms(&self) -> u32 {
        if self.is_custom() {
            self.custom_tracks
                .iter()
                .map(|t| t.delay_ms + t.duration_ms)
                .max()
                .unwrap_or(0)
        } else {
            self.duration_ms
        }
    }

    /// Compose all tracks at the given `elapsed_ms` of the
    /// transition (clamped to `effective_duration_ms`). Returns
    /// the [`ComposedTransform`] the renderer should apply.
    /// `direction` decides whether each track interpolates
    /// `off_state → rest` (Enter) or `rest → off_state` (Exit).
    ///
    /// When `custom_tracks` is empty this is a no-op and the
    /// caller should fall back to the preset path
    /// (`crate::anim::evaluate` on the overlay side).
    pub fn evaluate_tracks(
        &self,
        elapsed_ms: f32,
        direction: TransitionDirection,
    ) -> ComposedTransform {
        let mut out = ComposedTransform::IDENTITY;
        if self.custom_tracks.is_empty() {
            return out;
        }
        for track in &self.custom_tracks {
            let delay = track.delay_ms as f32;
            let dur = track.duration_ms.max(1) as f32;
            let raw = ((elapsed_ms - delay) / dur).clamp(0.0, 1.0);
            let eased = track.easing.eval(raw);
            // For Enter: 0 = off-state, 1 = rest. For Exit: 0 = rest,
            // 1 = off-state. We compute `off_weight` = the lerp
            // factor toward off_state and let each kind use it.
            let off_weight = match direction {
                TransitionDirection::Enter => 1.0 - eased,
                TransitionDirection::Exit => eased,
            };
            apply_track(&mut out, &track.kind, off_weight);
        }
        out
    }
}

/// Apply one track's contribution to the running ComposedTransform.
/// `off_weight` ∈ [0, 1]: 1.0 = full off-state, 0.0 = at rest.
fn apply_track(out: &mut ComposedTransform, kind: &TrackKind, off_weight: f32) {
    match kind {
        TrackKind::Fade { off_alpha } => {
            // Lerp 1.0 (rest) → off_alpha by off_weight.
            let a = 1.0 + (off_alpha - 1.0) * off_weight;
            out.alpha *= a.clamp(0.0, 1.0);
        }
        TrackKind::Translate { axis, offset_px } => {
            let v = offset_px * off_weight;
            match axis {
                Axis::X => out.translate_x_px += v,
                Axis::Y => out.translate_y_px += v,
            }
        }
        TrackKind::Rotate { offset_deg } => {
            let v = offset_deg * off_weight;
            out.rotate_rad += v.to_radians();
        }
        TrackKind::Scale { offset_pct } => {
            // Lerp 100 (rest) → offset_pct by off_weight.
            let s_pct = 100.0 + (offset_pct - 100.0) * off_weight;
            out.scale *= (s_pct / 100.0).max(0.0);
        }
        TrackKind::Flip { axis, offset_deg } => {
            // Current angle (degrees from rest) lerped by off_weight.
            let angle_deg = offset_deg * off_weight;
            // 2D card-flip approximation: scale on the perpendicular
            // axis tracks |cos(angle)|. Modern shaders can replace
            // this with a real perspective projection; canvas can't
            // express that, so we settle for the foreshortening
            // shape only.
            let scale_axis = angle_deg.to_radians().cos().abs().max(0.0);
            out.flip_scale *= scale_axis;
            out.flip_axis = *axis;
        }
    }
}

impl TransitionConfig {
    /// "Instant snap" preset.
    pub fn instant() -> Self {
        TransitionConfig {
            kind: TransitionKind::None,
            duration_ms: 0,
            easing: Easing::Linear,
            ..Default::default()
        }
    }

    /// "Soft fade" preset — opacity-only, gentle ease-out.
    pub fn fade(duration_ms: u32) -> Self {
        TransitionConfig {
            kind: TransitionKind::Fade,
            duration_ms,
            easing: Easing::EaseOut,
            initial_opacity: 0.0,
            final_opacity: 1.0,
            ..Default::default()
        }
    }

    /// "Bouncy grow + fade" preset — Motion.dev's `scale` + `opacity`
    /// combo with a soft spring. Used as the default for the
    /// menu open + submenu pop-out.
    pub fn pop(duration_ms: u32) -> Self {
        TransitionConfig {
            kind: TransitionKind::GrowAndFade,
            duration_ms,
            easing: Easing::Spring {
                stiffness: 180.0,
                damping: 14.0,
            },
            initial_scale: 0.6,
            initial_opacity: 0.0,
            final_opacity: 1.0,
            ..Default::default()
        }
    }
}

// ============================================================================
// ChainConfig — per-item stagger for elements with multiple sub-pieces
// ============================================================================

/// Stagger config for elements whose enter / exit animates a
/// collection of sub-items in sequence (today: submenu sub-items).
/// Each sub-item starts its transition `stagger_ms` after the
/// previous one, giving a Motion.dev-style "ripple" effect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Delay between adjacent items, in milliseconds. 0 = all
    /// items animate simultaneously.
    #[serde(default = "default_stagger_ms")]
    pub stagger_ms: u32,
}

fn default_stagger_ms() -> u32 {
    35
}

impl Default for ChainConfig {
    fn default() -> Self {
        ChainConfig {
            stagger_ms: default_stagger_ms(),
        }
    }
}

// ============================================================================
// ElementAnimation — enter + exit + optional chain
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ElementAnimation {
    #[serde(default)]
    pub enter: TransitionConfig,
    #[serde(default)]
    pub exit: TransitionConfig,
    /// Per-item stagger for multi-item elements. None = items
    /// animate together (no stagger). Only meaningful for the
    /// submenu element today; harmless on others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain: Option<ChainConfig>,
}

impl ElementAnimation {
    /// Slice highlight — hover-in is a snappy linear fade (305 ms)
    /// so the hovered slice locks visually with the cursor; hover-out
    /// is a long ease-out tail (1229 ms with 214 ms pre-roll) so the
    /// previously-hovered slice gently dims rather than snapping
    /// off when the cursor crosses a boundary.
    pub fn slice_highlight_default() -> Self {
        ElementAnimation {
            enter: TransitionConfig {
                kind: TransitionKind::GrowAndFade,
                duration_ms: 305,
                easing: Easing::Linear,
                delay_ms: 0,
                initial_scale: 0.0,
                initial_opacity: 0.0,
                final_opacity: 1.0,
                custom_tracks: Vec::new(),
            },
            exit: TransitionConfig {
                kind: TransitionKind::GrowAndFade,
                duration_ms: 1229,
                easing: Easing::EaseOut,
                delay_ms: 214,
                initial_scale: 0.0,
                initial_opacity: 0.0,
                final_opacity: 1.0,
                custom_tracks: Vec::new(),
            },
            chain: None,
        }
    }

    /// Submenu pop-out — eased grow + fade for the open
    /// (initial_scale 0.47 keeps a visible pop without over-
    /// shooting) and a longer ease-in fade for the close so the
    /// dismiss feels deliberate rather than ripped away.
    pub fn submenu_default() -> Self {
        ElementAnimation {
            enter: TransitionConfig {
                kind: TransitionKind::GrowAndFade,
                duration_ms: 349,
                easing: Easing::EaseInOut,
                delay_ms: 0,
                initial_scale: 0.47,
                initial_opacity: 0.0,
                final_opacity: 1.0,
                custom_tracks: Vec::new(),
            },
            exit: TransitionConfig {
                kind: TransitionKind::Fade,
                duration_ms: 591,
                easing: Easing::EaseIn,
                delay_ms: 0,
                initial_scale: 0.5,
                initial_opacity: 0.0,
                final_opacity: 1.0,
                custom_tracks: Vec::new(),
            },
            chain: Some(ChainConfig { stagger_ms: 32 }),
        }
    }

    /// Whole-menu open / close — soft spring on open (137/13.5 is
    /// gentle without feeling sluggish) and an ease-in-out fade
    /// on close. final_opacity 0.9 leaves a touch of transparency
    /// so the desktop reads through the wedge fills.
    pub fn menu_default() -> Self {
        ElementAnimation {
            enter: TransitionConfig {
                kind: TransitionKind::GrowAndFade,
                duration_ms: 826,
                easing: Easing::Spring {
                    stiffness: 137.0,
                    damping: 13.5,
                },
                delay_ms: 0,
                initial_scale: 0.0,
                initial_opacity: 0.0,
                final_opacity: 0.9,
                custom_tracks: Vec::new(),
            },
            exit: TransitionConfig {
                kind: TransitionKind::GrowAndFade,
                duration_ms: 617,
                easing: Easing::EaseInOut,
                delay_ms: 0,
                initial_scale: 0.0,
                initial_opacity: 0.0,
                final_opacity: 0.9,
                custom_tracks: Vec::new(),
            },
            chain: None,
        }
    }
}

// ============================================================================
// AnimationConfig — top-level
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationConfig {
    #[serde(default = "ElementAnimation::menu_default")]
    pub menu: ElementAnimation,
    #[serde(default = "ElementAnimation::submenu_default")]
    pub submenu: ElementAnimation,
    #[serde(default = "ElementAnimation::slice_highlight_default")]
    pub slice_highlight: ElementAnimation,
    /// Animation played when the user cycles to a different
    /// radial-menu page via the scroll wheel over the centre puck.
    /// Independent of `menu` (which covers the whole-overlay
    /// open/close) and `submenu` (which covers in-page sub-item
    /// pop-outs) because the page-cycle gesture neither opens nor
    /// closes the overlay — the ring contents swap in place.
    #[serde(default = "PageTransitionConfig::default")]
    pub page_transition: PageTransitionConfig,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        AnimationConfig {
            menu: ElementAnimation::menu_default(),
            submenu: ElementAnimation::submenu_default(),
            slice_highlight: ElementAnimation::slice_highlight_default(),
            page_transition: PageTransitionConfig::default(),
        }
    }
}

// ============================================================================
// PageTransitionConfig — animates the scroll-wheel page cycle
// ============================================================================

/// What kind of motion accompanies a page swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageTransitionStyle {
    /// No animation — slices swap instantly the moment the wheel
    /// fires. Cheapest, no risk of disorientation.
    None,
    /// Old slices fade out + shrink ~10 %, new ones fade in + grow
    /// back. Subtle, fast, no rotation.
    CrossfadeScale,
    /// Whole ring rotates ~half a slice in the scroll direction
    /// while crossfading to the new page. Reads as "pages spinning
    /// past" — the most physical of the styles.
    SpinCrossfade,
    /// No slice motion: the centre puck briefly enlarges + flashes
    /// the new accent colour while slices swap behind it. Minimal,
    /// fastest visible feedback.
    CenterPulse,
    /// GPU shader: noise-driven dissolve flash that sweeps across
    /// the menu during the transition. Slices still crossfade
    /// underneath; the shader adds a particle-like visual layer.
    Dissolve,
    /// GPU shader: animated plasma waves wash over the menu
    /// during the transition. Most theatrical of the styles —
    /// reads as the menu briefly entering a "warp".
    Plasma,
    /// Card-flip around an axis. Outgoing ring rotates 0° → 90°
    /// (vanishing edge-on at the midpoint); incoming ring
    /// rotates 90° → 0° (entering edge-on, settling at the
    /// front). 2D approximation via |cos(angle)| scale on the
    /// axis perpendicular to the flip — same trick as the Flip
    /// custom-track kind. Defaults to a horizontal flip (Y axis
    /// of rotation, X scale collapses).
    Flip,
}

impl Default for PageTransitionStyle {
    fn default() -> Self {
        PageTransitionStyle::SpinCrossfade
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageTransitionConfig {
    /// Visual style of the swap. See [`PageTransitionStyle`].
    #[serde(default)]
    pub style: PageTransitionStyle,

    /// Total duration of the transition, in milliseconds. The
    /// spin/crossfade default lands ~220 ms; tighten for snappier
    /// flicks, lengthen for a more deliberate feel.
    #[serde(default = "default_page_transition_duration_ms")]
    pub duration_ms: u32,

    /// Easing curve applied to the [0, 1] transition progress.
    /// Defaults to `EaseOut` so the bulk of the motion happens
    /// early — the new page lands quickly and settles smoothly.
    #[serde(default = "default_page_transition_easing")]
    pub easing: Easing,

    /// Maximum rotation, in degrees, applied to the ring during a
    /// `SpinCrossfade` transition. Sign is flipped to follow scroll
    /// direction. ~22.5° (half a slice at the default 8-slice
    /// layout) feels physical without being disorienting. Ignored
    /// for non-spin styles.
    #[serde(default = "default_page_transition_rotation_deg")]
    pub rotation_deg: f32,

    /// Minimum gap between scroll-wheel page cycles, in
    /// milliseconds. Hi-res scroll wheels (MX Master 4) emit many
    /// tick events per physical click, so without a debounce a
    /// single light flick can advance several pages. 0 disables
    /// the debounce; sensible range 100-500.
    #[serde(default = "default_page_cycle_debounce_ms")]
    pub cycle_debounce_ms: u32,

    /// Shader overlay configuration. Runs *on top of* whatever
    /// `style` is doing at the canvas level — pick e.g.
    /// `style: SpinCrossfade` together with
    /// `shader.style: Plasma` to layer a spin animation under a
    /// plasma flourish. When `style` is set to one of the legacy
    /// shader-only variants (`Dissolve` / `Plasma`) and
    /// `shader.style` is `None`, the renderer falls back to
    /// running that legacy shader so older configs keep working
    /// unchanged.
    #[serde(default)]
    pub shader: PageTransitionShaderConfig,

    /// Custom track-based animation for the canvas-side
    /// page-cycle. Behaves like any other ElementAnimation: when
    /// `enter.custom_tracks` or `exit.custom_tracks` is non-empty
    /// the renderer evaluates those instead of the preset
    /// `style` field. The OUTGOING ring uses Exit, the INCOMING
    /// ring uses Enter — both come from the same page_transition
    /// tween so their timing stays locked. Empty default = use
    /// the preset path (back-compat with existing configs).
    #[serde(default)]
    pub animation: ElementAnimation,
}

fn default_page_transition_duration_ms() -> u32 {
    493
}
fn default_page_transition_easing() -> Easing {
    Easing::EaseInOut
}
fn default_page_transition_rotation_deg() -> f32 {
    90.0
}
fn default_page_cycle_debounce_ms() -> u32 {
    250
}

impl Default for PageTransitionConfig {
    fn default() -> Self {
        PageTransitionConfig {
            style: PageTransitionStyle::default(),
            duration_ms: default_page_transition_duration_ms(),
            easing: default_page_transition_easing(),
            rotation_deg: default_page_transition_rotation_deg(),
            cycle_debounce_ms: default_page_cycle_debounce_ms(),
            shader: PageTransitionShaderConfig::default(),
            animation: ElementAnimation::default(),
        }
    }
}

// ============================================================================
// PageTransitionShaderConfig — orthogonal GPU overlay
// ============================================================================

/// Selectable look for the page-transition shader overlay. `None`
/// = no shader pass (the canvas-side `style` still runs). The
/// other variants pick a fragment-shader branch in
/// `overlay-rs/src/render/page_fx.wgsl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageTransitionShaderStyle {
    /// No shader overlay. The canvas-only styles
    /// (`CrossfadeScale`, `SpinCrossfade`, `CenterPulse`,
    /// `None`) run on their own.
    None,
    /// Animated noise-mask flash. Reads as "pixels scattering"
    /// across the disc.
    Dissolve,
    /// Sin/cos plasma waves washing across the menu, phase
    /// driven by transition progress. Theatrical / warp feel.
    Plasma,
}

impl Default for PageTransitionShaderStyle {
    fn default() -> Self {
        PageTransitionShaderStyle::None
    }
}

/// Knobs that drive the page-transition shader. Live-reloaded
/// from `~/.config/juhradial/config.json` so users can tune them
/// without restarting the overlay.
///
/// All numeric ranges below are conservative — extreme values
/// don't crash the shader, but the visual quality drops off
/// sharply outside them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageTransitionShaderConfig {
    /// Which fragment-branch to take. `None` skips the shader
    /// pass entirely (zero GPU cost).
    #[serde(default)]
    pub style: PageTransitionShaderStyle,

    /// Strength multiplier 0..=1. Multiplied into the final
    /// alpha. `0` is functionally identical to `style: None`
    /// but the layer is still constructed; prefer `style:
    /// none` for the off case so the shader pipeline isn't
    /// invoked at all.
    #[serde(default = "default_page_shader_intensity")]
    pub intensity: f32,

    /// Dissolve-only — UV multiplier feeding the noise field.
    /// Higher = finer grain (more particles per pixel); lower
    /// = coarser blobs. Typical 4..=16, default 8.
    #[serde(default = "default_dissolve_noise_scale")]
    pub dissolve_noise_scale: f32,

    /// Dissolve-only — softness of the threshold band. Higher
    /// = wider, blurrier dissolve front; lower = harder edge
    /// between dissolved and intact pixels. Typical 0.05..=0.5.
    #[serde(default = "default_dissolve_band_softness")]
    pub dissolve_band_softness: f32,

    /// Plasma-only — UV multiplier for the wave field. Higher
    /// = more waves per pixel; lower = fewer, broader waves.
    /// Typical 3..=12, default 6.
    #[serde(default = "default_plasma_wave_scale")]
    pub plasma_wave_scale: f32,

    /// Plasma-only — phase-progression multiplier. Higher =
    /// the plasma "boils" faster within the transition window;
    /// lower = slower swirl. Typical 0.5..=2.5, default 1.0.
    #[serde(default = "default_plasma_wave_speed")]
    pub plasma_wave_speed: f32,
}

fn default_page_shader_intensity() -> f32 {
    1.0
}
fn default_dissolve_noise_scale() -> f32 {
    8.0
}
fn default_dissolve_band_softness() -> f32 {
    0.25
}
fn default_plasma_wave_scale() -> f32 {
    6.0
}
fn default_plasma_wave_speed() -> f32 {
    1.0
}

impl Default for PageTransitionShaderConfig {
    fn default() -> Self {
        PageTransitionShaderConfig {
            style: PageTransitionShaderStyle::default(),
            intensity: default_page_shader_intensity(),
            dissolve_noise_scale: default_dissolve_noise_scale(),
            dissolve_band_softness: default_dissolve_band_softness(),
            plasma_wave_scale: default_plasma_wave_scale(),
            plasma_wave_speed: default_plasma_wave_speed(),
        }
    }
}

impl PageTransitionShaderConfig {
    /// Resolve the *effective* shader style for a given
    /// `PageTransitionConfig`. Lets the legacy `style: Dissolve
    /// | Plasma` configs continue to drive the shader without
    /// duplicating the variant in `shader.style`. Explicit
    /// `shader.style` always wins so users can layer e.g.
    /// `style: SpinCrossfade` + `shader.style: Plasma`.
    pub fn effective_style(
        &self,
        canvas_style: PageTransitionStyle,
    ) -> PageTransitionShaderStyle {
        if !matches!(self.style, PageTransitionShaderStyle::None) {
            return self.style;
        }
        match canvas_style {
            PageTransitionStyle::Dissolve => PageTransitionShaderStyle::Dissolve,
            PageTransitionStyle::Plasma => PageTransitionShaderStyle::Plasma,
            _ => PageTransitionShaderStyle::None,
        }
    }
}

impl PageTransitionConfig {
    /// Equivalent `TransitionConfig` so the existing `Tween` plumbing
    /// (which is parameterised by `TransitionConfig`) can drive the
    /// page-transition progress without a new tween type. Kind is
    /// always `Fade` since `progress` is the unit-time scalar — the
    /// renderer reads `style` separately to decide how to apply it.
    pub fn as_transition_config(&self) -> TransitionConfig {
        TransitionConfig {
            kind: TransitionKind::Fade,
            duration_ms: self.duration_ms,
            easing: self.easing,
            delay_ms: 0,
            initial_scale: 1.0,
            initial_opacity: 0.0,
            final_opacity: 1.0,
            custom_tracks: Vec::new(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_object_picks_up_defaults() {
        let cfg: AnimationConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.submenu.enter.kind, TransitionKind::GrowAndFade);
        assert_eq!(cfg.menu.enter.kind, TransitionKind::GrowAndFade);
        assert_eq!(
            cfg.slice_highlight.enter.kind,
            TransitionKind::GrowAndFade
        );
    }

    #[test]
    fn user_can_pick_spring_easing() {
        let json = r#"{
            "menu": {
                "enter": {
                    "kind": "grow_and_fade",
                    "duration_ms": 500,
                    "easing": { "type": "spring", "stiffness": 200, "damping": 8 }
                }
            }
        }"#;
        let cfg: AnimationConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.menu.enter.duration_ms, 500);
        match cfg.menu.enter.easing {
            Easing::Spring { stiffness, damping } => {
                assert!((stiffness - 200.0).abs() < 1e-4);
                assert!((damping - 8.0).abs() < 1e-4);
            }
            other => panic!("expected spring, got {other:?}"),
        }
    }

    #[test]
    fn easing_curves_pass_through_endpoints() {
        for e in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
        ] {
            assert!(e.eval(0.0).abs() < 1e-4, "{e:?} should be 0 at t=0");
            assert!((e.eval(1.0) - 1.0).abs() < 1e-4, "{e:?} should be 1 at t=1");
        }
    }

    #[test]
    fn underdamped_spring_overshoots_above_one() {
        let s = Easing::Spring {
            stiffness: 250.0,
            damping: 6.0,
        };
        let mut peak = 0.0_f32;
        for i in 0..=100 {
            let v = s.eval(i as f32 / 100.0);
            if v > peak {
                peak = v;
            }
        }
        assert!(peak > 1.05, "expected overshoot > 1.05, got {peak}");
    }

    #[test]
    fn critically_damped_spring_does_not_overshoot() {
        let s = Easing::Spring {
            stiffness: 100.0,
            damping: 20.0, // ζ = 1 → critical
        };
        for i in 0..=100 {
            let v = s.eval(i as f32 / 100.0);
            assert!(v <= 1.001, "critically damped should not overshoot, got {v}");
        }
    }

    #[test]
    fn chain_config_round_trips() {
        let json = r#"{ "submenu": { "chain": { "stagger_ms": 80 } } }"#;
        let cfg: AnimationConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.submenu.chain.unwrap().stagger_ms, 80);
    }
}
