//! Async client for the juhradial-cursor GNOME extension's D-Bus
//! surface. Currently exposes `GetFocusedWindowClass` so the page
//! editor's "Detect from focused window" button can capture the
//! WM_CLASS / app_id of whatever app the user just clicked into.
//!
//! Lives next to `raise.rs` (which talks to the same extension via
//! `RaiseOverlay`) — kept separate to keep that module's name
//! self-documenting. A future cleanup could fold both into one
//! `CursorHelperProxy` if more methods land.

use tracing::warn;
use zbus::{proxy, Connection};

const SETTINGS_APP_ID: &str = "org.juhradial.settings";

#[proxy(
    interface = "org.juhradial.CursorHelper",
    default_service = "org.juhradial.CursorHelper",
    default_path = "/org/juhradial/CursorHelper"
)]
trait CursorHelper {
    fn get_focused_window_class(&self, ignore_app_id: &str) -> zbus::Result<String>;
}

/// Ask the extension for the class of whatever real app window is
/// currently focused. Skips windows owned by the settings app
/// itself so clicking the "Detect" button (which steals focus
/// briefly) doesn't return our own class.
///
/// Returns `Some(class)` for a real focused app, `None` when the
/// extension is missing, the call fails, or no eligible window is
/// focused.
pub async fn detect_focused_class() -> Option<String> {
    match try_call().await {
        Ok(s) if !s.is_empty() => Some(s),
        Ok(_) => None,
        Err(e) => {
            warn!(
                "GetFocusedWindowClass D-Bus call failed: {e} \
                 (juhradial-cursor extension not enabled, or running \
                 against an old version that doesn't expose this method)"
            );
            None
        }
    }
}

/// Sleep `delay_secs` seconds then sample the focused class. The
/// caller's "Detect" click brought the settings window forward;
/// this delay gives the user a window to alt-tab or click into the
/// target app so the sample lands on the *intended* foreground
/// window instead of the settings app itself. The extension also
/// filters out our own class via `ignore_app_id`, so even if the
/// user doesn't switch focus the result will be the next-most-
/// recent app — never the settings window.
pub async fn detect_focused_class_after(delay_secs: u64) -> Option<String> {
    tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
    detect_focused_class().await
}

async fn try_call() -> zbus::Result<String> {
    let conn = Connection::session().await?;
    let proxy = CursorHelperProxy::new(&conn).await?;
    proxy.get_focused_window_class(SETTINGS_APP_ID).await
}
