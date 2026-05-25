//! Async D-Bus client for `org.juhradial.Daemon` (juhradiald).
//!
//! Wraps the daemon's HID++ surface so the settings UI can read
//! live device state (DPI, battery, host slots, real device name)
//! and call writes (SetDpi, SetHost) directly. All methods return
//! `Option<T>` instead of erroring — when the daemon isn't running
//! or doesn't reply, we just show "—" / disabled controls in the
//! UI rather than spamming errors.

use zbus::{proxy, Connection};

const DAEMON_BUS: &str = "org.juhradial.Daemon";
const DAEMON_PATH: &str = "/org/juhradial/Daemon";

#[proxy(
    interface = "org.juhradial.Daemon",
    default_service = "org.juhradial.Daemon",
    default_path = "/org/juhradial/Daemon"
)]
trait Daemon {
    fn get_battery_status(&self) -> zbus::Result<(u8, bool)>;
    fn get_device_name(&self) -> zbus::Result<String>;
    fn get_dpi(&self) -> zbus::Result<u16>;
    fn set_dpi(&self, dpi: u16) -> zbus::Result<()>;
    fn dpi_supported(&self) -> zbus::Result<bool>;
    fn get_smart_shift(&self) -> zbus::Result<(bool, u8)>;
    fn set_smart_shift(&self, enabled: bool, threshold: u8) -> zbus::Result<()>;
    fn smart_shift_supported(&self) -> zbus::Result<bool>;
    /// Atomic 3-state wheel-mode setter — `mode` ∈ {"free",
    /// "ratchet", "smartshift"}. Settings calls this directly on
    /// every picker change so the device updates without waiting
    /// for ReloadConfig.
    fn set_wheel_mode(&self, mode: &str, threshold: u8) -> zbus::Result<()>;
    /// Read the current device-side wheel mode + threshold so the
    /// picker can reflect what the mouse actually has after a
    /// SmartShift-button press changes it on the hardware side.
    fn get_wheel_mode(&self) -> zbus::Result<(String, u8)>;
    /// Toggle the ThumbWheel side-scroll direction inversion
    /// (HID++ 0x2150). Applies live to the device.
    fn set_thumb_wheel_invert(&self, invert: bool) -> zbus::Result<()>;
    /// Read the device-side ThumbWheel `(divert, invert)` flags.
    /// Used both as a diagnostic and so the toggle reflects any
    /// state changed outside our control.
    fn get_thumb_wheel_status(&self) -> zbus::Result<(bool, bool)>;
    /// Whether the device exposes ThumbWheel — settings UI hides
    /// the horizontal-scroll-reverse toggle when false.
    fn thumb_wheel_supported(&self) -> zbus::Result<bool>;
    fn get_hiresscroll_mode(&self) -> zbus::Result<(bool, bool, bool)>;
    fn set_hiresscroll_mode(
        &self,
        hires: bool,
        invert: bool,
        target: bool,
    ) -> zbus::Result<()>;
    fn get_host_names(&self) -> zbus::Result<Vec<String>>;
    fn get_easy_switch_info(&self) -> zbus::Result<(u8, u8)>;
    fn set_host(&self, host_index: u8) -> zbus::Result<bool>;

    // Macros
    fn start_macro_recording(&self) -> zbus::Result<()>;
    fn stop_macro_recording(&self) -> zbus::Result<String>;
    fn save_macro(&self, json: String) -> zbus::Result<()>;
    fn is_macro_running(&self) -> zbus::Result<bool>;

    // Gaming
    fn get_gaming_mode(&self) -> zbus::Result<bool>;
    fn set_gaming_mode(&self, enabled: bool) -> zbus::Result<()>;
    fn cycle_gaming_dpi(&self) -> zbus::Result<String>;
    fn test_haptic_redirect(&self) -> zbus::Result<()>;
    fn diagnose_haptic_redirect(&self) -> zbus::Result<String>;

    // Haptics — fires the per-event pattern through the device so
    // the user can preview a configured pattern from the settings
    // window without opening the radial.
    fn trigger_haptic(&self, event: &str) -> zbus::Result<()>;

    // Slice-action dispatch (mirrors overlay's haptic_client
    // proxy). Used by the settings "Test" button to fire the
    // exact same code path the radial menu uses, so what the user
    // sees in the test matches what they get on a real activation.
    fn execute_shortcut(&self, keys: &str) -> zbus::Result<()>;
    fn execute_macro(&self, id: &str) -> zbus::Result<()>;
    fn show_menu_at_cursor(&self, x: i32, y: i32) -> zbus::Result<()>;

