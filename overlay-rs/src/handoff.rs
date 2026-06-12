//! Center-puck handoff state machine for the AI page.
//!
//! When the page cycle lands on the AI page, the disc morphs to the
//! chat shell but the centre puck survives: it flies into the chat
//! header and stays *armed* — wheel input keeps cycling pages and
//! the chat is rendered but not focus-active. The puck disarms (and
//! the chat becomes the real interaction target) only on a
//! deliberate gesture:
//!
//!   * a click anywhere in the chat outside the puck's hit circle, OR
//!   * cursor travel that leaves the disc's centre zone
//!     (`CENTER_ZONE_RADIUS` around the old disc centre) after at
//!     least [`ACTIVATION_TRAVEL_PX`] of real motion.
//!
//! A cursor merely *resting* where the disc centre was must NOT
//! activate — that's why activation requires accumulated travel, not
//! just a position outside the zone: the first pointer event after
//! arming only establishes the baseline, and micro-jitter below the
//! travel floor is ignored.
//!
//! This module is deliberately iced-free (plain points + instants)
//! so the transition rules are unit-testable without a widget tree.

use std::time::Instant;

/// Minimum accumulated pointer travel (px) since arming before a
/// position outside the centre zone counts as "deliberate mouse
/// travel into the chat".
pub const ACTIVATION_TRAVEL_PX: f32 = 8.0;

/// Radius of the parked header puck (32 px puck per the design).
pub const HEADER_PUCK_R: f32 = 16.0;

/// Extra slop around the header puck's hit circle so wheel/click
/// targeting the small puck doesn't demand pixel perfection.
pub const HEADER_PUCK_HIT_SLOP: f32 = 4.0;

/// A point in window-local logical pixels. Mirror of `iced::Point`
/// without the dependency.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct P {
    pub x: f32,
    pub y: f32,
}

impl P {
    pub fn new(x: f32, y: f32) -> Self {
        P { x, y }
    }

    fn dist(self, other: P) -> f32 {
        let (dx, dy) = (self.x - other.x, self.y - other.y);
        (dx * dx + dy * dy).sqrt()
    }
}

/// Handoff phase. See module docs for the transition rules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AiHandoff {
    /// Not on the AI page (or chat closed).
    Inactive,
    /// Chat shell up (or morphing), puck armed: wheel cycles pages,
    /// chat is render-only.
    PuckArmed {
        entered_at: Instant,
        /// Last pointer position seen since arming. `None` until the
        /// first pointer event establishes the baseline.
        last_cursor: Option<P>,
        /// Accumulated pointer travel since arming, px.
        travel: f32,
    },
    /// Puck disarmed — the chat owns input; wheel cycles pages only
    /// over the header puck's hit circle.
    ChatActive,
}

impl AiHandoff {
    /// Arm the puck (page cycle just landed on the AI page).
    pub fn armed(now: Instant) -> Self {
        AiHandoff::PuckArmed {
            entered_at: now,
            last_cursor: None,
            travel: 0.0,
        }
    }

    pub fn is_armed(&self) -> bool {
        matches!(self, AiHandoff::PuckArmed { .. })
    }

    pub fn is_chat_active(&self) -> bool {
        matches!(self, AiHandoff::ChatActive)
    }

    /// Feed a pointer-motion event. `disc_center` / `center_zone_r`
    /// describe the old disc's centre circle (the zone whose *exit*
    /// activates). Returns `true` when this event activated the chat.
    pub fn on_pointer(&mut self, pos: P, disc_center: P, center_zone_r: f32) -> bool {
        let AiHandoff::PuckArmed {
            entered_at,
            last_cursor,
            travel,
        } = *self
        else {
            return false;
        };
        let Some(last) = last_cursor else {
            // First event after arming: baseline only. Even a
            // position far outside the zone must not activate —
            // the cursor may simply have been resting there when
            // the wheel landed on the AI page.
            *self = AiHandoff::PuckArmed {
                entered_at,
                last_cursor: Some(pos),
                travel: 0.0,
            };
            return false;
        };
        let travel = travel + pos.dist(last);
        let outside = pos.dist(disc_center) > center_zone_r;
        if outside && travel >= ACTIVATION_TRAVEL_PX {
            *self = AiHandoff::ChatActive;
            return true;
        }
        *self = AiHandoff::PuckArmed {
            entered_at,
            last_cursor: Some(pos),
            travel,
        };
        false
    }

    /// Feed a click. Clicks outside the puck's hit circle activate;
    /// clicks on the puck itself leave it armed (the puck is the
    /// page-cycle affordance, not a chat surface). Returns `true`
    /// when this click activated the chat.
    pub fn on_click(&mut self, pos: P, puck_center: P, puck_hit_r: f32) -> bool {
        if !self.is_armed() {
            return false;
        }
        if pos.dist(puck_center) <= puck_hit_r {
            return false;
        }
        *self = AiHandoff::ChatActive;
        true
    }

    /// Force activation (a chat widget consumed an interaction the
    /// canvas never saw — e.g. a click landing on a thread chip).
    pub fn activate(&mut self) {
        if self.is_armed() {
            *self = AiHandoff::ChatActive;
        }
    }

