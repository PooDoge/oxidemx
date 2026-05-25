//! Proxy mode — wrap a real game controller.
//!
//! In `Proxy` mode the daemon discovers a physical gamepad, grabs
//! it exclusively (`EVIOCGRAB`) so the desktop and games can no
//! longer read it, and forwards its input through the virtual pad.
//! The game therefore sees a single controller — ours — and the
//! rumble it sends lands on the bridge. The user keeps playing with
//! their real controller; only the *event path* is rerouted.
//!
//! See `HAPTIC_GAMEPAD_BRIDGE_DESIGN.md` §8.2.
//!
//! ## Phase status
//!
//! **Phase 6 (this commit):** discovery + grab + input forwarding.
//! The virtual pad keeps its fixed Xbox-360 capability set rather
//! than mirroring the real controller's — perfect for an Xbox pad,
//! good enough for any standard gamepad, but exotic controls (e.g.
//! a DualSense touchpad) won't forward. Capability mirroring,
//! multi-controller selection (`preferred_controller_guid`), rumble
//! passthrough, and the hard-hide udev rule are later refinements.

use evdev::{AbsoluteAxisCode, Device, KeyCode};
use juhradial_shared::HapticRedirectConfig;

/// Discover a real gamepad to proxy, open it, and grab it
/// exclusively with `EVIOCGRAB`.
///
/// Returns `None` when no gamepad is connected — the caller then
/// falls back to standalone mode. The grab is released
/// automatically when the returned [`Device`] is dropped (the
/// kernel drops the grab on `close`), so teardown needs no explicit
/// ungrab.
pub fn discover_controller(config: &HapticRedirectConfig) -> Option<Device> {
    if config.preferred_controller_guid.is_some() {
        tracing::info!(
            "haptic redirect: preferred-controller selection is not implemented \
             yet — proxying the first gamepad found"
        );
    }

    // Scan /dev/input/event* in a stable order.
    let mut paths: Vec<_> = std::fs::read_dir("/dev/input")
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("event"))
        })
        .collect();
    paths.sort();

    for path in paths {
        let mut device = match Device::open(&path) {
            Ok(device) => device,
            Err(_) => continue, // permission / transient — skip quietly
        };

        // Never proxy our own virtual pad.
        if device.name() == Some(super::virtual_pad::NAME) {
            continue;
        }
        if !is_gamepad(&device) {
            continue;
        }

        match device.grab() {
            Ok(()) => {
                tracing::info!(
                    path = %path.display(),
                    name = device.name().unwrap_or("<unknown>"),
                    "haptic redirect: proxying controller"
                );
                return Some(device);
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "haptic redirect: found a controller but could not grab it \
                     (already grabbed?) — skipping"
                );
            }
        }
    }

    tracing::info!("haptic redirect: no gamepad found to proxy");
    None
}

/// A device is a gamepad if it carries the standard gamepad button
/// (`BTN_SOUTH` == `BTN_GAMEPAD`) *and* at least one analogue axis
/// — the same signature udev's `input_id` builtin keys
/// `ID_INPUT_JOYSTICK` off. Mice (`EV_REL`, no `ABS_X`) and
/// keyboards (no `BTN_GAMEPAD`) fail one half each.
pub(super) fn is_gamepad(device: &Device) -> bool {
    let has_gamepad_button = device
        .supported_keys()
        .is_some_and(|keys| keys.contains(KeyCode::BTN_SOUTH));
    let has_stick = device
        .supported_absolute_axes()
        .is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_X));
    has_gamepad_button && has_stick
}

#[cfg(test)]
mod tests {
    /// Discovery + grab exercised against the live `/dev/input`
    /// tree. Ignored by default — the result depends on whether a
    /// controller happens to be plugged in, and grabbing one
    /// briefly steals it from the desktop. Run with:
    ///
    /// ```text
    /// cargo test -p juhradiald --lib -- --ignored --nocapture proxy_discovery
    /// ```
    #[test]
    #[ignore = "scans /dev/input and briefly grabs any real controller"]
    fn proxy_discovery_smoke() {
        use super::discover_controller;
        use juhradial_shared::HapticRedirectConfig;

        match discover_controller(&HapticRedirectConfig::default()) {
            Some(device) => {
                eprintln!(
                    "discovered + grabbed controller: {}",
                    device.name().unwrap_or("<unknown>")
                );
                // Dropping `device` here releases the grab.
            }
            None => eprintln!("no controller connected — discovery returned None"),
        }
    }
}
