//! Runtime animation primitives for the radial overlay.
//!
//! Time-based, Motion.dev-style: each `Tween` represents a single
//! transition from a `start` value to an `end` value over a
//! configured `duration_ms`, optionally preceded by a `delay_ms`,
//! shaped by an `Easing` curve. The frame ticker drives `step(dt_ms)`
//! every frame; once `elapsed_ms >= delay_ms + duration_ms` the tween
//! is *idle* and step() short-circuits.
//!
//! Direction reversal mid-flight (e.g. user hovers a slice, animation
//! is partway through, then hovers off) is handled by `set_target`
//! which captures the *current eased value* as the new `start` and
//! resets the elapsed clock — so reversals look smooth instead of
//! snapping to 0/1 first.
//!
//! Per-item chain support (`item_progress`) lets a single master
//! tween drive a sequence of sub-items each starting `stagger_ms`
//! later than the previous — used by the submenu sub-item arc.

use juhradial_shared::{ChainConfig, Easing, TransitionConfig, TransitionKind};

/// Single time-based transition. The `current` value is computed
/// from `(elapsed_ms, duration_ms, easing, start, end)` and updated
/// in place by `step()` every frame.
#[derive(Debug, Clone, Copy)]
pub struct Tween {
    /// Last computed eased value, in the [start, end] range.
    /// Renderers read this directly.
    pub current: f32,
    /// Transition target (0.0 or 1.0 for menu/submenu/highlight).
    /// Compared against `start` to determine direction.
    pub target: f32,
    /// Where the current transition began. Captured at each
    /// `set_target` so reversal-from-mid-flight stays smooth.
    start: f32,
    /// Where the current transition ends — same as `target` once
    /// settled, but stored separately so step() doesn't have to
    /// distinguish "in flight" from "settled".
    end: f32,
    /// Time elapsed since the current transition started, in ms.
    /// Includes the pre-roll delay.
    elapsed_ms: f32,
    /// Pre-roll delay in ms. The tween holds at `start` for this
    /// long after `set_target` before beginning to interpolate.
    delay_ms: f32,
    /// Duration of the current transition, in ms (excludes the
    /// pre-roll delay).
    duration_ms: f32,
    /// Easing curve applied to normalised time within the duration.
    easing: Easing,
}

impl Tween {
    /// Park a new tween at `value` with no transition in flight.
    pub const fn at(value: f32) -> Self {
        Tween {
            current: value,
            target: value,
            start: value,
            end: value,
            elapsed_ms: 0.0,
            delay_ms: 0.0,
            duration_ms: 0.0,
            easing: Easing::Linear,
        }
    }

    /// Begin a new transition toward `target`. The tween captures
    /// the current eased value as the new `start` so reversals
    /// from mid-flight look continuous instead of snapping. Pulls
    /// `duration_ms` / `easing` / `delay_ms` from `cfg`. When
    /// `cfg.kind == None` the transition completes instantly.
    pub fn set_target(&mut self, target: f32, cfg: &TransitionConfig) {
        self.start = self.current;
        self.end = target;
        self.target = target;
        self.elapsed_ms = 0.0;
        self.delay_ms = cfg.delay_ms as f32;
        self.duration_ms = cfg.duration_ms.max(1) as f32;
        self.easing = cfg.easing;
        if matches!(cfg.kind, TransitionKind::None) {
            // Snap immediately — no interpolation, no delay.
            self.current = target;
            self.start = target;
            self.elapsed_ms = self.delay_ms + self.duration_ms;
        }
    }

    /// Advance the tween by `dt_ms` of real time. Cheap when the
    /// transition has already settled.
    pub fn step(&mut self, dt_ms: f32) {
        if self.is_idle() {
            return;
        }
        self.elapsed_ms += dt_ms;
        let t_after_delay = (self.elapsed_ms - self.delay_ms).max(0.0);
        let t_norm = (t_after_delay / self.duration_ms.max(1.0)).clamp(0.0, 1.0);
        let eased = self.easing.eval(t_norm);
        self.current = self.start + (self.end - self.start) * eased;
        if t_norm >= 1.0 {
            // Spring overshoot can leave `current` slightly past
            // `end`; clamp to the target on settle so downstream
            // hit-tests match what the user sees.
            self.current = self.end;
        }
    }

