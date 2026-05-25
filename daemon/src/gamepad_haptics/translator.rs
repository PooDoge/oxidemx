//! Rumble → MX Master 4 haptic translation.
//!
//! A game's force-feedback effect carries two motor magnitudes (a
//! low-frequency "strong" channel and a high-frequency "weak" one),
//! both `u16`. The MX Master 4's piezo actuator is a single,
//! discrete-waveform device — so translation is lossy by nature:
//! we mix the two channels into one intensity, shape it with a
//! user-chosen curve, and pick one of four hardware waveforms.
//!
//! See `HAPTIC_GAMEPAD_BRIDGE_DESIGN.md` §7 for the full design.
//!
//! ## Phase status
//!
//! **Phase 5 (this commit):** Event mode — one haptic pulse per
//! play, continuous rumble re-pulsed at the throttle rate. The
//! `Stream` event mode (intensity-paced ticks) is not implemented
//! yet; it degrades to Event.

use std::time::{Duration, Instant};

use evdev::{FFEffectData, FFEffectKind, FFReplay, FFTrigger};
use juhradial_shared::{HapticEventMode, HapticRedirectConfig, HapticRedirectCurve};

use crate::hidpp::{Mx4HapticPattern, SharedHapticManager};

// ────────────────────────────────────────────────────────────────────
// Pure translation math (no device, no state — unit-tested directly)
// ────────────────────────────────────────────────────────────────────

/// Pull `(strong, weak)` magnitudes on the `0..=65535` rumble scale
/// out of any effect kind.
///
/// `Periodic` / `Constant` / `Ramp` carry a *signed* level whose
/// half-range peaks at `i16::MAX`; doubling `|level|` lifts it onto
/// the same `u16` scale as a rumble effect. `Spring` / `Friction` /
/// `Damper` / `Inertia` are positional/conditional effects with no
/// rumble feel — they contribute nothing.
fn magnitudes(effect: &FFEffectData) -> (f32, f32) {
    match effect.kind {
        FFEffectKind::Rumble {
            strong_magnitude,
            weak_magnitude,
        } => (f32::from(strong_magnitude), f32::from(weak_magnitude)),
        FFEffectKind::Periodic { magnitude, .. } => {
            (f32::from(magnitude.unsigned_abs()) * 2.0, 0.0)
        }
        FFEffectKind::Constant { level, .. } => {
            (f32::from(level.unsigned_abs()) * 2.0, 0.0)
        }
        FFEffectKind::Ramp {
            start_level,
            end_level,
            ..
        } => {
            let peak = start_level.unsigned_abs().max(end_level.unsigned_abs());
            (f32::from(peak) * 2.0, 0.0)
        }
        FFEffectKind::Spring { .. }
        | FFEffectKind::Friction { .. }
        | FFEffectKind::Damper
        | FFEffectKind::Inertia => (0.0, 0.0),
    }
}

/// Combined rumble intensity `0.0..=1.0` — the two motor channels
/// mixed by the configured weights, then scaled by the live FF
/// master gain and the user's intensity multiplier.
fn combined_intensity(cfg: &HapticRedirectConfig, effect: &FFEffectData, gain: f32) -> f32 {
    let (strong, weak) = magnitudes(effect);
    let mixed = (cfg.strong_weight * strong + cfg.weak_weight * weak) / f32::from(u16::MAX);
    (mixed * gain * cfg.intensity_scale).clamp(0.0, 1.0)
}

/// Shape the combined intensity through the user's chosen curve.
/// Input and output are both `0.0..=1.0`.
fn apply_curve(curve: HapticRedirectCurve, x: f32) -> f32 {
    match curve {
        // Faithful — pattern tier tracks intensity linearly.
        HapticRedirectCurve::Linear => x,
        // Quadratic accent: sustained mid-magnitude rumble feels
        // lighter, sharp peaks still reach the top tiers.
        HapticRedirectCurve::Eventy => x * x,
        // Logarithmic compression normalised to 0..1, then held
        // under a 0.7 ceiling so nothing reaches the heaviest tier.
        HapticRedirectCurve::Subtle => {
            const CEILING: f32 = 0.7;
            (x.ln_1p() / std::f32::consts::LN_2) * CEILING
        }
    }
}

