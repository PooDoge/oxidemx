//! Single-instance enforcement via D-Bus name ownership.
//!
//! Flow:
//!   1. Spawn a dedicated thread with its own tokio runtime.
//!   2. The thread connects to the session bus and tries to claim
//!      `org.juhradial.Settings` with `DoNotQueue`.
//!   3a. If we get the name → register a `Focus` method handler at
//!       `/org/juhlabs/juhradial/Settings`. Each call writes `()`
//!       into an async-channel that iced consumes via a
//!       `Subscription::run` stream. The thread parks forever to
//!       keep the connection + service alive.
//!   3b. If the name is taken → connect a Proxy to the existing
//!       owner, call `Focus` on it, exit(0).
//!   4. main() blocks until the worker reports its outcome via a
//!      sync mpsc channel. If we lost the race, exit immediately;
//!      otherwise return the focus-event receiver to wire into
//!      iced's subscription.
//!
//! The session bus is the only dep — works on any Linux desktop
//! that has dbus-broker / dbus-daemon (i.e. all of them). No
//! reliance on the GNOME extension or any specific WM.

use async_channel::{Receiver, Sender};
use futures_util::stream::StreamExt;
use std::sync::mpsc as sync_mpsc;
use tracing::{info, warn};
use zbus::{
    fdo::{RequestNameFlags, RequestNameReply},
    interface, Connection,
};

const BUS_NAME: &str = "org.juhradial.Settings";
const OBJECT_PATH: &str = "/org/juhradial/Settings";
const INTERFACE: &str = "org.juhradial.Settings";

/// Outcome of the singleton handshake.
pub enum Acquisition {
    /// We are the primary instance. The receiver yields one `()`
    /// per `Focus` call from a second instance.
    Primary(Receiver<()>),
    /// Another instance was already running; we sent it Focus and
    /// the caller should exit immediately.
    SecondaryFocused,
    /// Couldn't talk to the bus at all (no DBUS_SESSION_BUS_ADDRESS,
    /// dbus-daemon not running). The caller should proceed without
    /// singleton enforcement — better to open a duplicate window
    /// than refuse to launch.
    BusUnavailable,
}

/// Service struct exposed on the session bus. Holds a sender into
/// the channel iced subscribes to; each `Focus` call wakes the
/// running window via `Message::Focus`.
struct SettingsService {
    focus_tx: Sender<()>,
}

#[interface(name = "org.juhradial.Settings")]
impl SettingsService {
    /// Bring the settings window to the front + focus it. Idempotent
    /// — second instance calls this and exits, but additional
    /// `juhradial-settings` invocations from any source (terminal,
    /// .desktop file, etc.) all funnel through here.
    async fn focus(&self) {
        info!("Focus requested via D-Bus");
        let _ = self.focus_tx.send(()).await;
    }
}

/// Run the singleton handshake on a dedicated thread + tokio
/// runtime, blocking until we know whether we're the primary
/// instance. Cheap (one bus round-trip in the success case).
pub fn try_acquire_or_focus_existing() -> Acquisition {
    let (status_tx, status_rx) = sync_mpsc::channel::<Acquisition>();
    let (focus_tx, focus_rx) = async_channel::unbounded::<()>();

    std::thread::Builder::new()
        .name("juhradial-settings-singleton".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    warn!("singleton: tokio runtime failed: {e}");
                    let _ = status_tx.send(Acquisition::BusUnavailable);
                    return;
                }
            };
            rt.block_on(async move {
                handshake(status_tx, focus_tx, focus_rx).await;
            });
        })
        .expect("spawn singleton thread");

    status_rx
        .recv()
        .unwrap_or(Acquisition::BusUnavailable)
}

async fn handshake(
    status_tx: sync_mpsc::Sender<Acquisition>,
    focus_tx: Sender<()>,
    focus_rx: Receiver<()>,
) {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            warn!("singleton: no session bus ({e}); skipping enforcement");
            let _ = status_tx.send(Acquisition::BusUnavailable);
            return;
        }
    };

    // Register the service FIRST, then claim the name — zbus warns
    // otherwise that incoming method calls between the name claim
    // and the object registration could be lost. We register on a
    // throwaway path BEFORE name acquisition, then move it once
    // we know we're the primary; if we lose the race, we discard
    // the registration when the connection drops.
    if let Err(e) = conn
        .object_server()
        .at(OBJECT_PATH, SettingsService { focus_tx })
        .await
    {
        warn!("singleton: failed to register service: {e}");
        let _ = status_tx.send(Acquisition::BusUnavailable);
        return;
    }

    let request = conn
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
        .await;

    match request {
        Ok(RequestNameReply::PrimaryOwner) => {
            info!("singleton: claimed {BUS_NAME} as primary instance");
            let _ = status_tx.send(Acquisition::Primary(focus_rx));
            // Park forever; the connection + service stay alive,
            // serving Focus calls from future settings invocations.
            std::future::pending::<()>().await;
        }
        Ok(other) => {
            // Shouldn't happen with DoNotQueue but treat it as
            // "another owner exists" — same handling.
            info!("singleton: {BUS_NAME} unexpected reply ({other:?}); calling Focus on owner");
            send_focus_to_existing(&conn).await;
            let _ = status_tx.send(Acquisition::SecondaryFocused);
        }
        Err(zbus::Error::NameTaken) => {
            info!("singleton: {BUS_NAME} already taken; calling Focus on owner");
            send_focus_to_existing(&conn).await;
            let _ = status_tx.send(Acquisition::SecondaryFocused);
        }
        Err(e) => {
            warn!("singleton: request_name failed ({e}); skipping enforcement");
            let _ = status_tx.send(Acquisition::BusUnavailable);
        }
    }
}

async fn send_focus_to_existing(conn: &Connection) {
    let proxy = match zbus::Proxy::new(conn, BUS_NAME, OBJECT_PATH, INTERFACE).await {
        Ok(p) => p,
        Err(e) => {
            warn!("singleton: couldn't make proxy to existing instance: {e}");
            return;
        }
    };
    if let Err(e) = proxy.call::<_, _, ()>("Focus", &()).await {
        warn!("singleton: Focus call to existing instance failed: {e}");
    }
}

/// Convert the focus-event receiver into a `Stream` for iced's
/// `Subscription::run`. Each item the stream yields becomes a
/// `Message::Focus` in the iced update loop.
pub fn focus_stream(rx: Receiver<()>) -> impl futures_util::stream::Stream<Item = ()> {
    rx.boxed()
}
