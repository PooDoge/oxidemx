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

use oxidemx_shared::{
    Axis, ChainConfig, ComposedTransform, Easing, TransitionConfig, TransitionDirection,
    TransitionKind,
};

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

    /// Compute the [0,1] *raw* progress of a chain item that
    /// started `item_offset_ms` after this tween's transition
    /// began and runs for `item_duration_ms` of its own. Used by
    /// chain renderers (submenu sub-items) so every item runs
    /// for its configured duration regardless of where it sits
    /// in the chain — total chain time = `duration_ms + (n−1) ×
    /// stagger`, matching Motion.dev / Material's compose model.
    ///
    /// Returns the raw normalised time *not* the eased value —
    /// apply `easing` separately if needed.
    pub fn item_progress(&self, item_offset_ms: f32, item_duration_ms: f32) -> f32 {
        let t_after =
            (self.elapsed_ms - self.delay_ms - item_offset_ms).max(0.0);
        (t_after / item_duration_ms.max(1.0)).clamp(0.0, 1.0)
    }

    /// Total elapsed time since the current transition started,
    /// in milliseconds (includes the pre-roll delay). Public so
    /// the custom-track evaluator can drive each track's own
    /// timeline against the same wall-clock.
    pub fn elapsed_ms(&self) -> f32 {
        self.elapsed_ms
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
    #[allow(dead_code)]
    pub const VISIBLE: Visual = Visual { progress: 1.0, scale: 1.0, opacity: 1.0 };
    #[allow(dead_code)]
    pub const HIDDEN: Visual = Visual { progress: 0.0, scale: 0.0, opacity: 0.0 };
}

/// Translate a tween's current value plus the active enter/exit
/// transition into per-frame `Visual` parameters. Direction is
/// inferred from the tween: if it's heading toward 1 (or already
/// fully visible), apply the enter kind; if heading toward 0,
/// apply the exit kind.
///
/// Kept around for tests + as the inner core of `evaluate_composed`'s
/// preset path. Real renderers should call `evaluate_composed` so
/// they automatically pick up custom track lists when present.
#[allow(dead_code)]
pub fn evaluate(tween: &Tween, enter: &TransitionConfig, exit: &TransitionConfig) -> Visual {
    let going_in = tween.target >= tween.start;
    let cfg = if going_in { enter } else { exit };
    visual_for(tween.current, cfg)
}

/// Combined evaluator that returns the full [`ComposedTransform`]
/// (alpha + translate + rotate + scale + flip). Picks the
/// custom-tracks path when the active config has tracks; otherwise
/// falls back to the preset path and lifts its `Visual` into a
/// `ComposedTransform` with no translate/rotate/flip.
///
/// Renderers that want per-element transforms beyond the
/// preset's scale + opacity should call this instead of
/// `evaluate()`. The preset path is exactly equivalent to today's
/// behaviour, so swapping in `evaluate_composed` at every call
/// site is a no-op for users who haven't customised anything.
pub fn evaluate_composed(
    tween: &Tween,
    enter: &TransitionConfig,
    exit: &TransitionConfig,
) -> ComposedTransform {
    let going_in = tween.target >= tween.start;
    let direction = if going_in {
        TransitionDirection::Enter
    } else {
        TransitionDirection::Exit
    };
    let cfg = if going_in { enter } else { exit };
    evaluate_composed_for(tween, cfg, direction)
}

/// Same as `evaluate_composed` but with the direction supplied
/// explicitly (rather than inferred from the tween). Used by
/// page-transition rendering, which has ONE tween driving 0→1
/// over the cycle but needs Enter semantics on the incoming
/// ring and Exit semantics on the outgoing ring — the inferred
/// direction would always say "Enter" since the tween only ever
/// goes forward.
pub fn evaluate_composed_for(
    tween: &Tween,
    cfg: &TransitionConfig,
    direction: TransitionDirection,
) -> ComposedTransform {
    if cfg.is_custom() {
        return cfg.evaluate_tracks(tween.elapsed_ms(), direction);
    }
    let v = visual_for(tween.current, cfg);
    ComposedTransform {
        alpha: v.opacity,
        translate_x_px: 0.0,
        translate_y_px: 0.0,
        rotate_rad: 0.0,
        scale: v.scale,
        flip_scale: 1.0,
        flip_axis: Axis::Y,
    }
}