/// Map a shaped intensity to one of the four MX-4 collision
/// waveforms — a perceptually-ascending ramp (whisper → subtle →
/// damp → sharp). Thresholds are §7.4 of the design doc.
fn tier(shaped: f32) -> Mx4HapticPattern {
    if shaped < 0.20 {
        Mx4HapticPattern::WhisperCollision
    } else if shaped < 0.50 {
        Mx4HapticPattern::SubtleCollision
    } else if shaped < 0.80 {
        Mx4HapticPattern::DampCollision
    } else {
        Mx4HapticPattern::SharpCollision
    }
}

/// Full translation: an FF effect → the MX-4 waveform to play, or
/// `None` when the rumble is below the configured deadzone.
fn translate(
    cfg: &HapticRedirectConfig,
    effect: &FFEffectData,
    gain: f32,
) -> Option<Mx4HapticPattern> {
    let combined = combined_intensity(cfg, effect, gain);
    if combined < cfg.min_intensity {
        return None;
    }
    Some(tier(apply_curve(cfg.curve, combined)))
}

/// Fire a single representative haptic pulse — the settings "Test
/// haptic" button uses this to verify the rumble → haptic chain end
/// to end without launching a game (design §11.3).
///
/// Builds a synthetic mid-strong rumble effect, runs it through the
/// same [`translate`] the live path uses, and dispatches the
/// resulting pattern through the shared haptic manager.
pub fn fire_test_pulse(config: &HapticRedirectConfig, haptics: &SharedHapticManager) {
    let synthetic = FFEffectData {
        direction: 0,
        trigger: FFTrigger::default(),
        replay: FFReplay {
            length: 500,
            delay: 0,
        },
        kind: FFEffectKind::Rumble {
            strong_magnitude: 0xC000,
            weak_magnitude: 0x4000,
        },
    };

    match translate(config, &synthetic, 1.0) {
        Some(pattern) => match haptics.lock() {
            Ok(mut manager) => {
                let _ = manager.pulse_pattern(pattern);
                tracing::info!(?pattern, "haptic redirect: test pulse fired");
            }
            Err(e) => tracing::warn!(
                error = %e,
                "haptic redirect: test pulse — haptic-manager mutex poisoned"
            ),
        },
        None => tracing::info!(
            "haptic redirect: test pulse — synthetic effect fell below the deadzone"
        ),
    }
}

// ────────────────────────────────────────────────────────────────────
// Stateful translator — throttling, sustain tracking, dispatch
// ────────────────────────────────────────────────────────────────────

/// The effect currently driving sustained haptic output. The piezo
/// can't mix, so we track a single sustain — the most recent play
/// wins (see design §14.6 for the full "highest-intensity" rule,
/// deferred).
struct Sustain {
    effect_id: u16,
    pattern: Mx4HapticPattern,
    /// `None` = play until an explicit stop/erase (a `replay.length`
    /// of 0 — infinite). `Some` = a finite effect that stops itself.
    stop_at: Option<Instant>,
}

/// Translates incoming rumble into MX Master 4 haptic pulses and
/// dispatches them through the shared [`HapticManager`].
///
/// Lives inside the FF servicing loop ([`super::ff_protocol::serve`]);
/// the loop feeds it play / stop / erase / gain events and ticks it
/// at the throttle rate so sustained rumble keeps pulsing.
pub struct RumbleTranslator {
    cfg: HapticRedirectConfig,
    haptics: SharedHapticManager,
    /// Live FF master gain, `0.0..=1.0`, from `FF_GAIN` events.
    gain: f32,
    /// When we last dispatched a pulse — the throttle anchor.
    last_emit: Option<Instant>,
    /// The effect currently being sustained, if any.
    sustain: Option<Sustain>,
}

