//! Single-instance for the chat window via the well-known D-Bus name
//! `org.oxidemx.Chat`. The primary instance owns the name and serves a `Present`
//! method; a second launch calls `Present` on the primary and exits.

use std::sync::Mutex;
use tokio::sync::mpsc;
use zbus::{connection, interface};

const NAME: &str = "org.oxidemx.Chat";
const PATH: &str = "/org/oxidemx/Chat";

/// Outcome of `acquire_or_present`.
pub enum SingleInstance {
    /// This process owns the name; poll `present` for raise requests.
    Primary { present: mpsc::UnboundedReceiver<()> },
    /// Another instance is already running and was asked to present; exit.
    Secondary,
}

struct PresentService { tx: mpsc::UnboundedSender<()> }

#[interface(name = "org.oxidemx.Chat")]
impl PresentService {
    /// Ask the running chat window to raise/focus itself.
    async fn present(&self) { let _ = self.tx.send(()); }
}

/// Global receiver stash — the binary fills this before `run_chat_window()`,
/// and the subscription drains it once.
static PRESENT_RX: Mutex<Option<mpsc::UnboundedReceiver<()>>> = Mutex::new(None);

/// Store the receiver for the subscription to pick up.
pub fn stash_present_receiver(rx: mpsc::UnboundedReceiver<()>) {
    *PRESENT_RX.lock().unwrap() = Some(rx);
}

/// Take the receiver out of the stash (called once from the subscription).
pub fn take_present_receiver() -> Option<mpsc::UnboundedReceiver<()>> {
    PRESENT_RX.lock().unwrap().take()
}

/// Try to become the primary instance. On success, returns `Primary` with a
/// receiver of present-requests and keeps the connection alive (leaked into a
/// background tokio task). On a name clash, calls `Present` on the existing
/// instance and returns `Secondary`. On any other error, returns `Primary` with
/// a dead receiver so the caller still opens a window (never silently exits).
pub async fn acquire_or_present() -> SingleInstance {
    let (tx, rx) = mpsc::unbounded_channel();
    let built = connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.serve_at(PATH, PresentService { tx }));
    match built {
        Ok(builder) => match builder.build().await {
            Ok(conn) => {
                // Keep the connection alive by leaking it — it must outlive
                // the iced event loop. Box::leak gives it a 'static lifetime.
                Box::leak(Box::new(conn));
                SingleInstance::Primary { present: rx }
            }
            Err(zbus::Error::NameTaken) => {
                call_present().await;
                SingleInstance::Secondary
            }
            // Degrade gracefully: open a window anyway rather than silently dying.
            Err(e) => {
                tracing::warn!("D-Bus name acquire failed ({e}); opening window without single-instance guard");
                SingleInstance::Primary { present: rx }
            }
        },
        Err(e) => {
            tracing::warn!("D-Bus builder failed ({e}); opening window without single-instance guard");
            SingleInstance::Primary { present: rx }
        }
    }
}

async fn call_present() {
    let builder = match connection::Builder::session() {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("could not build session bus connection for Present call: {e}");
            return;
        }
    };
    let conn = match builder.build().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("could not connect to session bus to call Present: {e}");
            return;
        }
    };
    let _ = conn
        .call_method(Some(NAME), PATH, Some("org.oxidemx.Chat"), "Present", &())
        .await;
}
