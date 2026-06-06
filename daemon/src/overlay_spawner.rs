//! Owns the child process handle for oxidemx-overlay.
//!
//! Backs the daemon's `EnsureOverlayRunning()` D-Bus handler. The
//! indicator extension calls that handler when the user toggles
//! "Radial Overlay" ON, so the extension never has to spawn
//! long-lived processes itself (§3.4 of INDICATOR_DESIGN.md, "Stack
//! supervisor + unified health surface").
//!
//! Idempotent: a second `ensure_running` call while the overlay is
//! already on the bus returns Ok(()) without spawning a second
//! process. The held Child is dropped (not killed) on daemon
//! shutdown so the user's overlay survives a daemon restart.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};
use tracing::{info, warn};
use zbus::Connection;

/// Wayland app_id / D-Bus well-known name the overlay claims when
/// it boots. Matches `overlay-rs/src/app.rs::APP_ID`. If you change
/// one, change both.
const OVERLAY_BUS_NAME: &str = "org.oxidemx.overlay";

/// Total time we wait for the overlay process to claim its bus
/// name after spawn before giving up. Empirically the overlay takes
/// ~200 ms to register on a warm box; 3 s gives us margin for
/// cold-cache first-launch.
const SPAWN_WAIT_TIMEOUT: Duration = Duration::from_secs(3);

/// How often we re-probe the bus while waiting. 100 ms is a balance
/// between snappiness and not burning a CPU spinning on `name_has_owner`.
const SPAWN_WAIT_POLL: Duration = Duration::from_millis(100);

pub struct OverlaySpawner {
    child: Mutex<Option<Child>>,
}

impl OverlaySpawner {
    pub fn new() -> Self {
        Self { child: Mutex::new(None) }
    }

    /// Idempotent overlay-process ensure.
    ///
    /// Returns `Ok(())` immediately if `OVERLAY_BUS_NAME` already has
    /// an owner on the session bus. Otherwise spawns
    /// `oxidemx-overlay` and polls until the name appears or
    /// `SPAWN_WAIT_TIMEOUT` elapses.
    pub async fn ensure_running(&self, conn: &Connection) -> Result<(), String> {
        let proxy = match zbus::fdo::DBusProxy::new(conn).await {
            Ok(p) => p,
            Err(e) => return Err(format!("DBusProxy construct: {e}")),
        };
        if name_owned(&proxy, OVERLAY_BUS_NAME).await {
            return Ok(());
        }
        match Command::new("oxidemx-overlay")
            .env("WINIT_UNIX_BACKEND", "x11")
            .spawn()
        {
            Ok(child) => {
                info!("Spawned oxidemx-overlay via EnsureOverlayRunning");
                match self.child.lock() {
                    Ok(mut guard) => *guard = Some(child),
                    Err(poisoned) => {
                        warn!("OverlaySpawner child mutex poisoned; recovering");
                        *poisoned.into_inner() = Some(child);
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to spawn oxidemx-overlay");
                return Err(format!("spawn oxidemx-overlay: {e}"));
            }
        }
        let deadline = Instant::now() + SPAWN_WAIT_TIMEOUT;
        while Instant::now() < deadline {
            if name_owned(&proxy, OVERLAY_BUS_NAME).await {
                return Ok(());
            }
            tokio::time::sleep(SPAWN_WAIT_POLL).await;
        }
        Err(format!(
            "overlay did not appear on the bus within {:?}",
            SPAWN_WAIT_TIMEOUT
        ))
    }
}

impl Default for OverlaySpawner {
    fn default() -> Self { Self::new() }
}

async fn name_owned(proxy: &zbus::fdo::DBusProxy<'_>, name: &str) -> bool {
    let Ok(name_owned) = name.try_into() else { return false };
    proxy.name_has_owner(name_owned).await.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_constructs_empty_state() {
        let s = OverlaySpawner::new();
        assert!(s.child.lock().unwrap().is_none());
    }
}
