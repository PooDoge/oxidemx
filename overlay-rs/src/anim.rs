//! Runtime animation primitives for the radial overlay.
//!
//! `Tween` is a single 0..1 progress value with a target and a
//! per-tick speed factor. The shared frame ticker (60 Hz from
//! `iced::time::every`) drives `step()` on every tween every frame;
//! when current ≈ target the tween is idle and step() is cheap.
//!
//! Visual *interpretation* of the progress value (turn it into an
//! opacity, a scale, a bouncy overshoot scale) lives in
//! `apply_to_*` helpers below — keeps the renderers free of the
//! easing math, which makes adding new transition kinds a one-place
//! change.
//!
//! All tween parameters are owned by the caller; `Tween` itself is
//! deliberately tiny + Copy so it can sit inline in animation
//! state without forcing borrows. Speed is replaced on each
//! `set_target` so an enter speed and an exit speed can coexist
//! without separate fields.

use juhradial_shared::{TransitionConfig, TransitionKind};

/// Single animated scalar in [0.0, 1.0]. The current value chases
/// `target` geometrically — each tick covers `speed` of the
/// remaining distance, which gives a clean ease-out feel without
/// any per-frame state machinery. Values outside [0,1] are valid
/// but only the [0,1] range is interpreted by the renderer.
#[derive(Debug, Clone, Copy)]
pub struct Tween {
    pub current: f32,
    pub target: f32,
    pub speed: f32,
}

impl Tween {
    /// New tween parked at `value` with no animation in flight.
    pub const fn at(value: f32) -> Self {
        Tween { current: value, target: value, speed: 0.25 }
    }

    /// Move one frame closer to the target. Cheap when idle; the
    /// abs-delta check snaps to target and short-circuits.
    pub fn step(&mut self) {
        let delta = self.target - self.current;
        if delta.abs() < 0.001 {
            self.current = self.target;
            return;
        }
        let speed = self.speed.clamp(0.001, 1.0);
        self.current += delta * speed;
    }

    /// Set a new target and the speed at which to chase it. The
    /// caller passes the relevant enter-or-exit `TransitionConfig`
    /// so the speed reflects the user's tweaks.
    pub fn set_target(&mut self, target: f32, cfg: &TransitionConfig) {
        self.target = target;
        // `None` kind means "snap" — we honour it by maxing the
        // speed so the next step() jumps to the target.
        self.speed = if matches!(cfg.kind, TransitionKind::None) {
            1.0
        } else {
            cfg.speed
        };
    }

    /// True when the tween has reached its target (within a
    /// pixel-imperceptible epsilon). Used by the menu lifecycle to
    /// know when an exit-fade has finished and the window can stop
    /// painting.
    pub fn is_idle(&self) -> bool {
        (self.target - self.current).abs() < 0.001
    }
}

/// Output of a tween-driven render: the raw progress, what scale
/// to apply, and what opacity to apply, all derived from the tween
/// + a `TransitionConfig` that says "this is a fade" / "this is a
/// growbounce" / etc. Renderers hold one of these per element and
/// just multiply / blend with it.
#[derive(Debug, Clone, Copy)]
pub struct Visual {
    /// 0..1 (or slightly past 1 for GrowBounce overshoot). Renders
    /// for transient effects that scale linearly with progress.
    pub progress: f32,
    /// Multiplier on the element's natural size, in [0, ~1.1].
    pub scale: f32,
    /// Multiplier on the element's natural alpha, in [0, 1].
    pub opacity: f32,
}

impl Visual {
    pub const VISIBLE: Visual = Visual { progress: 1.0, scale: 1.0, opacity: 1.0 };
    pub const HIDDEN: Visual = Visual { progress: 0.0, scale: 0.0, opacity: 0.0 };
}