    /// True once the transition has settled (elapsed has covered
    /// delay + duration).
    pub fn is_idle(&self) -> bool {
        self.elapsed_ms >= self.delay_ms + self.duration_ms
    }

    /// Compute the [0,1] *raw* progress of an item that started
    /// `item_offset_ms` after this tween's transition began. Used
    /// by chain renderers (e.g. submenu sub-items) to derive a
    /// per-item time without needing a tween per item. Returns
    /// the raw normalised time *not* the eased value — apply
    /// `easing` separately if needed.
    pub fn item_progress(&self, item_offset_ms: f32) -> f32 {
        let t_after = (self.elapsed_ms - self.delay_ms - item_offset_ms).max(0.0);
        (t_after / self.duration_ms.max(1.0)).clamp(0.0, 1.0)
    }
}

/// Per-frame visual state for one element. Renderers multiply with
/// these values instead of inspecting the tween directly.
#[derive(Debug, Clone, Copy)]
pub struct Visual {
    /// 0..1 (or slightly past 1 for spring overshoot). For
    /// transient effects (radius interpolation, etc.).
    pub progress: f32,
    /// Multiplier on the element's natural size.
    pub scale: f32,
    /// Multiplier on the element's natural alpha, [0, 1].
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
    let going_in = tween.target >= tween.start;
    let cfg = if going_in { enter } else { exit };
    visual_for(tween.current, cfg)
}

/// Same as `evaluate` but for chain sub-items: applies a per-item
/// `offset_ms` so each item starts later than the master tween.
/// `enter`/`exit` give the *kind* + `initial_*`/`final_*`; the
/// timing knobs come from the master tween's already-set duration
/// and easing.
pub fn evaluate_chain_item(
    tween: &Tween,
    enter: &TransitionConfig,
    exit: &TransitionConfig,
    offset_ms: f32,
) -> Visual {
    let going_in = tween.target >= tween.start;
    let cfg = if going_in { enter } else { exit };
    let raw_t = tween.item_progress(offset_ms);
    // For chain items we re-apply the *configured* easing on top
    // of the per-item raw t — which matches what the master tween
    // would do for that item if it were a tween in its own right.
    let eased = tween.easing.eval(raw_t);
    let interpolated = lerp(tween.start, tween.end, eased);
    visual_for(interpolated, cfg)
}

