//! Async D-Bus client for `org.kde.juhradialmx` (juhradiald).
//!
//! Wraps the daemon's HID++ surface so the settings UI can read
//! live device state (DPI, battery, host slots, real device name)
//! and call writes (SetDpi, SetHost) directly. All methods return
//! `Option<T>` instead of erroring — when the daemon isn't running
//! or doesn't reply, we just show "—" / disabled controls in the
//! UI rather than spamming errors.

use zbus::{proxy, Connection};

const DAEMON_BUS: &str = "org.kde.juhradialmx";
const DAEMON_PATH: &str = "/org/kde/juhradialmx/Daemon";

#[proxy(
    interface = "org.kde.juhradialmx.Daemon",
    default_service = "org.kde.juhradialmx",
    default_path = "/org/kde/juhradialmx/Daemon"
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

    DaemonSnapshot {
        battery,
        device_name,
        dpi,
        dpi_supported,
        easy_switch,
        gaming_mode,
        macro_recording,
        hiresscroll,
    }
}

/// Fire-and-forget DPI setter. Returns `Ok(())` on success; the
/// caller can map errors to status text in the UI.
pub async fn set_dpi(dpi: u16) -> Result<(), String> {
    call_with_proxy(|p| async move { p.set_dpi(dpi).await }).await
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
