//! GtkApplication wiring. Owns the layer-shell overlay window and the
//! D-Bus listener task; routes the daemon's MenuRequested / HideMenu /
//! CursorMoved signals into the window.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use tracing::{debug, error, info};

use crate::dbus::OverlayEvent;
use crate::window::OverlayWindow;

const APP_ID: &str = "org.kde.juhradialmx.overlay";

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::FLAGS_NONE)
        .build();

    app.connect_activate(|app| {
        // Build the overlay window once and keep it alive between
        // activations — show_at()/hide_menu() flip its layer-shell
        // surface visibility instead of recreating it.
        let win = Rc::new(RefCell::new(OverlayWindow::new(app)));
        win.borrow().present_hidden();

        // Channel from the D-Bus listener (background async task) to
        // the GTK main loop. Unbounded — menu events are rare and
        // dropping any of them is worse than holding a small queue.
        let (tx, rx) = async_channel::unbounded::<OverlayEvent>();

        // Spawn the listener on glib's main context so it shares the
        // GTK event loop. zbus uses async-io internally and is happy
        // running on any executor that drives futures; glib's
        // MainContext is one of them.
        let ctx = glib::MainContext::default();
        ctx.spawn_local(async move {
            loop {
                if let Err(e) = crate::dbus::run_listener(tx.clone()).await {
                    error!("dbus listener exited: {e}; reconnecting in 2s");
                    glib::timeout_future_seconds(2).await;
                } else {
                    info!("dbus listener returned ok; exiting reconnect loop");
                    break;
                }
            }
        });

        // Drain events into the window on the main thread.
        let win_for_events = win.clone();
        ctx.spawn_local(async move {
            while let Ok(evt) = rx.recv().await {
                match evt {
                    OverlayEvent::Show { x, y } => {
                        debug!(x, y, "Show event");
                        win_for_events
                            .borrow()
                            .show_at(x as f64, y as f64);
                    }
                    OverlayEvent::Hide => {
                        debug!("Hide event");
                        win_for_events.borrow().hide_menu();
                    }
                    OverlayEvent::CursorMoved { dx, dy } => {
                        win_for_events
                            .borrow()
                            .radial
                            .on_cursor_moved(dx, dy);
                    }
                }
            }
            info!("event channel closed");
        });

        // TODO: spawn the config watcher (notify::RecommendedWatcher
        //       on config.json + profiles/) and route reload events
        //       into win.borrow_mut().reload_config().
        //
        // TODO: build the system-tray icon (KStatusNotifierItem).
        //
        // Hold a strong ref so the application's window list keeps the
        // overlay alive for the lifetime of the gtk::Application.
        let _ = win;
    });

    app.run()
}
