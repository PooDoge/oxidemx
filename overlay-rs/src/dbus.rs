//! Listener for the daemon's D-Bus signals on `org.oxidemx.Daemon`.
//!
//! Wire format mirrors what the Python overlay subscribes to. Names
//! and signatures are taken directly from
//! `daemon/src/dbus/interface.rs`:
//!
//!   * `MenuRequested(x: i32, y: i32)` — Mutter logical pixels.
//!   * `HideMenu()` — close the menu.
//!   * `CursorMoved(x: i32, y: i32)` — accumulated REL_X / REL_Y
//!     deltas from the start of the press, NOT absolute coords.
//!
//! Exposed as a `Stream<OverlayEvent>` so iced's
//! `Subscription::run` pumps events into the application's update
//! loop directly. Reconnects on disconnect with a 2 s backoff.

use futures_util::stream::{Stream, StreamExt};
use tracing::{debug, info, warn};
use zbus::proxy;

const DAEMON_PATH: &str = "/org/oxidemx/Daemon";

#[derive(Debug, Clone)]
pub enum OverlayEvent {
    Show { x: i32, y: i32 },
    Hide,
    CursorMoved { dx: i32, dy: i32 },
}

#[proxy(
    interface = "org.oxidemx.Daemon",
    default_service = "org.oxidemx.Daemon",
    default_path = "/org/oxidemx/Daemon"
)]
trait Daemon {
    #[zbus(signal)]
    fn menu_requested(&self, x: i32, y: i32) -> zbus::Result<()>;

    #[zbus(signal, name = "HideMenu")]
    fn hide_menu(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn cursor_moved(&self, x: i32, y: i32) -> zbus::Result<()>;
}

/// Stream entry-point used by `iced::Subscription::run`. Spawns a
/// background task that connects to the session bus, subscribes to
/// the three signal streams, and forwards each onto an
/// `async_channel`. We then convert the channel into a futures
/// `Stream` for iced's consumption.
pub fn stream() -> impl Stream<Item = OverlayEvent> {
    let (tx, rx) = async_channel::unbounded::<OverlayEvent>();

    // The background loop reconnects if zbus drops the connection.
    iced::futures::executor::block_on(async {});
    tokio::task::spawn(async move {
        loop {
            match run_listener(tx.clone()).await {
                Ok(()) => {
                    info!("dbus listener returned ok; exiting reconnect loop");
                    break;
                }
                Err(e) => {
                    warn!("dbus listener exited: {e}; reconnecting in 2s");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    });

    rx
}

async fn run_listener(tx: async_channel::Sender<OverlayEvent>) -> zbus::Result<()> {
    let conn = zbus::connection::Builder::session()?.build().await?;
    // Single-instance guard. The daemon's spawner checks this name
    // before spawning, but an overlay sitting in this listener's
    // 2 s reconnect backoff doesn't own it yet — a menu request in
    // that window spawns a duplicate. Duplicates used to park in
    // the NameTaken → retry loop forever (three live overlays
    // fighting over focus made the AI page dismiss instantly);
    // now they exit on the spot. Vision-harness instances
    // (OXIDEMX_VISION_SHOT) intentionally run alongside the real
    // overlay and never claim the name.
    if std::env::var_os("OXIDEMX_VISION_SHOT").is_none() {
        match conn.request_name("org.oxidemx.overlay").await {
            Ok(()) => {}
            Err(zbus::Error::NameTaken) => {
                info!("another overlay instance owns org.oxidemx.overlay — exiting duplicate");
                std::process::exit(0);
            }
            Err(e) => return Err(e),
        }
    }
    // Serve AgentHost only when we own the overlay name (not vision-shot instances).
    if std::env::var_os("OXIDEMX_VISION_SHOT").is_none() {
        conn.object_server()
            .at(
                "/org/oxidemx/AgentHost",
                crate::agent::host::AgentHostService,
            )
            .await?;
        info!("org.oxidemx.AgentHost registered at /org/oxidemx/AgentHost");
    }
    let proxy = DaemonProxy::new(&conn).await?;
    info!(
        "Subscribing to org.oxidemx.Daemon signals on {}",
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
            if let Ok(args) = sig.args() {
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
        }
    };

    let hide_task = async move {
        while (hide_stream.next().await).is_some() {
            debug!("HideMenu");
            if tx_hide.send(OverlayEvent::Hide).await.is_err() {
                return;
            }
        }
    };

    let cursor_task = async move {
        while let Some(sig) = cursor_stream.next().await {
            if let Ok(args) = sig.args() {
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
        }
    };

    futures_util::future::join3(menu_task, hide_task, cursor_task).await;
    Ok(())
}
