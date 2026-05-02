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
    /// Slice highlight — quick fade in / out of the hover glow.
    pub fn slice_highlight_default() -> Self {
        ElementAnimation {
            enter: TransitionConfig::fade(120),
            exit: TransitionConfig::fade(120),
            chain: None,
        }
    }

    /// Submenu pop-out — bouncy scale + fade with per-item stagger.
    pub fn submenu_default() -> Self {
        ElementAnimation {
            enter: TransitionConfig::pop(350),
            exit: TransitionConfig {
                kind: TransitionKind::Fade,
                duration_ms: 180,
                easing: Easing::EaseIn,
                initial_opacity: 0.0,
                final_opacity: 1.0,
                ..Default::default()
            },
            chain: Some(ChainConfig::default()),
        }
    }

    /// Whole-menu open / close — small bouncy scale on open, quick
    /// fade out on close.
    pub fn menu_default() -> Self {
        ElementAnimation {
            enter: TransitionConfig::pop(280),
            exit: TransitionConfig {
                kind: TransitionKind::Fade,
                duration_ms: 140,
                easing: Easing::EaseIn,
                initial_opacity: 0.0,
                final_opacity: 1.0,
                ..Default::default()
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
}

impl Default for AnimationConfig {
    fn default() -> Self {
        AnimationConfig {
            menu: ElementAnimation::menu_default(),
            submenu: ElementAnimation::submenu_default(),
            slice_highlight: ElementAnimation::slice_highlight_default(),
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
        assert_eq!(cfg.slice_highlight.enter.kind, TransitionKind::Fade);
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