    // Tell the daemon to re-read `~/.config/juhradial/config.json`
    // from disk and re-apply the changed sections (button diverts,
    // scroll/smartshift, pointer accel, haptic patterns). The
    // settings GUI calls this immediately after every successful
    // `persist::save` — without it the daemon keeps running with
    // stale config and button reassignments have no effect on the
    // device.
    fn reload_config(&self) -> zbus::Result<()>;
}

/// Tell the daemon to show the radial menu at the given screen
/// coordinate. Used by the settings UI's "Open radial menu" /
/// "Preview transition" buttons so the user can validate live
/// changes without lifting hands off the keyboard.
pub async fn show_radial_at(x: i32, y: i32) {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(_) => return,
    };
    let proxy = match DaemonProxy::builder(&conn).build().await {
        Ok(p) => p,
        Err(_) => return,
    };
    let _ = proxy.show_menu_at_cursor(x, y).await;
}

/// Fire a single haptic pulse through the daemon. Best-effort —
/// silently no-ops if the daemon isn't running or replies with an
/// error, so the settings UI's "Test" buttons never block on bus
/// roundtrip latency or surface scary toasts when the device is
/// disconnected.
pub async fn trigger_haptic_event(event: String) {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(_) => return,
    };
    let proxy =
        match DaemonProxy::builder(&conn).build().await {
            Ok(p) => p,
            Err(_) => return,
        };
    let _ = proxy.trigger_haptic(&event).await;
}

/// Trigger a recorded macro by id through the daemon's playback
/// engine. Same fire-and-forget shape as `trigger_haptic_event` so
/// the slice-test buttons can spawn it without awaiting.
pub async fn trigger_macro(id: String) {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(_) => return,
    };
    let proxy = match DaemonProxy::builder(&conn).build().await {
        Ok(p) => p,
        Err(_) => return,
    };
    let _ = proxy.execute_macro(&id).await;
}

/// Tell the daemon to re-read `~/.config/juhradial/config.json`
/// from disk and re-apply changed sections (button diverts,
/// scroll/smartshift, pointer accel, haptic patterns). The
/// settings GUI fires this after every successful `persist::save`
/// — without it, button reassignments persist to disk but never
/// reach the device because the daemon keeps running with the
/// config it loaded at startup.
///
/// Best-effort: silently no-ops when the daemon isn't running
/// (settings still works for offline editing). Logs at debug
/// level on success so an operator tailing the daemon log can
/// confirm reloads are flowing.
pub async fn request_reload() {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(_) => return,
    };
    let proxy = match DaemonProxy::builder(&conn).build().await {
        Ok(p) => p,
        Err(_) => return,
    };
    let _ = proxy.reload_config().await;
}

/// Synthesize a keyboard shortcut via the daemon's xdotool /
/// ydotool wrapper. `keys` is the chord in xdotool format
/// (`"ctrl+shift+v"`, `"super+e"`).
pub async fn trigger_shortcut(keys: String) {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(_) => return,
    };
    let proxy = match DaemonProxy::builder(&conn).build().await {
        Ok(p) => p,
        Err(_) => return,
    };
    let _ = proxy.execute_shortcut(&keys).await;
}

/// Switch the MX Master 4's active Easy-Switch host (1-3).
pub async fn trigger_set_host(host_index: u8) {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(_) => return,
    };
    let proxy = match DaemonProxy::builder(&conn).build().await {
        Ok(p) => p,
        Err(_) => return,
    };
    let _ = proxy.set_host(host_index).await;
}