fn visual_for(current: f32, cfg: &TransitionConfig) -> Visual {
    let p = current.clamp(-0.5, 1.5); // allow modest spring overshoot
    match cfg.kind {
        TransitionKind::None => {
            let on = current > 0.5;
            Visual {
                progress: if on { 1.0 } else { 0.0 },
                scale: 1.0,
                opacity: if on { cfg.final_opacity } else { 0.0 },
            }
        }
        TransitionKind::Fade => {
            let op = lerp(cfg.initial_opacity, cfg.final_opacity, p.clamp(0.0, 1.0));
            Visual {
                progress: p,
                scale: 1.0,
                opacity: op,
            }
        }
        TransitionKind::Grow => Visual {
            progress: p,
            scale: lerp(cfg.initial_scale, 1.0, p),
            opacity: if current > 0.001 { cfg.final_opacity } else { 0.0 },
        },
        TransitionKind::GrowAndFade => {
            let scale = lerp(cfg.initial_scale, 1.0, p);
            let op = lerp(cfg.initial_opacity, cfg.final_opacity, p.clamp(0.0, 1.0));
            Visual {
                progress: p,
                scale,
                opacity: op,
            }
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Convenience: pull the per-item stagger out of an
/// `ElementAnimation`'s chain block, defaulting to 0 when the
/// element has no chain configured. Renderers call this once per
/// draw to compute per-item offsets.
pub fn chain_stagger_ms(chain: Option<&ChainConfig>) -> f32 {
    chain.map(|c| c.stagger_ms as f32).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use juhradial_shared::{Easing, TransitionConfig, TransitionKind};

    fn fade_cfg(duration_ms: u32) -> TransitionConfig {
        TransitionConfig {
            kind: TransitionKind::Fade,
            duration_ms,
            easing: Easing::Linear,
            ..Default::default()
        }
    }

    #[test]
    fn idle_tween_stays_idle() {
        let mut t = Tween::at(0.5);
        for _ in 0..10 {
            t.step(16.0);
        }
        assert!((t.current - 0.5).abs() < 1e-4);
    }

    #[test]
    fn linear_fade_reaches_target_in_duration() {
        let mut t = Tween::at(0.0);
        let cfg = fade_cfg(100); // 100ms duration
        t.set_target(1.0, &cfg);
        // 100ms = 6 ticks at ~16.67 ms; pump 7 to be safe.
        for _ in 0..7 {
            t.step(16.67);
        }
        assert!((t.current - 1.0).abs() < 1e-4, "got {}", t.current);
        assert!(t.is_idle());
    }

    #[test]
    fn delay_holds_value_then_animates() {
        let mut t = Tween::at(0.0);
        let cfg = TransitionConfig {
            kind: TransitionKind::Fade,
            duration_ms: 100,
            delay_ms: 100,
            easing: Easing::Linear,
            ..Default::default()
        };
        t.set_target(1.0, &cfg);
        // After 50ms (still in delay), value should be ~0.
        t.step(50.0);
        assert!(t.current.abs() < 1e-3, "delay phase: {}", t.current);
        // After full delay + half duration: value should be ~0.5.
        t.step(100.0);
        assert!((t.current - 0.5).abs() < 0.05, "mid-anim: {}", t.current);
    }

    #[test]
    fn reversing_mid_flight_starts_from_current() {
        let mut t = Tween::at(0.0);
        let cfg = fade_cfg(200);
        t.set_target(1.0, &cfg);
        t.step(100.0); // halfway up
        let mid = t.current;
        assert!(mid > 0.3 && mid < 0.7);
        // Reverse: the new start should be `mid`, not 1.
        t.set_target(0.0, &cfg);
        assert!((t.start - mid).abs() < 1e-4);
        // After full duration of the reverse, should be at 0.
        t.step(220.0);
        assert!(t.current.abs() < 1e-3);
    }

    #[test]
    fn none_kind_snaps_instantly() {
        let mut t = Tween::at(0.0);
        let cfg = TransitionConfig {
            kind: TransitionKind::None,
            duration_ms: 1000,
            ..Default::default()
        };
        t.set_target(1.0, &cfg);
        assert!((t.current - 1.0).abs() < 1e-4);
        assert!(t.is_idle());
    }

    #[test]
    fn item_progress_handles_offsets() {
        let mut t = Tween::at(0.0);
        let cfg = fade_cfg(200);
        t.set_target(1.0, &cfg);
        t.step(100.0);
        // Item with 0 offset — half-way through.
        assert!((t.item_progress(0.0) - 0.5).abs() < 0.05);
        // Item with 100ms offset — just starting.
        assert!(t.item_progress(100.0).abs() < 0.05);
        // Item with 200ms offset — hasn't started yet.
        assert!(t.item_progress(200.0).abs() < 1e-4);
    }

    #[test]
    fn evaluate_grow_and_fade_lerps_both() {
        let mut t = Tween::at(0.5);
        t.target = 1.0;
        t.start = 0.0;
        let cfg = TransitionConfig {
            kind: TransitionKind::GrowAndFade,
            initial_scale: 0.0,
            initial_opacity: 0.0,
            final_opacity: 1.0,
            ..Default::default()
        };
        let v = evaluate(&t, &cfg, &cfg);
        assert!((v.scale - 0.5).abs() < 1e-4);
        assert!((v.opacity - 0.5).abs() < 1e-4);
    }
}
