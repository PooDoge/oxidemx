//! ThumbWheel divert + forward path.
//!
//! The MX Master 4 firmware only honours the 0x2150 *invert* bit on
//! diverted notifications — with divert=false (the kernel-managed
//! default) the invert byte is ACKed but silently ignored, so the
//! physical scroll direction never changes. Solaar achieves the
//! reversal by routing the wheel through the divert pipeline; this
//! module is our equivalent.
//!
//! When the user enables horizontal-scroll inversion in the settings:
//!   1. The daemon writes `set_thumb_wheel_reporting(divert=true,
//!      invert=true)` on feature 0x2150.
//!   2. The firmware stops emitting REL_HWHEEL evdev events and starts
//!      sending HID++ broadcast notifications (function 0) carrying
//!      the signed displacement in bytes [4..6] of the report.
//!   3. `hidraw.rs` recognises those notifications and dispatches
//!      them here via [`ThumbWheelForwarder::emit_displacement`].
//!   4. We re-emit REL_HWHEEL (+ REL_HWHEEL_HI_RES) on a small uinput
//!      device so the compositor still sees horizontal scrolling —
//!      otherwise the user loses h-scroll entirely.
//!
//! `feedback_hidpp_wire_format.md` documents the 0x2150 notification
//! layout. Solaar reference: `diversion.py:1486-1489`.
//!
//! When invert is turned off, the daemon writes `(divert=false,
//! invert=false)`, the firmware resumes emitting REL_HWHEEL itself,
//! and we drop the uinput device.

use std::io;
use std::sync::{Arc, Mutex};

use evdev::{
    uinput::VirtualDevice, AttributeSet, BusType, EventType, InputEvent, InputId,
    RelativeAxisCode,
};

/// Device name announced on /dev/input. The same string is used by
/// the diagnostics scan to recognise our forwarder.
pub const DEVICE_NAME: &str = "oxidemx thumb-wheel forwarder";

/// Logitech VID — keeps compositor heuristics that distinguish "mouse
/// horizontal scroll" from "trackpad horizontal scroll" on the side
/// of treating us as a mouse.
const VENDOR: u16 = 0x046D;
/// Arbitrary product id, picked clear of any real Logitech device.
const PRODUCT: u16 = 0x4218;
const VERSION: u16 = 0x0001;

/// One "detent" of high-resolution scroll. The kernel hidpp driver
/// emits 120 per click for MX-series mice; mirroring that keeps
/// downstream apps (Firefox, GTK, electron) feeling identical.
const TICKS_PER_DETENT: i32 = 120;

/// Uinput device that re-emits horizontal scroll events from the
/// diverted thumb-wheel stream.
pub struct ThumbWheelForwarder {
    device: VirtualDevice,
    invert: bool,
    /// Hi-res accumulator: the kernel hidpp driver only emits the
    /// coarse REL_HWHEEL event once 120 hi-res ticks have piled up
    /// in one direction. We do the same so behaviour matches.
    hires_accum: i32,
}

impl ThumbWheelForwarder {
    /// Build the uinput device. `invert=true` sign-flips every
    /// displacement before emitting; the caller is expected to mirror
    /// this with `set_thumb_wheel_reporting(divert=true, invert=…)`
    /// on the device so the firmware-side and software-side stay in
    /// sync if/when we discover the firmware itself honours invert
    /// (the current best guess is that it doesn't — see the design
    /// note in the module docs).
    pub fn new(invert: bool) -> io::Result<Self> {
        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        axes.insert(RelativeAxisCode::REL_HWHEEL);
        axes.insert(RelativeAxisCode::REL_HWHEEL_HI_RES);

        let device = VirtualDevice::builder()?
            .name(DEVICE_NAME)
            .input_id(InputId::new(BusType::BUS_VIRTUAL, VENDOR, PRODUCT, VERSION))
            .with_relative_axes(&axes)?
            .build()?;

        tracing::info!(invert, "thumb-wheel forwarder: uinput device created");
        Ok(Self {
            device,
            invert,
            hires_accum: 0,
        })
    }

    pub fn invert(&self) -> bool {
        self.invert
    }

    pub fn set_invert(&mut self, invert: bool) {
        if self.invert != invert {
            self.invert = invert;
            self.hires_accum = 0;
            tracing::info!(invert, "thumb-wheel forwarder: invert flipped");
        }
    }

    /// Re-emit a signed displacement onto the uinput device.
    ///
    /// `raw` is the i16 displacement straight out of the 0x2150
    /// notification (bytes [4..6], big-endian). Negative = wheel
    /// rolled "left" from the user's POV (toward the index finger),
    /// positive = rolled "right".
    pub fn emit_displacement(&mut self, raw: i16) {
        if raw == 0 {
            return;
        }
        let signed = i32::from(raw);
        let value = if self.invert { -signed } else { signed };

        // Hi-res tick is always emitted.
        let mut events = [
            InputEvent::new(
                EventType::RELATIVE.0,
                RelativeAxisCode::REL_HWHEEL_HI_RES.0,
                value,
            ),
            // Detent event slot, filled in only when a full notch
            // accumulates. emit() ignores a no-op event but we'd
            // rather not send one at all.
            InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_HWHEEL.0, 0),
        ];

        self.hires_accum += value;
        let detents = self.hires_accum / TICKS_PER_DETENT;
        let slice: &[InputEvent] = if detents != 0 {
            self.hires_accum -= detents * TICKS_PER_DETENT;
            events[1] = InputEvent::new(
                EventType::RELATIVE.0,
                RelativeAxisCode::REL_HWHEEL.0,
                detents,
            );
            &events[..]
        } else {
            &events[..1]
        };

        if let Err(e) = self.device.emit(slice) {
            tracing::warn!(error = %e, "thumb-wheel forwarder: emit failed");
        } else {
            tracing::trace!(
                raw,
                value,
                accum = self.hires_accum,
                detents,
                "thumb-wheel forwarder: displacement emitted"
            );
        }
    }
}

impl Drop for ThumbWheelForwarder {
    fn drop(&mut self) {
        tracing::info!("thumb-wheel forwarder: uinput device torn down");
    }
}

/// Shared state between the D-Bus interface (which controls
/// activation) and the hidraw read loop (which dispatches incoming
/// notifications). Kept narrow so neither side has to take the
/// HapticManager mutex on every wheel tick.
pub struct ThumbWheelState {
    /// HID++ feature index for THUMB_WHEEL (0x2150) on the connected
    /// device, populated after feature discovery. `None` when no
    /// MX Master 4 is connected or the device lacks the feature.
    pub feature_index: Option<u8>,
    /// Active forwarder when the user has enabled inversion. `None`
    /// means the wheel is in normal kernel-managed mode — no
    /// notifications are expected.
    pub forwarder: Option<ThumbWheelForwarder>,
}

impl ThumbWheelState {
    pub const fn new() -> Self {
        Self {
            feature_index: None,
            forwarder: None,
        }
    }
}

impl Default for ThumbWheelState {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedThumbWheelState = Arc<Mutex<ThumbWheelState>>;

pub fn new_shared_state() -> SharedThumbWheelState {
    Arc::new(Mutex::new(ThumbWheelState::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_state_is_send_sync() {
        // The hidraw loop and D-Bus interface live on different
        // tasks; the type must be safe to share.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SharedThumbWheelState>();
    }

    #[test]
    fn defaults_to_inert() {
        let state = ThumbWheelState::new();
        assert!(state.feature_index.is_none());
        assert!(state.forwarder.is_none());
    }
}