/// Snapshot of everything the settings UI cares about. Single
/// `poll()` Task fills this in one batch so the iced model doesn't
/// have to track three independent timers.
#[derive(Debug, Clone, Default)]
pub struct DaemonSnapshot {
    pub battery: Option<(u8, bool)>,
    pub device_name: Option<String>,
    pub dpi: Option<u16>,
    pub dpi_supported: bool,
    pub easy_switch: Option<EasySwitch>,
    pub gaming_mode: bool,
    pub macro_recording: bool,
    /// Live HiResScroll mode triple (hires, invert, target).
    /// `None` when the feature isn't supported.
    pub hiresscroll: Option<HiResScroll>,
    /// Live wheel mode read from the device — slug is one of
    /// "freespin" / "ratchet" / "smartshift", threshold is the
    /// user-visible 1..100 value. `None` when the feature isn't
    /// supported. The picker uses this to reflect changes made
    /// via the SmartShift button on the mouse.
    pub wheel_mode: Option<(String, u8)>,
    /// Live ThumbWheel `(divert, invert)` flags. `None` when 0x2150
    /// isn't supported. Settings reflects `invert` in the toggle.
    pub thumb_wheel: Option<(bool, bool)>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HiResScroll {
    pub hires: bool,
    pub invert: bool,
    pub target: bool,
}

#[derive(Debug, Clone)]
pub struct EasySwitch {
    /// One label per host slot (length usually 3).
    pub host_names: Vec<String>,
    /// Total slot count reported by the device.
    pub slot_count: u8,
    /// Index of the slot the device is currently bonded to (0..slot_count-1).
    pub current_host: u8,
}

/// Async snapshot probe — opens a fresh connection (cheap, zbus
/// reuses the session bus internally), fires every read, returns
/// what we got. Anything that errors stays `None` in the snapshot.
pub async fn poll() -> DaemonSnapshot {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(_) => return DaemonSnapshot::default(),
    };
    let proxy = match DaemonProxy::new(&conn).await {
        Ok(p) => p,
        Err(_) => return DaemonSnapshot::default(),
    };

    let battery = proxy.get_battery_status().await.ok().and_then(|(p, c)| {
        if p == 0 {
            None
        } else {
            Some((p, c))
        }
    });
    let device_name = proxy
        .get_device_name()
        .await
        .ok()
        .filter(|s| !s.is_empty());

    let dpi_supported = proxy.dpi_supported().await.unwrap_or(false);
    let dpi = if dpi_supported {
        proxy.get_dpi().await.ok().filter(|&v| v > 0)
    } else {
        None
    };

    let easy_switch = match (
        proxy.get_easy_switch_info().await,
        proxy.get_host_names().await,
    ) {
        (Ok((slot_count, current_host)), Ok(host_names)) if slot_count > 0 => Some(EasySwitch {
            host_names,
            slot_count,
            current_host,
        }),
        _ => None,
    };

    let gaming_mode = proxy.get_gaming_mode().await.unwrap_or(false);
    let macro_recording = proxy.is_macro_running().await.unwrap_or(false);
    let hiresscroll = proxy
        .get_hiresscroll_mode()
        .await
        .ok()
        .map(|(hires, invert, target)| HiResScroll {
            hires,
            invert,
            target,
        });

    // Wheel mode: only meaningful when smartshift is supported. The
    // daemon returns ("ratchet", 0) as a sentinel when the feature
    // is missing, so gate on smart_shift_supported() rather than
    // matching that tuple (which is also a valid real value).
    let wheel_mode = if proxy.smart_shift_supported().await.unwrap_or(false) {
        proxy.get_wheel_mode().await.ok()
    } else {
        None
    };

    let thumb_wheel = if proxy.thumb_wheel_supported().await.unwrap_or(false) {
        proxy.get_thumb_wheel_status().await.ok()
    } else {
        None
    };

    DaemonSnapshot {
        battery,
        device_name,
        dpi,
        dpi_supported,
        easy_switch,
        gaming_mode,
        macro_recording,
        hiresscroll,
        wheel_mode,
        thumb_wheel,
    }
}

/// Fire-and-forget DPI setter. Returns `Ok(())` on success; the
/// caller can map errors to status text in the UI.
pub async fn set_dpi(dpi: u16) -> Result<(), String> {
    call_with_proxy(|p| async move { p.set_dpi(dpi).await }).await
}

/// Fire the daemon's atomic 3-state wheel-mode setter. `mode` is
/// the slug from settings ("free" / "ratchet" / "smartshift").
/// `threshold` is the 1..100 value the user dialed; only
/// meaningful for the "smartshift" mode but passed through
/// regardless. Returns Ok(()) on success.
pub async fn set_wheel_mode(mode: String, threshold: u8) -> Result<(), String> {
    call_with_proxy(move |p| {
        let mode = mode.clone();
        async move { p.set_wheel_mode(&mode, threshold).await }
    })
    .await
}

