//! GtkApplication wiring. Owns the layer-shell overlay window and the
//! D-Bus listener task; routes MenuRequested into the window's show path.

use gtk4 as gtk;
use gtk::prelude::*;

const APP_ID: &str = "org.kde.juhradialmx.overlay";

pub fn run() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        // Overlay should not exit when the only window is hidden — we
        // keep the window alive between activations and toggle visibility
        // via layer-shell present()/set_anchor() updates.
        .flags(gio::ApplicationFlags::FLAGS_NONE)
        .build();

    app.connect_activate(|app| {
        let win = crate::window::OverlayWindow::new(app);
        win.present_hidden();

        // TODO: spawn the D-Bus listener task and route signals into
        //       win.show_at(x, y) / win.hide_menu().
        //
        // TODO: spawn the config watcher and route reload events into
        //       win.reload_config().
        //
        // TODO: build the system-tray icon (settings, exit, edit-menu).
        let _ = win; // hold a strong ref via the application's window list
    });

    app.run()
}
