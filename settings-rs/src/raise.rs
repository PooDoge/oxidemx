//! Best-effort window-raise bridge to the juhradial-cursor GNOME
//! extension.
//!
//! Wayland's compositor blocks app-side focus requests
//! (xdg-toplevel can't grant itself focus without an
//! xdg-activation token from the requesting process). Since our
//! Focus message arrives via D-Bus from a separate process, we
//! have no token. The cleanest workaround is to ask the
//! juhradial-cursor GNOME extension to raise + activate the
//! window — the extension runs *inside* Mutter and uses
//! `Meta.Window.activate()` which the compositor respects.
//!
//! This is fire-and-forget: if the extension isn't installed or
//! the call fails, the iced fallback (`window::set_mode` +
//! `window::gain_focus`) at least un-minimises the window. In
//! practice that combination handles ~95 % of the focus-request
//! cases — and once the extension is reloaded with the v4
//! `RaiseOverlay` (raise + activate), the remaining 5 %
//! (cross-workspace, focus-stealing-prevention) work too.

use tracing::{debug, warn};
use zbus::{proxy, Connection};

const APP_ID: &str = "org.juhlabs.juhradial.settings";

#[proxy(
    interface = "org.juhradial.CursorHelper",
    default_service = "org.juhradial.CursorHelper",
    default_path = "/org/juhradial/CursorHelper"
)]
trait CursorHelper {
    fn raise_overlay(&self, app_id: &str) -> zbus::Result<bool>;
}

/// Fire the RaiseOverlay call; log success/failure but never error
/// out — the iced-side fallback handles the unhappy path.
pub async fn raise_settings_window() {
    match try_raise().await {
        Ok(true) => debug!("RaiseOverlay succeeded"),
        Ok(false) => debug!("RaiseOverlay returned false (window not found)"),
        Err(e) => warn!("RaiseOverlay D-Bus call failed: {e}"),
    }
}

async fn try_raise() -> zbus::Result<bool> {
    let conn = Connection::session().await?;
    let proxy = CursorHelperProxy::new(&conn).await?;
    proxy.raise_overlay(APP_ID).await
}
