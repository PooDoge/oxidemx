//! Quick-toggle and quick-slider actions → daemon D-Bus method calls.
//!
//! Each `Action` variant maps to one or more method calls on
//! `org.juhradial.Daemon`. The proxy is defined inline via `#[zbus::proxy]`.
//! All calls are fire-and-forget: errors are logged but not propagated so
//! a transient D-Bus hiccup doesn't crash the popup.

use tracing::{info, warn};
use zbus::{proxy, Connection};

// ---------------------------------------------------------------------------
// D-Bus proxy
// ---------------------------------------------------------------------------

#[proxy(
    interface = "org.juhradial.Daemon",
    default_service = "org.juhradial.Daemon",
    default_path = "/org/juhradial/Daemon"
)]
trait Daemon {
    /// Set gaming-mode on/off. Bumps DPI and hides the radial when on.
    fn set_gaming_mode(&self, enabled: bool) -> zbus::Result<()>;

    /// Enable / disable haptic feedback.
    fn set_haptics_enabled(&self, enabled: bool) -> zbus::Result<()>;

    /// Show / hide the radial overlay.
    fn set_radial_enabled(&self, enabled: bool) -> zbus::Result<()>;

    /// Enable / disable SmartShift (free-spin scroll).
    fn set_smart_shift(&self, enabled: bool) -> zbus::Result<()>;

    /// Set pointer DPI (200 – 6400).
    fn set_dpi(&self, dpi: u16) -> zbus::Result<()>;

    /// Set scroll sensitivity (1 – 10).
    fn set_scroll_sensitivity(&self, level: u8) -> zbus::Result<()>;

    /// Set haptic intensity (0 = off, higher = stronger).
    fn set_haptic_intensity(&self, level: u8) -> zbus::Result<()>;

    /// Set pointer acceleration (clamped to -1.0 – 1.0 by the daemon).
    fn set_pointer_accel(&self, accel: f64) -> zbus::Result<()>;

    /// Switch the active Easy-Switch host (0-indexed, 0–2).
    fn switch_easy_switch_host(&self, host: u8) -> zbus::Result<()>;

    /// Fetch the current active device state.
    /// Returns: (battery_pct, charging, connected, device_name, connection_type, firmware_version)
    fn get_active_device_state(&self) -> zbus::Result<(u8, bool, bool, String, String, String)>;
}

// ---------------------------------------------------------------------------
// Action enum
// ---------------------------------------------------------------------------

/// An action the popup can dispatch to the daemon.
#[derive(Debug, Clone)]
pub enum Action {
    Gaming(bool),
    Haptics(bool),
    Radial(bool),
    Smart(bool),
    /// Flow cross-device scroll — not yet wired on the daemon side.
    Flow(bool),
    /// Cursor highlight — not yet wired on the daemon side.
    Highlight(bool),
    Dpi(u16),
    Scroll(u8),
    HapticIntensity(u8),
    Accel(f32),
    EasySwitch(u8),
}

/// Dispatch an action to the daemon. Returns `Ok(())` even for
/// unimplemented variants (Flow, Highlight) — they log a warning
/// instead of returning an error so the caller treats them identically.
pub async fn apply(action: Action) -> Result<(), String> {
    match action {
        Action::Flow(_) => {
            warn!("Action::Flow is not yet wired on the daemon — skipping");
            return Ok(());
        }
        Action::Highlight(_) => {
            warn!("Action::Highlight is not yet wired on the daemon — skipping");
            return Ok(());
        }
        _ => {}
    }

    let conn = Connection::session()
        .await
        .map_err(|e| format!("D-Bus session connect: {e}"))?;
    let proxy = DaemonProxy::new(&conn)
        .await
        .map_err(|e| format!("DaemonProxy::new: {e}"))?;

    let result: zbus::Result<()> = match action {
        Action::Gaming(v) => {
            info!(enabled = v, "dispatch: set_gaming_mode");
            proxy.set_gaming_mode(v).await
        }
        Action::Haptics(v) => {
            info!(enabled = v, "dispatch: set_haptics_enabled");
            proxy.set_haptics_enabled(v).await
        }
        Action::Radial(v) => {
            info!(enabled = v, "dispatch: set_radial_enabled");
            proxy.set_radial_enabled(v).await
        }
        Action::Smart(v) => {
            info!(enabled = v, "dispatch: set_smart_shift");
            proxy.set_smart_shift(v).await
        }
        Action::Dpi(dpi) => {
            info!(dpi, "dispatch: set_dpi");
            proxy.set_dpi(dpi).await
        }
        Action::Scroll(level) => {
            info!(level, "dispatch: set_scroll_sensitivity");
            proxy.set_scroll_sensitivity(level).await
        }
        Action::HapticIntensity(level) => {
            info!(level, "dispatch: set_haptic_intensity");
            proxy.set_haptic_intensity(level).await
        }
        Action::Accel(accel) => {
            info!(accel, "dispatch: set_pointer_accel");
            proxy.set_pointer_accel(accel as f64).await
        }
        Action::EasySwitch(host) => {
            info!(host, "dispatch: switch_easy_switch_host");
            proxy.switch_easy_switch_host(host).await
        }
        // Already handled above.
        Action::Flow(_) | Action::Highlight(_) => unreachable!(),
    };

    result.map_err(|e| format!("daemon D-Bus call failed: {e}"))
}

/// Fetch the current device state from the daemon.
/// Returns `None` if the daemon is not reachable.
pub async fn fetch_device_state() -> Option<DeviceState> {
    let conn = Connection::session().await.ok()?;
    let proxy = DaemonProxy::new(&conn).await.ok()?;
    match proxy.get_active_device_state().await {
        Ok((battery_pct, charging, connected, device_name, connection_type, _firmware)) => {
            Some(DeviceState {
                battery_pct,
                charging,
                connected,
                device_name,
                connection_type,
            })
        }
        Err(e) => {
            warn!("fetch_device_state: {e}");
            None
        }
    }
}

/// Device state snapshot from the daemon.
#[derive(Debug, Clone)]
pub struct DeviceState {
    pub battery_pct: u8,
    pub charging: bool,
    pub connected: bool,
    pub device_name: String,
    pub connection_type: String,
}
