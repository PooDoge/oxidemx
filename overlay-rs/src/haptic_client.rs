//! Async D-Bus client for the daemon's haptic surface.
//!
//! The daemon owns the device — it talks HID++ feature 0x19B0 to
//! pulse the MX Master 4's haptic motor. The overlay knows when
//! the user is hovering a different slice (drag-mode cursor delta
//! or toggle-mode mouse move). Bridging the two: the overlay
//! fires `NotifySliceHover(u8)` whenever its `target_slice`
//! changes, and `TriggerHaptic("menu_appear" | "confirm" |
//! "invalid")` for one-off events.
//!
//! Fire-and-forget by design — if the daemon isn't running, the
//! menu still works (visually); haptics just stay silent. No
//! retry loop, no error surface.

use tracing::warn;
use zbus::{proxy, Connection};

const DAEMON_SERVICE: &str = "org.kde.juhradialmx";
const DAEMON_PATH: &str = "/org/kde/juhradialmx/Daemon";

#[proxy(
    interface = "org.kde.juhradialmx.Daemon",
    default_service = "org.kde.juhradialmx",
    default_path = "/org/kde/juhradialmx/Daemon"
)]
trait Haptic {
    fn notify_slice_hover(&self, index: u8) -> zbus::Result<()>;
    fn trigger_haptic(&self, event: &str) -> zbus::Result<()>;
}

/// Tell the daemon a new slice slot is hovered. The daemon's
/// `notify_slice_hover` handler debounces internally + fires the
/// configured `slice_change` haptic pattern. Index is 0..7 for
/// the eight ring slots.
pub async fn notify_slice_hover(index: u8) {
    if let Err(e) = try_notify(index).await {
        warn!(
            "NotifySliceHover D-Bus call failed: {e} \
             (daemon {DAEMON_SERVICE} not running?)"
        );
    }
}

async fn try_notify(index: u8) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    let proxy = HapticProxy::new(&conn).await?;
    proxy.notify_slice_hover(index).await
}

/// Fire a one-off haptic event by name. Daemon recognises:
///
///   * `menu_appear` — fired on Show
///   * `slice_change` — fired by NotifySliceHover (don't call
///     this directly; use `notify_slice_hover` instead so the
///     daemon can debounce)
///   * `confirm` — fired on dispatch (slice action triggered)
///   * `invalid` — fired on dismiss without action
pub async fn trigger_haptic(event: String) {
    if let Err(e) = try_trigger(&event).await {
        warn!(
            "TriggerHaptic({event}) D-Bus call failed: {e} \
             (daemon {DAEMON_SERVICE} not running?)"
        );
    }
}

async fn try_trigger(event: &str) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    let proxy = HapticProxy::new(&conn).await?;
    proxy.trigger_haptic(event).await
}

#[allow(dead_code)]
const _: &str = DAEMON_PATH; // retain in case of doc-only refs
