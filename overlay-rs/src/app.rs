//! GtkApplication wiring. Loads the user config, builds the overlay
//! window with the initial radial state, spawns the D-Bus listener,
//! and routes daemon signals into the window on the GTK main thread.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use juhradial_shared::AppConfig;
use tracing::{debug, error, info, warn};

use crate::dbus::OverlayEvent;
use crate::radial::RadialState;
use crate::window::OverlayWindow;

const APP_ID: &str = "org.kde.juhradialmx.overlay";

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::FLAGS_NONE)
        .build();

    app.connect_activate(|app| {
        // Load the user's config; fall back to the default config (with
        // a default theme) if it can't be read so the overlay always
        // has *something* to render. The config-watch task will pick
        // up the file once the user creates / fixes it.
        let config = match crate::config::load() {
            Ok(c) => c,
            Err(e) => {
                warn!("could not load config ({e}); using defaults");
                AppConfig::default()
            }
        };

        let state = Rc::new(RefCell::new(RadialState::new(&config)));

        // Build the overlay window once and keep it alive between
        // activations — show_at()/hide_menu() flip its layer-shell
        // surface visibility instead of recreating it.
        let win = Rc::new(OverlayWindow::new(app, state.clone()));
        win.present_hidden();

        // Channel from the D-Bus listener (background async task) to
        // the GTK main loop. Unbounded — menu events are rare and
        // dropping any of them is worse than holding a small queue.
        let (tx, rx) = async_channel::unbounded::<OverlayEvent>();

        let ctx = glib::MainContext::default();

        // Listener task: subscribe to daemon signals.
        let tx_for_listener = tx.clone();
        ctx.spawn_local(async move {
            loop {
                if let Err(e) = crate::dbus::run_listener(tx_for_listener.clone()).await {
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
                        win_for_events.show_at(x as f64, y as f64);
                    }
                    OverlayEvent::Hide => {
                        debug!("Hide event");
                        win_for_events.hide_menu();
                    }
                    OverlayEvent::CursorMoved { dx, dy } => {
                        win_for_events.radial.on_cursor_moved(dx, dy);
                    }
                }
            }
            info!("event channel closed");
        });

        // TODO: spawn the config watcher (notify::RecommendedWatcher
        //       on config.json + profiles/) and route reload events
        //       into win.radial.reload_from(&new_config).
        //
        // TODO: build the system-tray icon (KStatusNotifierItem).

        // Hold a strong ref so the application's window list keeps the
        // overlay alive for the lifetime of the gtk::Application.
        let _ = win;
    });

    app.run()
}