impl RumbleTranslator {
    pub fn new(cfg: HapticRedirectConfig, haptics: SharedHapticManager) -> Self {
        if matches!(cfg.event_mode, HapticEventMode::Stream) {
            tracing::info!(
                "haptic redirect: 'stream' event mode is not implemented yet — \
                 using 'event' mode"
            );
        }
        Self {
            cfg,
            haptics,
            gain: 1.0,
            last_emit: None,
            sustain: None,
        }
    }

    /// The cadence at which the servicing loop should call [`Self::tick`]
    /// — also the minimum gap between dispatched pulses.
    pub fn tick_period(&self) -> Duration {
        Duration::from_millis(u64::from(self.cfg.throttle_ms.max(1)))
    }

    /// Record an `FF_GAIN` change. `raw` is the kernel value
    /// (`0..=0xFFFF`).
    pub fn on_gain(&mut self, raw: i32) {
        let clamped = raw.clamp(0, i32::from(u16::MAX)) as f32;
        self.gain = clamped / f32::from(u16::MAX);
        tracing::debug!(gain = self.gain, "haptic redirect: FF master gain updated");
    }

    /// Handle an effect starting to play. `repeat` is the `EV_FF`
    /// event value (play count); `effect` is the data stored at
    /// upload time.
    pub fn on_play(&mut self, effect_id: u16, effect: &FFEffectData, repeat: i32) {
        let pattern = match translate(&self.cfg, effect, self.gain) {
            Some(pattern) => pattern,
            None => {
                // Below the deadzone — emit nothing, and stop
                // sustaining this effect if we were. Log the
                // computed intensity so users can tell "rumble
                // arrived but was too quiet for the deadzone" from
                // "no rumble arrived at all".
                let (strong, weak) = magnitudes(effect);
                let intensity = combined_intensity(&self.cfg, effect, self.gain);
                tracing::debug!(
                    effect_id,
                    strong = strong as u16,
                    weak = weak as u16,
                    intensity,
                    deadzone = self.cfg.min_intensity,
                    "haptic redirect: dropped rumble — below deadzone"
                );
                if self.sustain_matches(effect_id) {
                    self.sustain = None;
                }
                return;
            }
        };

        // `replay.length == 0` is an infinite effect (sustain until
        // an explicit stop). A finite length sustains for
        // length × repeat, then self-expires; a length shorter than
        // one throttle tick collapses to a single pulse, which is
        // exactly the "short transient = one pulse" case from §7.5.
        let stop_at = match effect.replay.length {
            0 => None,
            len => {
                let reps = u64::from(repeat.max(1) as u32);
                Some(Instant::now() + Duration::from_millis(u64::from(len) * reps))
            }
        };

        self.sustain = Some(Sustain {
            effect_id,
            pattern,
            stop_at,
        });
        self.fire(pattern);
    }

    /// Handle an effect being stopped (`EV_FF` value 0).
    pub fn on_stop(&mut self, effect_id: u16) {
        if self.sustain_matches(effect_id) {
            self.sustain = None;
        }
    }

    /// Handle an effect being erased — same as a stop for our purposes.
    pub fn on_erase(&mut self, effect_id: u16) {
        self.on_stop(effect_id);
    }

    /// Periodic tick: re-pulse a sustained effect at the throttle
    /// rate, and retire it once its finite duration has elapsed.
    pub fn tick(&mut self) {
        let Some(sustain) = self.sustain.as_ref() else {
            return;
        };
        if let Some(stop_at) = sustain.stop_at {
            if Instant::now() >= stop_at {
                self.sustain = None;
                return;
            }
        }
        let pattern = sustain.pattern;
        self.fire(pattern);
    }

    /// Whether the current sustain (if any) is for `effect_id`.
    fn sustain_matches(&self, effect_id: u16) -> bool {
        self.sustain.as_ref().map(|s| s.effect_id) == Some(effect_id)
    }

