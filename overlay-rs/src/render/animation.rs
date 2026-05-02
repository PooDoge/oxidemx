//! Frame-tick driver + easing curves.
//!
//! `glib::timeout_add_local(Duration::from_millis(16), ...)` for the
//! ~60 Hz frame tick. Easing curves match the legacy Python overlay so
//! the visual feel is preserved.

#[allow(dead_code)]
pub fn ease_out_back(t: f64, overshoot: f64) -> f64 {
    let t = t - 1.0;
    t * t * ((overshoot + 1.0) * t + overshoot) + 1.0
}

#[allow(dead_code)]
pub fn ease_out_quad(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(2)
}

// TODO: a single `AnimationDriver` that owns the frame timer and a list
// of in-flight tweens (slice highlight progress, submenu pop-out
// progress, centre pulse, flash on click).