    /// Leave the handoff entirely (cycled away from the AI page, or
    /// the chat was closed).
    pub fn reset(&mut self) {
        *self = AiHandoff::Inactive;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CENTER: P = P { x: 242.0, y: 242.0 };
    const ZONE_R: f32 = 45.0;

    fn armed() -> AiHandoff {
        AiHandoff::armed(Instant::now())
    }

    #[test]
    fn resting_cursor_never_activates() {
        // The cursor sits where the disc centre was; nothing moves.
        let mut h = armed();
        assert!(!h.on_pointer(CENTER, CENTER, ZONE_R));
        assert!(h.is_armed());
        // Same position re-reported (iced re-synthesises moves on
        // redraws) — travel stays 0.
        assert!(!h.on_pointer(CENTER, CENTER, ZONE_R));
        assert!(h.is_armed());
    }

    #[test]
    fn resting_cursor_outside_zone_never_activates() {
        // Cursor happened to rest outside the centre zone when the
        // cycle landed on the AI page — the first event is baseline
        // only, and re-reports add no travel.
        let mut h = armed();
        let far = P::new(400.0, 500.0);
        assert!(!h.on_pointer(far, CENTER, ZONE_R));
        assert!(h.is_armed());
        assert!(!h.on_pointer(far, CENTER, ZONE_R));
        assert!(h.is_armed());
    }

    #[test]
    fn deliberate_travel_out_of_zone_activates() {
        let mut h = armed();
        assert!(!h.on_pointer(CENTER, CENTER, ZONE_R));
        // 50 px straight down: well past the travel floor and
        // outside the 45 px zone.
        let out = P::new(CENTER.x, CENTER.y + 50.0);
        assert!(h.on_pointer(out, CENTER, ZONE_R));
        assert!(h.is_chat_active());
    }

    #[test]
    fn jitter_below_travel_floor_does_not_activate() {
        // Baseline right at the zone edge, then 3 px of jitter that
        // crosses outside — under the 8 px floor, stays armed.
        let mut h = armed();
        let edge = P::new(CENTER.x + ZONE_R - 1.0, CENTER.y);
        assert!(!h.on_pointer(edge, CENTER, ZONE_R));
        let jitter = P::new(CENTER.x + ZONE_R + 2.0, CENTER.y);
        assert!(!h.on_pointer(jitter, CENTER, ZONE_R));
        assert!(h.is_armed());
    }

    #[test]
    fn travel_accumulates_across_events() {
        // Many small moves inside the zone, then a small step out:
        // cumulative travel passes the floor even though no single
        // step does.
        let mut h = armed();
        assert!(!h.on_pointer(CENTER, CENTER, ZONE_R));
        let mut x = CENTER.x;
        for _ in 0..14 {
            x += 3.0; // 42 px total, still inside at 42 < 45
            assert!(!h.on_pointer(P::new(x, CENTER.y), CENTER, ZONE_R));
        }
        assert!(h.is_armed());
        // One more 4 px step crosses the zone edge with travel = 46.
        assert!(h.on_pointer(P::new(x + 4.0, CENTER.y), CENTER, ZONE_R));
        assert!(h.is_chat_active());
    }

    #[test]
    fn travel_inside_zone_does_not_activate() {
        // Lots of motion that never leaves the centre zone.
        let mut h = armed();
        assert!(!h.on_pointer(CENTER, CENTER, ZONE_R));
        for i in 0..20 {
            let dx = if i % 2 == 0 { 20.0 } else { -20.0 };
            assert!(!h.on_pointer(P::new(CENTER.x + dx, CENTER.y), CENTER, ZONE_R));
        }
        assert!(h.is_armed());
    }

    #[test]
    fn click_outside_puck_activates() {
        let mut h = armed();
        let puck = P::new(38.0, 30.0);
        let in_chat = P::new(240.0, 400.0);
        assert!(h.on_click(in_chat, puck, HEADER_PUCK_R + HEADER_PUCK_HIT_SLOP));
        assert!(h.is_chat_active());
    }

    #[test]
    fn click_on_puck_stays_armed() {
        let mut h = armed();
        let puck = P::new(38.0, 30.0);
        assert!(!h.on_click(puck, puck, HEADER_PUCK_R + HEADER_PUCK_HIT_SLOP));
        assert!(h.is_armed());
        // Just inside the slop ring still counts as the puck.
        let near = P::new(puck.x + HEADER_PUCK_R + HEADER_PUCK_HIT_SLOP - 0.5, puck.y);
        assert!(!h.on_click(near, puck, HEADER_PUCK_R + HEADER_PUCK_HIT_SLOP));
        assert!(h.is_armed());
    }

    #[test]
    fn click_does_nothing_when_not_armed() {
        let mut h = AiHandoff::ChatActive;
        let puck = P::new(38.0, 30.0);
        assert!(!h.on_click(P::new(240.0, 400.0), puck, HEADER_PUCK_R));
        assert!(h.is_chat_active());
        let mut h = AiHandoff::Inactive;
        assert!(!h.on_click(P::new(240.0, 400.0), puck, HEADER_PUCK_R));
        assert_eq!(h, AiHandoff::Inactive);
    }

    #[test]
    fn reset_and_rearm_round_trip() {
        let mut h = armed();
        h.activate();
        assert!(h.is_chat_active());
        h.reset();
        assert_eq!(h, AiHandoff::Inactive);
        h = AiHandoff::armed(Instant::now());
        assert!(h.is_armed());
    }

    #[test]
    fn pointer_events_ignored_when_not_armed() {
        let mut h = AiHandoff::ChatActive;
        assert!(!h.on_pointer(P::new(400.0, 400.0), CENTER, ZONE_R));
        assert!(h.is_chat_active());
        let mut h = AiHandoff::Inactive;
        assert!(!h.on_pointer(P::new(400.0, 400.0), CENTER, ZONE_R));
        assert_eq!(h, AiHandoff::Inactive);
    }
}
