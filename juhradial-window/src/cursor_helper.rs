//! Async client for the `juhradial-cursor` GNOME shell extension's
//! window-positioning D-Bus methods. The extension exposes the
//! `org.juhradial.CursorHelper` service at `/org/juhradial/CursorHelper`.
//!
//! Used because Mutter (stable GNOME) doesn't advertise
//! `wlr-layer-shell`, so regular xdg-shell clients can't position
//! their own toplevels. The extension runs *inside* Mutter and can
//! call `Meta.Window.move_frame()` directly.
//!
//! This module was extracted from `overlay-rs/src/ext_positioner.rs`
//! in 2026-05 so `popup-rs` could use the same positioning client.
//! Re-exported via `juhradial_window::cursor_helper`.
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
    fn get_focused_window_class(&self, ignore_app_id: &str) -> zbus::Result<String>;
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

/// Ask the GNOME extension for the currently-focused window's
/// application class. `ignore_app_id` is the overlay's own app_id —
/// the extension skips that window so toggle-mode focus on the
/// overlay itself doesn't poison the result.
///
/// Returns `Some(class)` when focus resolves to a real app window,
/// `None` when the call fails (extension missing) or no eligible
/// window is focused. Empty strings are treated as `None` so the
/// caller doesn't have to special-case them.
pub async fn get_focused_window_class(ignore_app_id: String) -> Option<String> {
    match try_get_focused(&ignore_app_id).await {
        Ok(s) if !s.is_empty() => Some(s),
        Ok(_) => None,
        Err(e) => {
            warn!(
                "GetFocusedWindowClass D-Bus call failed: {e} \
                 (extension {HELPER_SERVICE} not enabled, or running \
                 against an old version that doesn't expose this method)"
            );
            None
        }
    }
}

async fn try_get_focused(ignore_app_id: &str) -> zbus::Result<String> {
    let conn = Connection::session().await?;
    let proxy = CursorHelperProxy::new(&conn).await?;
    proxy.get_focused_window_class(ignore_app_id).await
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
    //          "org.juhradial.overlay" 200 100 -1
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