    /// Dispatch a pattern to the mouse, if the throttle window has
    /// elapsed since the last pulse.
    fn fire(&mut self, pattern: Mx4HapticPattern) {
        let now = Instant::now();
        if let Some(last) = self.last_emit {
            if now.duration_since(last) < self.tick_period() {
                return; // throttled — too soon since the last pulse
            }
        }
        self.last_emit = Some(now);

        match self.haptics.lock() {
            Ok(mut manager) => {
                // pulse_pattern swallows device errors internally —
                // haptics are best-effort.
                let _ = manager.pulse_pattern(pattern);
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "haptic redirect: haptic-manager mutex poisoned — dropping pulse"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evdev::{FFReplay, FFTrigger};

    fn rumble(strong: u16, weak: u16) -> FFEffectData {
        FFEffectData {
            direction: 0,
            trigger: FFTrigger::default(),
            replay: FFReplay {
                length: 0,
                delay: 0,
            },
            kind: FFEffectKind::Rumble {
                strong_magnitude: strong,
                weak_magnitude: weak,
            },
        }
    }

    #[test]
    fn combined_intensity_mixes_by_weight() {
        let cfg = HapticRedirectConfig::default(); // strong 1.0, weak 0.4
        // Pure strong, full magnitude → clamps to 1.0.
        assert_eq!(combined_intensity(&cfg, &rumble(0xFFFF, 0), 1.0), 1.0);
        // Pure weak, full magnitude → 0.4 of the scale.
        let weak_only = combined_intensity(&cfg, &rumble(0, 0xFFFF), 1.0);
        assert!((weak_only - 0.4).abs() < 0.001, "got {weak_only}");
        // Silence stays silent.
        assert_eq!(combined_intensity(&cfg, &rumble(0, 0), 1.0), 0.0);
    }

    #[test]
    fn gain_and_scale_attenuate() {
        let mut cfg = HapticRedirectConfig::default();
        // Half gain halves a full-strong effect.
        let half = combined_intensity(&cfg, &rumble(0xFFFF, 0), 0.5);
        assert!((half - 0.5).abs() < 0.001, "got {half}");
        // intensity_scale below 1 attenuates too.
        cfg.intensity_scale = 0.25;
        let quarter = combined_intensity(&cfg, &rumble(0xFFFF, 0), 1.0);
        assert!((quarter - 0.25).abs() < 0.001, "got {quarter}");
    }

    #[test]
    fn curve_shapes() {
        // Linear is the identity.
        assert_eq!(apply_curve(HapticRedirectCurve::Linear, 0.6), 0.6);
        // Eventy is quadratic — mid values pulled down.
        assert!((apply_curve(HapticRedirectCurve::Eventy, 0.5) - 0.25).abs() < 0.001);
        // Subtle compresses and caps under its 0.7 ceiling.
        let top = apply_curve(HapticRedirectCurve::Subtle, 1.0);
        assert!((top - 0.7).abs() < 0.001, "ceiling should be ~0.7, got {top}");
        assert_eq!(apply_curve(HapticRedirectCurve::Subtle, 0.0), 0.0);
        // Subtle lifts low values above linear (logarithmic).
        assert!(apply_curve(HapticRedirectCurve::Subtle, 0.3) > 0.3 * 0.7);
    }

    #[test]
    fn tier_thresholds_ascend() {
        assert_eq!(tier(0.05), Mx4HapticPattern::WhisperCollision);
        assert_eq!(tier(0.20), Mx4HapticPattern::SubtleCollision);
        assert_eq!(tier(0.49), Mx4HapticPattern::SubtleCollision);
        assert_eq!(tier(0.50), Mx4HapticPattern::DampCollision);
        assert_eq!(tier(0.79), Mx4HapticPattern::DampCollision);
        assert_eq!(tier(0.80), Mx4HapticPattern::SharpCollision);
        assert_eq!(tier(1.00), Mx4HapticPattern::SharpCollision);
    }

    #[test]
    fn deadzone_drops_quiet_rumble() {
        let cfg = HapticRedirectConfig::default(); // min_intensity 0.04
        // A faint rumble (~3% of scale) is below the deadzone.
        assert!(translate(&cfg, &rumble(0x0800, 0), 1.0).is_none());
        // A solid rumble translates.
        assert!(translate(&cfg, &rumble(0xC000, 0x4000), 1.0).is_some());
    }

    #[test]
    fn translate_picks_ascending_tiers_with_linear_curve() {
        let mut cfg = HapticRedirectConfig::default();
        cfg.curve = HapticRedirectCurve::Linear;
        // ~30% → SubtleCollision band (0.20..0.50).
        assert_eq!(
            translate(&cfg, &rumble(0x4CCC, 0), 1.0),
            Some(Mx4HapticPattern::SubtleCollision)
        );
        // Full strong → SharpCollision (top band).
        assert_eq!(
            translate(&cfg, &rumble(0xFFFF, 0), 1.0),
            Some(Mx4HapticPattern::SharpCollision)
        );
    }

    fn rumble_for(strong: u16, weak: u16, length_ms: u16) -> FFEffectData {
        FFEffectData {
            direction: 0,
            trigger: FFTrigger::default(),
            replay: FFReplay {
                length: length_ms,
                delay: 0,
            },
            kind: FFEffectKind::Rumble {
                strong_magnitude: strong,
                weak_magnitude: weak,
            },
        }
    }

    /// A translator with no haptic device behind it — `fire()` runs
    /// (locks the manager, calls `pulse_pattern`) but the manager
    /// no-ops with no device. Lets the stateful logic be exercised.
    fn test_translator() -> RumbleTranslator {
        let haptics = crate::hidpp::new_shared_haptic_manager(
            &crate::config::HapticConfig::default(),
        );
        RumbleTranslator::new(HapticRedirectConfig::default(), haptics)
    }

    #[test]
    fn play_sets_sustain_and_stop_clears_it() {
        let mut tr = test_translator();
        tr.on_play(3, &rumble(0xC000, 0x4000), 1);
        assert!(tr.sustain.is_some());
        assert_eq!(tr.sustain.as_ref().unwrap().effect_id, 3);

        tr.on_stop(3);
        assert!(tr.sustain.is_none());
    }

    #[test]
    fn stop_for_a_different_effect_keeps_the_sustain() {
        let mut tr = test_translator();
        tr.on_play(3, &rumble(0xC000, 0), 1);
        tr.on_stop(7); // unrelated effect id
        assert!(tr.sustain.is_some());
    }

    #[test]
    fn deadzone_play_does_not_sustain() {
        let mut tr = test_translator();
        // ~1.5% of scale — well under the 0.04 deadzone.
        tr.on_play(1, &rumble(0x0400, 0), 1);
        assert!(tr.sustain.is_none());
    }

    #[test]
    fn finite_effect_expires_on_tick() {
        let mut tr = test_translator();
        // A 1 ms effect: sustained at first, retired by the next
        // tick once its duration has elapsed.
        tr.on_play(2, &rumble_for(0xC000, 0, 1), 1);
        assert!(tr.sustain.is_some());

        std::thread::sleep(Duration::from_millis(5));
        tr.tick();
        assert!(tr.sustain.is_none());
    }

    #[test]
    fn throttle_blocks_a_rapid_second_pulse() {
        let mut tr = test_translator();
        tr.fire(Mx4HapticPattern::SharpCollision);
        let first = tr.last_emit;
        assert!(first.is_some());

        // Immediately again — inside the throttle window, so dropped;
        // last_emit must be untouched.
        tr.fire(Mx4HapticPattern::SharpCollision);
        assert_eq!(tr.last_emit, first);
    }

    #[test]
    fn periodic_effect_uses_magnitude() {
        let cfg = HapticRedirectConfig::default();
        let periodic = FFEffectData {
            direction: 0,
            trigger: FFTrigger::default(),
            replay: FFReplay {
                length: 0,
                delay: 0,
            },
            kind: FFEffectKind::Periodic {
                waveform: evdev::FFWaveform::Sine,
                period: 100,
                magnitude: 0x6000,
                offset: 0,
                phase: 0,
                envelope: evdev::FFEnvelope {
                    attack_length: 0,
                    attack_level: 0,
                    fade_length: 0,
                    fade_level: 0,
                },
            },
        };
        // |0x6000| * 2 ≈ 0x C000 on the u16 scale — well above the
        // deadzone, so it translates to a pattern.
        assert!(translate(&cfg, &periodic, 1.0).is_some());
    }
}