/// Translate a tween's current value plus the active enter/exit
/// transition into per-frame `Visual` parameters. Direction is
/// inferred from the tween: if it's heading toward 1 (or already
/// fully visible), apply the enter kind; if heading toward 0,
/// apply the exit kind.
pub fn evaluate(tween: &Tween, enter: &TransitionConfig, exit: &TransitionConfig) -> Visual {
    let going_in = tween.target >= tween.current;
    let cfg = if going_in { enter } else { exit };
    let p = tween.current.clamp(0.0, 1.0);

    // Curve: convert linear progress into the visual progress used
    // for scale / opacity. Each kind picks its own.
    let curved = match cfg.kind {
        TransitionKind::None => {
            if p > 0.5 {
                1.0
            } else {
                0.0
            }
        }
        TransitionKind::Fade => p, // linear opacity ramp
        TransitionKind::Grow => ease_out_quad(p),
        TransitionKind::GrowBounce => ease_out_back(p, cfg.bounce),
    };

    let (scale, opacity) = match cfg.kind {
        TransitionKind::None => (1.0, 1.0),
        TransitionKind::Fade => {
            let op = lerp(cfg.initial_opacity, 1.0, curved);
            (1.0, op)
        }
        TransitionKind::Grow | TransitionKind::GrowBounce => {
            let sc = lerp(cfg.initial_scale, 1.0, curved);
            // Opacity rides the same curve but clamped to [0,1] so
            // a bounce overshoot doesn't blow out the alpha.
            let op = lerp(cfg.initial_opacity, 1.0, curved.clamp(0.0, 1.0));
            (sc, op)
        }
    };

    Visual { progress: p, scale, opacity }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn ease_out_quad(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// Penner's ease-out-back, with a configurable overshoot. Default
/// overshoot of ~1.70158 matches the canonical formula.
fn ease_out_back(t: f32, overshoot: f32) -> f32 {
    let c1 = overshoot.max(0.0);
    let c3 = c1 + 1.0;
    let p = t - 1.0;
    1.0 + c3 * p * p * p + c1 * p * p
}

#[cfg(test)]
mod tests {
    use super::*;
    use juhradial_shared::{TransitionConfig, TransitionKind};

    #[test]
    fn idle_tween_doesnt_move() {
        let mut t = Tween::at(0.5);
        let before = t.current;
        for _ in 0..10 {
            t.step();
        }
        assert!((t.current - before).abs() < 1e-4);
    }

    #[test]
    fn enter_then_exit_uses_correct_speed() {
        let mut t = Tween::at(0.0);
        let enter = TransitionConfig {
            kind: TransitionKind::Fade,
            speed: 0.5,
            ..Default::default()
        };
        let exit = TransitionConfig {
            kind: TransitionKind::Fade,
            speed: 0.1,
            ..Default::default()
        };
        t.set_target(1.0, &enter);
        assert!((t.speed - 0.5).abs() < 1e-4);
        t.step();
        assert!(t.current > 0.0);
        t.set_target(0.0, &exit);
        assert!((t.speed - 0.1).abs() < 1e-4);
    }

    #[test]
    fn none_kind_snaps_to_target() {
        let mut t = Tween::at(0.0);
        let cfg = TransitionConfig {
            kind: TransitionKind::None,
            ..Default::default()
        };
        t.set_target(1.0, &cfg);
        t.step();
        assert!((t.current - 1.0).abs() < 1e-4);
    }

    #[test]
    fn evaluate_fade_at_half_is_half_opacity() {
        let mut t = Tween::at(0.5);
        t.target = 1.0; // entering
        let cfg = TransitionConfig {
            kind: TransitionKind::Fade,
            initial_opacity: 0.0,
            ..Default::default()
        };
        let v = evaluate(&t, &cfg, &cfg);
        assert!((v.opacity - 0.5).abs() < 1e-4);
        assert!((v.scale - 1.0).abs() < 1e-4);
    }

    #[test]
    fn evaluate_grow_bounce_overshoots_above_1() {
        // At ~75 % of the way through a grow-bounce, scale should
        // exceed 1.0 (the bounce peak).
        let cfg = TransitionConfig {
            kind: TransitionKind::GrowBounce,
            bounce: 1.70158,
            initial_scale: 0.0,
            initial_opacity: 0.0,
            ..Default::default()
        };
        let mut peak = 0.0_f32;
        for i in 0..=20 {
            let mut t = Tween::at(i as f32 / 20.0);
            t.target = 1.0;
            let v = evaluate(&t, &cfg, &cfg);
            if v.scale > peak {
                peak = v.scale;
            }
        }
        assert!(peak > 1.0, "expected overshoot, got peak={peak}");
    }
}