/// Fire the daemon's ThumbWheel inversion setter. Applies to the
/// device immediately via HID++ 0x2150.
pub async fn set_thumb_wheel_invert(invert: bool) -> Result<(), String> {
    call_with_proxy(move |p| async move { p.set_thumb_wheel_invert(invert).await }).await
}

/// Switch the active Easy-Switch host. Returns Ok on success.
pub async fn set_host(idx: u8) -> Result<(), String> {
    call_with_proxy(|p| async move {
        let _ = p.set_host(idx).await?;
        Ok(())
    })
    .await
}

/// Toggle gaming mode on/off via the daemon.
pub async fn set_gaming_mode(enabled: bool) -> Result<(), String> {
    call_with_proxy(|p| async move { p.set_gaming_mode(enabled).await }).await
}

/// Cycle the gaming DPI preset. Returns the daemon's reported new
/// DPI label (e.g. "1600 DPI") or empty string on failure.
pub async fn cycle_gaming_dpi() -> Result<String, String> {
    let conn = Connection::session()
        .await
        .map_err(|e| format!("session bus: {e}"))?;
    let proxy = DaemonProxy::new(&conn)
        .await
        .map_err(|e| format!("daemon proxy: {e}"))?;
    proxy
        .cycle_gaming_dpi()
        .await
        .map_err(|e| format!("daemon call: {e}"))
}

/// Fire a one-shot haptic test pulse through the daemon's gamepad-
/// rumble → haptic translator.
pub async fn test_haptic_redirect() -> Result<(), String> {
    call_with_proxy(|p| async move { p.test_haptic_redirect().await }).await
}

/// Ask the daemon for a diagnostic report on the gamepad-rumble →
/// haptic bridge. Returns the human-readable report text.
pub async fn diagnose_haptic_redirect() -> Result<String, String> {
    let conn = Connection::session()
        .await
        .map_err(|e| format!("session bus: {e}"))?;
    let proxy = DaemonProxy::new(&conn)
        .await
        .map_err(|e| format!("daemon proxy: {e}"))?;
    proxy
        .diagnose_haptic_redirect()
        .await
        .map_err(|e| format!("daemon call: {e}"))
}

/// Tell the daemon to start capturing keyboard / mouse events.
/// The daemon's recorder buffers them until `stop_macro_recording`
/// is called.
pub async fn start_macro_recording() -> Result<(), String> {
    call_with_proxy(|p| async move { p.start_macro_recording().await }).await
}

/// Stop the recorder and return the captured event/action stream.
/// The result is the daemon's JSON of `{events, actions}`; the
/// caller wraps it into a `MacroConfig` before saving.
pub async fn stop_macro_recording() -> Result<String, String> {
    let conn = Connection::session()
        .await
        .map_err(|e| format!("session bus: {e}"))?;
    let proxy = DaemonProxy::new(&conn)
        .await
        .map_err(|e| format!("daemon proxy: {e}"))?;
    proxy
        .stop_macro_recording()
        .await
        .map_err(|e| format!("daemon call: {e}"))
}

/// Persist a `MacroConfig`-shaped JSON to the macros directory via
/// the daemon. The daemon validates + writes atomically so the UI
/// doesn't need filesystem permissions.
pub async fn save_macro(json: String) -> Result<(), String> {
    call_with_proxy(|p| async move { p.save_macro(json).await }).await
}

/// Apply a HiResScroll mode triple directly. Bypasses the config
/// reload flow — useful for instant device feedback when the user
/// flips a per-knob toggle. Idempotent on the daemon side.
pub async fn set_hiresscroll(hires: bool, invert: bool, target: bool) -> Result<(), String> {
    call_with_proxy(|p| async move { p.set_hiresscroll_mode(hires, invert, target).await })
        .await
}

async fn call_with_proxy<F, Fut>(f: F) -> Result<(), String>
where
    F: FnOnce(DaemonProxy<'static>) -> Fut,
    Fut: std::future::Future<Output = zbus::Result<()>>,
{
    let conn = Connection::session()
        .await
        .map_err(|e| format!("session bus: {e}"))?;
    let proxy = DaemonProxy::new(&conn)
        .await
        .map_err(|e| format!("daemon proxy: {e}"))?;
    let _ = (DAEMON_BUS, DAEMON_PATH); // satisfy dead-code; kept for docs
    f(proxy).await.map_err(|e| format!("daemon call: {e}"))
}
