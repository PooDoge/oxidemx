//! Quick-toggle and quick-slider actions → daemon D-Bus method calls.
//!
//! Each `Action` variant maps to one or more method calls on
//! `org.juhradial.Daemon`. The proxy is defined inline via `#[zbus::proxy]`.
//! All calls are fire-and-forget: errors are logged but not propagated so
//! a transient D-Bus hiccup doesn't crash the popup.

use tokio::sync::OnceCell;
use tracing::{info, warn};
use zbus::{proxy, Connection};

// ---------------------------------------------------------------------------
// Cached session connection — constructed on first use, reused thereafter.
// ---------------------------------------------------------------------------

static CONN: OnceCell<Connection> = OnceCell::const_new();

async fn session_conn() -> Result<&'static Connection, String> {
    CONN.get_or_try_init(|| async {
        Connection::session()
            .await
            .map_err(|e| format!("D-Bus session connect: {e}"))
    })
    .await
}

// ---------------------------------------------------------------------------
// D-Bus proxy — mirrors only the methods the daemon actually exposes.
//
// Source of truth: daemon/src/dbus/interface.rs
// Only list methods this popup actually calls; the proxy is not a full mirror.
// ---------------------------------------------------------------------------

#[proxy(
    interface = "org.juhradial.Daemon",
    default_service = "org.juhradial.Daemon",
    default_path = "/org/juhradial/Daemon"
)]
trait Daemon {
    /// Set gaming-mode on/off. Bumps DPI and hides the radial when on.
    fn set_gaming_mode(&self, enabled: bool) -> zbus::Result<()>;

    /// Toggle haptic feedback globally. Mutates config.haptics.enabled in
    /// the daemon and persists to disk so the change survives restarts.
    fn set_haptics_enabled(&self, enabled: bool) -> zbus::Result<()>;

    /// Enable / disable SmartShift (free-spin scroll).
    /// `threshold` is the torque percentage at which ratchet engages (1–100).
    /// When the popup toggle only signals on/off, pass 30 as the default
    /// threshold — this matches the daemon's own default in config::defaults.
    fn set_smart_shift(&self, enabled: bool, threshold: u8) -> zbus::Result<()>;

    /// Set pointer DPI (200 – 6400).
    fn set_dpi(&self, dpi: u16) -> zbus::Result<()>;

    /// Switch the active Easy-Switch host (0-indexed, 0–2).
    /// Returns true when the host switch was accepted by the device.
    fn set_host(&self, host_index: u8) -> zbus::Result<bool>;

    /// Fetch the current active device state.
    /// Returns: (battery_pct, charging, connection, device_name, device_id)
    fn get_active_device_state(&self) -> zbus::Result<(u8, bool, String, String, String)>;
}

// ---------------------------------------------------------------------------
// Action enum
// ---------------------------------------------------------------------------

/// An action the popup can dispatch to the daemon.
#[derive(Debug, Clone)]
pub enum Action {
    Gaming(bool),
    /// Haptic feedback toggle. Calls set_haptics_enabled which mutates
    /// config.haptics.enabled in the daemon and persists to disk.
    Haptics(bool),
    /// Radial overlay toggle — not yet wired on the daemon side (Phase 3.5).
    Radial(bool),
    Smart(bool),
    /// Flow cross-device scroll — not yet wired on the daemon side (Phase 3.5).
    Flow(bool),
    /// Cursor highlight — not yet wired on the daemon side (Phase 3.5).
    Highlight(bool),
    Dpi(u16),
    /// Scroll sensitivity — not yet wired on the daemon side (Phase 3.5).
    Scroll(u8),
    /// Haptic intensity — not yet wired on the daemon side (Phase 3.5).
    HapticIntensity(u8),
    /// Pointer acceleration — not yet wired on the daemon side (Phase 3.5).
    Accel(f32),
    EasySwitch(u8),
}

/// Dispatch an action to the daemon. Returns `Ok(())` even for
/// unimplemented variants — they log a warning instead of returning an error
/// so the caller treats them identically.
pub async fn apply(action: Action) -> Result<(), String> {
    // Actions not yet backed by a daemon method: log and return early so we
    // don't attempt a D-Bus connection for a no-op. Tracked as Phase 3.5
    // follow-up work in docs/plans/followups.md (P2.2).
    match &action {
        Action::Radial(_)
        | Action::Scroll(_)
        | Action::HapticIntensity(_)
        | Action::Accel(_)
        | Action::Flow(_)
        | Action::Highlight(_) => {
            warn!(action = ?action, "action not yet wired to daemon - tracked as Phase 3.5 follow-up");
            return Ok(());
        }
        _ => {}
    }

    let conn = session_conn().await?;
    let proxy = DaemonProxy::new(conn)
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
        Action::Smart(v) => {
            // set_smart_shift takes (enabled, threshold). The popup toggle
            // only signals on/off; pass 30 as the default threshold — this
            // matches the daemon's own SmartShift default (30% torque).
            info!(enabled = v, threshold = 30u8, "dispatch: set_smart_shift");
            proxy.set_smart_shift(v, 30).await
        }
        Action::Dpi(dpi) => {
            info!(dpi, "dispatch: set_dpi");
            proxy.set_dpi(dpi).await
        }
        Action::EasySwitch(host) => {
            info!(host, "dispatch: set_host");
            proxy.set_host(host).await.map(|_accepted| ())
        }
        // Already handled in the early-return arm above.
        Action::Radial(_)
        | Action::Scroll(_)
        | Action::HapticIntensity(_)
        | Action::Accel(_)
        | Action::Flow(_)
        | Action::Highlight(_) => unreachable!(),
    };

    result.map_err(|e| format!("daemon D-Bus call failed: {e}"))
}

/// Fetch the current device state from the daemon.
/// Returns `None` if the daemon is not reachable.
pub async fn fetch_device_state() -> Option<DeviceState> {
    let conn = session_conn().await.ok()?;
    let proxy = DaemonProxy::new(conn).await.ok()?;
    // Daemon returns (battery_pct, charging, connection, device_name, device_id) — 5 fields.
    match proxy.get_active_device_state().await {
        Ok((battery_pct, charging, connection_type, device_name, _device_id)) => {
            Some(DeviceState {
                battery_pct,
                charging,
                // Treat non-empty connection string as connected.
                connected: !connection_type.is_empty() && connection_type != "disconnected",
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
