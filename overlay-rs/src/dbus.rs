//! Listener for the daemon's D-Bus signals on `org.kde.juhradialmx`.
//!
//! Wire format mirrors what the existing Python overlay subscribes to.
//! Names and signatures are taken directly from
//! `daemon/src/dbus/interface.rs`:
//!
//!   * `MenuRequested(x: i32, y: i32)`  — Mutter logical pixels of the
//!     gesture-button-press cursor location.
//!   * `HideMenu()`                     — close the menu.
//!   * `CursorMoved(x: i32, y: i32)`    — accumulated REL_X / REL_Y
//!     deltas from the start of the press, used by drag-mode to map
//!     cursor motion to slice angles. NOT absolute screen coords.
//!
//! Events are forwarded to the GTK main thread via an
//! `async_channel::Sender` so window mutation stays single-threaded.

use async_channel::Sender;
use futures_util::StreamExt;
use tracing::{debug, info, warn};
use zbus::{proxy, Connection};

/// Daemon's well-known service name. Matches `daemon/src/dbus/mod.rs::DBUS_NAME`.
pub const DAEMON_SERVICE: &str = "org.kde.juhradialmx";
/// Daemon's object path. Matches `daemon/src/dbus/mod.rs::DBUS_PATH`.
pub const DAEMON_PATH: &str = "/org/kde/juhradialmx/Daemon";

/// Typed proxy for the subset of the daemon interface the overlay
/// consumes. The proxy macro generates `DaemonProxy` plus
/// `receive_<signal>()` helpers that return typed
/// `Stream<Item = <SignalArgs>>` values.
#[proxy(
    interface = "org.kde.juhradialmx.Daemon",
    default_service = "org.kde.juhradialmx",
    default_path = "/org/kde/juhradialmx/Daemon"
)]
trait Daemon {
    #[zbus(signal)]
    fn menu_requested(&self, x: i32, y: i32) -> zbus::Result<()>;

    #[zbus(signal, name = "HideMenu")]
    fn hide_menu(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn cursor_moved(&self, x: i32, y: i32) -> zbus::Result<()>;
}

/// Event the GTK side reacts to. All coords stay in their wire-format
/// units (i32 logical pixels for the menu open, i32 accumulated deltas
/// for cursor moves) — the radial widget is the only place that turns
/// them into f64 polar coordinates.
#[derive(Debug, Clone)]
pub enum OverlayEvent {
    Show { x: i32, y: i32 },
    Hide,
    CursorMoved { dx: i32, dy: i32 },
}

/// Subscribe to the daemon's three menu signals and forward each one
/// onto `tx`. Spawns one local task per signal stream so each runs
/// independently — Hide can fire even if a CursorMoved task is mid-
/// await without blocking on it. Returns when the proxy is created;
/// the spawned tasks live until either `tx` closes or any signal
/// stream ends (which we treat as a fatal disconnect).
pub async fn run_listener(tx: Sender<OverlayEvent>) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    let proxy = DaemonProxy::new(&conn).await?;
    info!(
        "Subscribing to org.kde.juhradialmx.Daemon signals on {}",
        DAEMON_PATH
    );

    let mut menu_stream = proxy.receive_menu_requested().await?;
    let mut hide_stream = proxy.receive_hide_menu().await?;
    let mut cursor_stream = proxy.receive_cursor_moved().await?;

    let tx_show = tx.clone();
    let tx_hide = tx.clone();
    let tx_cursor = tx;

    let menu_task = async move {
        while let Some(sig) = menu_stream.next().await {
            match sig.args() {
                Ok(args) => {
                    debug!(x = args.x, y = args.y, "MenuRequested");
                    if tx_show
                        .send(OverlayEvent::Show {
                            x: args.x,
                            y: args.y,
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Err(e) => warn!("MenuRequested decode failed: {e}"),
            }
        }
        warn!("MenuRequested signal stream ended");
    };

    let hide_task = async move {
        while (hide_stream.next().await).is_some() {
            debug!("HideMenu");
            if tx_hide.send(OverlayEvent::Hide).await.is_err() {
                return;
            }
        }
        warn!("HideMenu signal stream ended");
    };

    let cursor_task = async move {
        while let Some(sig) = cursor_stream.next().await {
            match sig.args() {
                Ok(args) => {
                    if tx_cursor
                        .send(OverlayEvent::CursorMoved {
                            dx: args.x,
                            dy: args.y,
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Err(e) => warn!("CursorMoved decode failed: {e}"),
            }
        }
        warn!("CursorMoved signal stream ended");
    };

    // join_all wins when all three tasks have finished. In practice
    // any one of them ending means the bus dropped — caller restarts.
    futures_util::future::join3(menu_task, hide_task, cursor_task).await;
    info!("dbus listener: all signal streams closed");
    Ok(())
}