/// Same as `evaluate` but for chain sub-items: applies a per-item
/// `offset_ms` so each item starts later than the master tween.
/// `enter`/`exit` give the *kind* + `initial_*`/`final_*` AND the
/// per-item `duration_ms` (the master tween's `duration_ms` is
/// extended to fit the full chain — see
/// `extended_chain_duration_ms`).
pub fn evaluate_chain_item(
    tween: &Tween,
    enter: &TransitionConfig,
    exit: &TransitionConfig,
    offset_ms: f32,
) -> Visual {
    let going_in = tween.target >= tween.start;
    let cfg = if going_in { enter } else { exit };
    let item_dur = cfg.duration_ms.max(1) as f32;
    let raw_t = tween.item_progress(offset_ms, item_dur);
    // Apply the *configured* per-item easing on the per-item raw t.
    // Each chain item gets its own complete easing curve over its
    // own duration — same behaviour as if it were a standalone
    // tween, just driven off a single shared clock.
    let eased = cfg.easing.eval(raw_t);
    let interpolated = lerp(tween.start, tween.end, eased);
    visual_for(interpolated, cfg)
}

/// Total chain duration in ms — what the master tween's
/// `duration_ms` should be set to so that ALL chain items finish
/// their per-item animation. Compose model: total =
/// `base_duration_ms + (item_count − 1) × stagger_ms`. Used by
/// SubmenuState to extend the master tween's clock so the last
/// item doesn't get cut off when stagger × items > duration.
pub fn extended_chain_duration_ms(
    base_duration_ms: u32,
    item_count: usize,
    stagger_ms: f32,
) -> u32 {
    let n = item_count.saturating_sub(1) as f32;
    let total = base_duration_ms as f32 + n * stagger_ms.max(0.0);
    total.max(1.0) as u32
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
    use oxidemx_shared::{Easing, TransitionConfig, TransitionKind};

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
        // Master tween 200 ms; each item also runs for 200 ms.
        let mut t = Tween::at(0.0);
        let cfg = fade_cfg(200);
        t.set_target(1.0, &cfg);
        t.step(100.0);
        // Item with 0 offset — half-way through its 200 ms window.
        assert!((t.item_progress(0.0, 200.0) - 0.5).abs() < 0.05);
        // Item with 100 ms offset — just starting.
        assert!(t.item_progress(100.0, 200.0).abs() < 0.05);
        // Item with 200 ms offset — hasn't started yet.
        assert!(t.item_progress(200.0, 200.0).abs() < 1e-4);
    }

    #[test]
    fn item_progress_independent_of_master_duration() {
        // Compose model: master tween extends to fit chain
        // (duration + (n−1)*stagger). Per-item still runs for
        // its own duration. So item N at offset = N*stagger
        // reaches 0.5 at elapsed = N*stagger + item_duration/2,
        // regardless of how long the master tween is set to.
        let mut t = Tween::at(0.0);
        let mut cfg = fade_cfg(1000); // master extended to 1000 ms
        cfg.easing = Easing::Linear; // predictable raw progress
        t.set_target(1.0, &cfg);
        t.step(300.0);
        // Per-item duration is 200 ms. Item at offset 200 ms
        // started at elapsed=200, animated 100 ms so far → 50 %.
        let p = t.item_progress(200.0, 200.0);
        assert!((p - 0.5).abs() < 0.05, "expected ~0.5, got {p}");
    }

    #[test]
    fn extended_chain_duration_composes() {
        // 4 items, 250 ms duration, 80 ms stagger
        // → total = 250 + 3*80 = 490 ms.
        assert_eq!(extended_chain_duration_ms(250, 4, 80.0), 490);
        // 1 item: stagger doesn't apply.
        assert_eq!(extended_chain_duration_ms(250, 1, 80.0), 250);
        // 0 items: still clamps to base duration.
        assert_eq!(extended_chain_duration_ms(250, 0, 80.0), 250);
        // Negative stagger clamped to 0.
        assert_eq!(extended_chain_duration_ms(250, 4, -10.0), 250);
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
