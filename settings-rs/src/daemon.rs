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
    fn get_host_names(&self) -> zbus::Result<Vec<String>>;
    fn get_easy_switch_info(&self) -> zbus::Result<(u8, u8)>;
    fn set_host(&self, host_index: u8) -> zbus::Result<bool>;
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

    DaemonSnapshot {
        battery,
        device_name,
        dpi,
        dpi_supported,
        easy_switch,
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
