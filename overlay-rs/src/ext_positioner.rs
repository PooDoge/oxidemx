//! Async client for the GNOME extension's window-positioning D-Bus
//! methods. The extension lives in `gnome-extension/juhradial-cursor`
//! and exposes `MoveOverlay(app_id, x, y, monitor) -> success` on
//! `org.juhradial.CursorHelper`.
//!
//! We use this because Mutter (stable GNOME) doesn't advertise
//! `wlr-layer-shell`, so a regular xdg-shell client can't position
//! its own toplevel. The extension runs *inside* Mutter and can
//! call `Meta.Window.move_frame()` directly, bypassing the
//! protocol restriction.
//!
//! Fire-and-forget by design — if the extension is missing or the
//! call fails, the menu opens wherever Mutter chose to place the
//! window. The caller (`app::update`) logs a warning so users can
//! tell the extension isn't picking up the positioning request.

use tracing::warn;
use zbus::{proxy, Connection};

const HELPER_SERVICE: &str = "org.juhradial.CursorHelper";
const HELPER_PATH: &str = "/org/juhradial/CursorHelper";

#[proxy(
    interface = "org.juhradial.CursorHelper",
    default_service = "org.juhradial.CursorHelper",
    default_path = "/org/juhradial/CursorHelper"
)]
trait CursorHelper {
    fn move_overlay(&self, app_id: &str, x: i32, y: i32, monitor: i32) -> zbus::Result<bool>;
    #[allow(dead_code)]
    fn raise_overlay(&self, app_id: &str) -> zbus::Result<bool>;
    #[allow(dead_code)]
    fn list_monitors(&self) -> zbus::Result<Vec<(i32, i32, i32, i32, i32)>>;
}

/// Ask the extension to position our window at `(x, y)` in stage
/// logical pixels. `monitor = -1` interprets `(x, y)` as absolute
/// stage coords; `monitor >= 0` makes them monitor-local.
///
/// Returns `true` if the extension found our window and moved it,
/// `false` otherwise.
pub async fn move_overlay(app_id: String, x: i32, y: i32, monitor: i32) -> bool {
    match try_move_overlay(&app_id, x, y, monitor).await {
        Ok(success) => success,
        Err(e) => {
            warn!(
                "MoveOverlay D-Bus call failed: {e} \
                 (extension {HELPER_SERVICE} not enabled?)"
            );
            false
        }
    }
}

async fn try_move_overlay(
    app_id: &str,
    x: i32,
    y: i32,
    monitor: i32,
) -> zbus::Result<bool> {
    let conn = Connection::session().await?;
    let proxy = CursorHelperProxy::new(&conn).await?;
    proxy.move_overlay(app_id, x, y, monitor).await
}

#[allow(dead_code)]
pub async fn list_monitors() -> zbus::Result<Vec<(i32, i32, i32, i32, i32)>> {
    let conn = Connection::session().await?;
    let proxy = CursorHelperProxy::new(&conn).await?;
    proxy.list_monitors().await
}

#[cfg(test)]
mod tests {
    // Live tests against the running extension would require a GNOME
    // session, so we keep these documentation-only here. Manual
    // smoke test:
    //
    //   busctl --user call org.juhradial.CursorHelper \
    //          /org/juhradial/CursorHelper org.juhradial.CursorHelper \
    //          MoveOverlay siiii \
    //          "org.kde.juhradialmx.overlay" 200 100 -1
    //
    // returns: b true   (when overlay window is present)
    //          b false  (when no matching window)
    //
    // The "no matching window" path is exercised in the spike-iced
    // test run since the extension also reloads us silently when
    // app_id changes.

    const _SERVICE: &str = super::HELPER_SERVICE;
    const _PATH: &str = super::HELPER_PATH;
}
