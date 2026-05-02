//! User-tweakable animation parameters for the radial overlay.
//!
//! Each animated *element* (the menu itself, the submenu pop-out, a
//! slice's hover highlight) carries a separate enter/exit transition
//! pair. Each transition picks a `kind` (the visual effect — fade,
//! grow, grow-with-bounce, none) and a small set of timing/shape
//! knobs the user can edit live in `~/.config/juhradial/config.json`
//! and pick up via the overlay's inotify watcher.
//!
//! All fields default to values that reproduce today's hardcoded
//! behaviour, so existing configs without an `animation` block render
//! identically. Adding the block in the editor (or by hand) is what
//! gives the user the tweakability.

use serde::{Deserialize, Serialize};

/// Visual style of a transition. Tells the renderer *what* to
/// interpolate (scale, opacity, both) and *how* (smooth vs. springy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    /// No animation — element snaps straight to the target state.
    None,
    /// Opacity-only fade. Element starts at `initial_opacity` and
    /// blends to fully visible (enter) or back down (exit).
    Fade,
    /// Smooth scale-up from `initial_scale` to 1.0, with opacity
    /// matching. Standard "popup" feel.
    Grow,
    /// Scale-up with overshoot — the spring/elastic style. The
    /// `bounce` knob in `TransitionConfig` controls how far past
    /// 1.0 the scale goes before settling.
    GrowBounce,
}

impl Default for TransitionKind {
    /// Default to `None` (instant snap) so any field a user
    /// *omits* from a partial config block doesn't accidentally
    /// introduce an animation they didn't ask for. The
    /// element-level "recommended" presets
    /// (`ElementAnimation::*_default`) are what you get when the
    /// whole `animation` block is missing — those are deliberate
    /// nice defaults; per-field omission is "stay still".
    fn default() -> Self {
        TransitionKind::None
    }
}

/// One direction of a transition (enter OR exit). Lives inside an
/// `ElementAnimation` so each element has independent enter/exit
/// behaviour — e.g. a snappy bouncy entry plus a quick linear fade
/// out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionConfig {
    /// What kind of visual effect this is.
    #[serde(default)]
    pub kind: TransitionKind,

    /// Per-frame interpolation factor for the underlying tween,
    /// applied at 60 Hz. Larger = snappier. 0.25 ≈ ~6 frames to
    /// settle (the legacy default), 0.10 ≈ slow & smooth, 0.50 ≈
    /// near-instant. Range is open but values outside (0.01, 0.95]
    /// will look pathological.
    #[serde(default = "default_speed")]
    pub speed: f32,

    /// Bounce strength for `GrowBounce` — how far past 1.0 the
    /// scale overshoots before settling. 0.0 = no overshoot,
    /// 1.70 = Penner's classic ease-out-back, 3.0 = exaggerated
    /// rubbery feel. Ignored for other kinds.
    #[serde(default = "default_bounce")]
    pub bounce: f32,

    /// Starting scale for `Grow` and `GrowBounce`. 0.0 = pop in
    /// from a single point, 1.0 = no growth (only opacity moves).
    /// Ignored for `None` and `Fade`.
    #[serde(default = "default_initial_scale")]
    pub initial_scale: f32,

    /// Starting opacity for `Fade`, `Grow`, and `GrowBounce`.
    /// 0.0 = invisible at the start of the enter (or end of the
    /// exit), 1.0 = always fully opaque (so opacity never
    /// changes — useful for a "scale only" feel).
    #[serde(default = "default_initial_opacity")]
    pub initial_opacity: f32,
}

fn default_speed() -> f32 {
    0.25
}
fn default_bounce() -> f32 {
    1.70158
}
fn default_initial_scale() -> f32 {
    0.5
}
fn default_initial_opacity() -> f32 {
    0.0
}

impl Default for TransitionConfig {
    fn default() -> Self {
        TransitionConfig {
            kind: TransitionKind::default(),
            speed: default_speed(),
            bounce: default_bounce(),
            initial_scale: default_initial_scale(),
            initial_opacity: default_initial_opacity(),
        }
    }
}

impl TransitionConfig {
    /// "Linear / snappy" preset — instant snap, no animation.
    pub fn instant() -> Self {
        TransitionConfig {
            kind: TransitionKind::None,
            speed: 1.0,
            ..Default::default()
        }
    }

    /// "Soft fade" preset — opacity-only, gentle.
    pub fn fade() -> Self {
        TransitionConfig {
            kind: TransitionKind::Fade,
            speed: 0.20,
            initial_opacity: 0.0,
            ..Default::default()
        }
    }
}

/// Enter + exit pair for one animated element.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ElementAnimation {
    #[serde(default)]
    pub enter: TransitionConfig,
    #[serde(default)]
    pub exit: TransitionConfig,
}

impl ElementAnimation {
    /// Slice highlight — opacity only, fast in, fast out. Used by
    /// the hover glow and the focused-slice halo. Defaults match
    /// the legacy hardcoded `Animation::step` (0.25 step factor).
    pub fn slice_highlight_default() -> Self {
        ElementAnimation {
            enter: TransitionConfig::fade(),
            exit: TransitionConfig::fade(),
        }
    }

    /// Submenu pop-out — bouncy grow on enter, gentle shrink on
    /// exit. Matches today's `ease_out_back` behaviour at the
    /// user level.
    pub fn submenu_default() -> Self {
        ElementAnimation {
            enter: TransitionConfig {
                kind: TransitionKind::GrowBounce,
                speed: 0.18,
                bounce: 1.70158,
                initial_scale: 0.5,
                initial_opacity: 0.0,
            },
            exit: TransitionConfig {
                kind: TransitionKind::Fade,
                speed: 0.30,
                initial_opacity: 0.0,
                ..Default::default()
            },
        }
    }

    /// Whole-menu open / close. Defaults to instant — preserves
    /// today's "pops in immediately on Show" feel until the user
    /// dials in something they like.
    pub fn menu_default() -> Self {
        ElementAnimation {
            enter: TransitionConfig::instant(),
            exit: TransitionConfig::instant(),
        }
    }
}

/// Top-level animation block — one `ElementAnimation` per animated
/// element. New elements add a field here with a default-getter so
/// older configs deserialize cleanly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationConfig {
    /// Whole-menu open/close.
    #[serde(default = "ElementAnimation::menu_default")]
    pub menu: ElementAnimation,
    /// Submenu pop-out (per-Submenu-slice arc).
    #[serde(default = "ElementAnimation::submenu_default")]
    pub submenu: ElementAnimation,
    /// Per-slice hover highlight.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_object_picks_up_defaults() {
        let cfg: AnimationConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.submenu.enter.kind, TransitionKind::GrowBounce);
        assert_eq!(cfg.menu.enter.kind, TransitionKind::None);
    }

    #[test]
    fn user_can_pick_fade_for_menu_open() {
        let json = r#"{ "menu": { "enter": { "kind": "fade", "speed": 0.20 } } }"#;
        let cfg: AnimationConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.menu.enter.kind, TransitionKind::Fade);
        assert!((cfg.menu.enter.speed - 0.20).abs() < 1e-4);
        // Exit untouched — still default (instant).
        assert_eq!(cfg.menu.exit.kind, TransitionKind::None);
    }

    #[test]
    fn unknown_kind_fails_gracefully() {
        let json = r#"{ "menu": { "enter": { "kind": "warp_speed" } } }"#;
        assert!(serde_json::from_str::<AnimationConfig>(json).is_err());
    }
}
