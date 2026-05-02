//! Listener for the daemon's D-Bus signals on `org.kde.juhradialmx`.
//!
//! Wire format mirrors what the existing Python overlay subscribes to:
//!   * `MenuRequested(x: f64, y: f64)`  — Mutter logical pixels
//!   * `HideMenu()`
//!   * `CursorMoved(dx: f64, dy: f64)`  — drag-mode delta from menu centre
//!
//! Events are forwarded into the GTK main thread via an
//! `async_channel::Sender` so window mutation stays single-threaded.

use async_channel::Sender;

#[derive(Debug, Clone)]
pub enum OverlayEvent {
    Show { x: f64, y: f64 },
    Hide,
    CursorMoved { dx: f64, dy: f64 },
}

pub async fn run_listener(_tx: Sender<OverlayEvent>) -> zbus::Result<()> {
    // TODO: connect to the session bus, subscribe to MenuRequested /
    // HideMenu / CursorMoved on the daemon's well-known name, decode and
    // forward via tx.send(...).await.
    //
    // Loosely:
    //   let conn = zbus::Connection::session().await?;
    //   let mut stream = conn.add_match_rule(...).await?;
    //   while let Some(msg) = stream.next().await {
    //       if let Ok((x, y)) = msg.body::<(f64, f64)>() {
    //           tx.send(OverlayEvent::Show { x, y }).await.ok();
    //       }
    //   }
    Ok(())
}
