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

const DAEMON_SERVICE: &str = "org.oxidemx.Daemon";
const DAEMON_PATH: &str = "/org/oxidemx/Daemon";

#[proxy(
    interface = "org.oxidemx.Daemon",
    default_service = "org.oxidemx.Daemon",
    default_path = "/org/oxidemx/Daemon"
)]
trait Haptic {
    fn notify_slice_hover(&self, index: u8) -> zbus::Result<()>;
    fn trigger_haptic(&self, event: &str) -> zbus::Result<()>;
    fn execute_shortcut(&self, keys: &str) -> zbus::Result<()>;
    fn execute_macro(&self, id: &str) -> zbus::Result<()>;
    fn set_host(&self, host_index: u8) -> zbus::Result<bool>;
    fn set_dpi(&self, dpi: u16) -> zbus::Result<()>;
    fn get_wheel_mode(&self) -> zbus::Result<(String, u8)>;
    fn set_wheel_mode(&self, mode: &str, threshold: u8) -> zbus::Result<()>;
    fn set_haptics_enabled(&self, enabled: bool) -> zbus::Result<()>;
    fn get_gaming_mode(&self) -> zbus::Result<bool>;
    fn set_gaming_mode(&self, enabled: bool) -> zbus::Result<()>;
    fn get_battery_status(&self) -> zbus::Result<(u8, bool)>;
    #[zbus(property)]
    fn haptics_enabled(&self) -> zbus::Result<bool>;
}

/// Tell the daemon a new slice slot is hovered. Kept around as a
/// well-tested entry point even though the overlay now drives
/// slice-change haptics through `trigger_haptic` directly — leaving
/// this in place means daemon-side debounce still works for any
/// future caller (e.g. an external IPC consumer or a unit test
/// exercising the daemon's hover-debounce path).
#[allow(dead_code)]
pub async fn notify_slice_hover(index: u8) {
    if let Err(e) = try_notify(index).await {
        warn!(
            "NotifySliceHover D-Bus call failed: {e} \
             (daemon {DAEMON_SERVICE} not running?)"
        );
    }
}

#[allow(dead_code)]
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

/// Synthesize a keyboard shortcut into the focused window via the
/// daemon's xdotool/ydotool wrapper. Format mirrors xdotool's
/// `key` argument: `"ctrl+c"`, `"ctrl+shift+z"`, `"super+e"`.
/// Used when a slice with `kind = Shortcut` dispatches.
pub fn execute_shortcut_blocking(keys: String) {
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => return,
    };
    rt.spawn(async move {
        if let Err(e) = try_execute_shortcut(&keys).await {
            warn!(
                "ExecuteShortcut({keys}) D-Bus call failed: {e} \
                 (daemon {DAEMON_SERVICE} not running?)"
            );
        }
    });
}

async fn try_execute_shortcut(keys: &str) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    let proxy = HapticProxy::new(&conn).await?;
    proxy.execute_shortcut(keys).await
}

/// Trigger a recorded macro by its id. Daemon loads the macro from
/// disk and replays it through its evdev/uinput sender.
pub fn execute_macro_blocking(id: String) {
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => return,
    };
    rt.spawn(async move {
        if let Err(e) = try_execute_macro(&id).await {
            warn!(
                "ExecuteMacro({id}) D-Bus call failed: {e} \
                 (daemon {DAEMON_SERVICE} not running?)"
            );
        }
    });
}

async fn try_execute_macro(id: &str) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    let proxy = HapticProxy::new(&conn).await?;
    proxy.execute_macro(id).await
}

/// Switch the MX Master 4's active Easy-Switch host. `host_index`
/// is 1-based on the mouse (1, 2, 3 — same numbers printed under
/// each button). Daemon validates + sends the HID++ host-switch
/// command and the device reconnects to the picked host.
pub fn set_host_blocking(host_index: u8) {
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => return,
    };
    rt.spawn(async move {
        if let Err(e) = try_set_host(host_index).await {
            warn!(
                "SetHost({host_index}) D-Bus call failed: {e} \
                 (daemon {DAEMON_SERVICE} not running?)"
            );
        }
    });
}

async fn try_set_host(host_index: u8) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    let proxy = HapticProxy::new(&conn).await?;
    let _ = proxy.set_host(host_index).await?;
    Ok(())
}

/// Apply a mouse quick setting from a `MouseSetting` slice. The
/// `setting` string is the slice's `command`:
///
///   * `dpi:<value>` — set sensor DPI
///   * `smartshift` — toggle the scroll wheel between smartshift
///     and freespin (threshold preserved by the daemon)
///   * `haptics` — toggle haptic feedback on/off
///   * `gaming` — toggle gaming mode
///
/// Toggles read current state first; fire-and-forget like every
/// other daemon call here.
pub fn mouse_setting_blocking(setting: String) {
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => return,
    };
    rt.spawn(async move {
        if let Err(e) = try_mouse_setting(&setting).await {
            warn!(
                "MouseSetting({setting}) D-Bus call failed: {e} \
                 (daemon {DAEMON_SERVICE} not running?)"
            );
        }
    });
}

async fn try_mouse_setting(setting: &str) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    let proxy = HapticProxy::new(&conn).await?;
    if let Some(dpi) = setting.strip_prefix("dpi:") {
        let dpi: u16 = dpi
            .trim()
            .parse()
            .map_err(|_| zbus::Error::Failure(format!("bad dpi value in {setting:?}")))?;
        return proxy.set_dpi(dpi).await;
    }
    match setting {
        "smartshift" => {
            let (mode, threshold) = proxy.get_wheel_mode().await?;
            let next = if mode == "freespin" {
                "smartshift"
            } else {
                "freespin"
            };
            proxy.set_wheel_mode(next, threshold).await
        }
        "haptics" => {
            let on = proxy.haptics_enabled().await.unwrap_or(true);
            proxy.set_haptics_enabled(!on).await
        }
        "gaming" => {
            let on = proxy.get_gaming_mode().await?;
            proxy.set_gaming_mode(!on).await
        }
        other => Err(zbus::Error::Failure(format!(
            "unknown mouse setting {other:?} (want dpi:<n>|smartshift|haptics|gaming)"
        ))),
    }
}

/// One-shot battery read for the MouseBattery widget. Returns
/// `None` when the daemon isn't reachable.
pub async fn battery_status() -> Option<(u8, bool)> {
    let conn = Connection::session().await.ok()?;
    let proxy = HapticProxy::new(&conn).await.ok()?;
    proxy.get_battery_status().await.ok()
}

#[allow(dead_code)]
const _: &str = DAEMON_PATH; // retain in case of doc-only refs
