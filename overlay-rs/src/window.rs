//! The layer-shell overlay window — a transparent always-on-top surface
//! that the radial widget renders into. Positioning is monitor-local
//! logical pixels; the compositor handles output selection and
//! HiDPI scaling on our behalf.

use gtk4 as gtk;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, Layer, LayerShell};

/// Visual diameter of the radial menu in logical pixels. Matches the
/// legacy WINDOW_SIZE = (MENU_RADIUS + SHADOW_OFFSET + SUBMENU_EXTEND)*2.
pub const WINDOW_SIZE: i32 = 484;

pub struct OverlayWindow {
    pub gtk_window: gtk::ApplicationWindow,
    pub radial: crate::radial::RadialWidget,
}

impl OverlayWindow {
    pub fn new(app: &gtk::Application) -> Self {
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .default_width(WINDOW_SIZE)
            .default_height(WINDOW_SIZE)
            .resizable(false)
            .decorated(false)
            .build();

        // Layer-shell setup: overlay layer, no keyboard focus by default,
        // anchor to top-left of the chosen monitor with margins to place
        // the centre at the cursor.
        win.init_layer_shell();
        win.set_layer(Layer::Overlay);
        win.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
        win.set_anchor(Edge::Left, true);
        win.set_anchor(Edge::Top, true);
        win.set_exclusive_zone(-1);

        let radial = crate::radial::RadialWidget::new();
        win.set_child(Some(&radial.drawing_area));

        // Start hidden — present_hidden() in app::run keeps the GTK
        // application alive while the surface is unmapped between
        // activations.
        win.set_visible(false);

        OverlayWindow {
            gtk_window: win,
            radial,
        }
    }

    pub fn present_hidden(&self) {
        // present() is required for GTK to register the window with the
        // application. We immediately hide it again so the surface is
        // mapped on demand by show_at().
        self.gtk_window.present();
        self.gtk_window.set_visible(false);
    }

    /// Show the menu centred at the given cursor position.
    ///
    /// `x_logical`, `y_logical` are Mutter-stage logical pixels (matching
    /// what the daemon's GNOME cursor helper returns). The compositor
    /// resolves this to the right output and the right physical pixels.
    pub fn show_at(&self, x_logical: f64, y_logical: f64) {
        let display = gdk4::Display::default().expect("no default GDK display");
        let monitor = display
            .monitors()
            .into_iter()
            .filter_map(|o| o.ok().and_downcast::<gdk4::Monitor>())
            .find(|mon| mon.geometry().contains_point(x_logical as i32, y_logical as i32))
            .or_else(|| {
                display
                    .monitors()
                    .into_iter()
                    .next()
                    .and_then(|o| o.ok().and_downcast::<gdk4::Monitor>())
            });

        if let Some(mon) = monitor {
            let g = mon.geometry();
            let half = WINDOW_SIZE / 2;
            let local_x = (x_logical as i32) - g.x();
            let local_y = (y_logical as i32) - g.y();
            let mx = (local_x - half).clamp(0, (g.width() - WINDOW_SIZE).max(0));
            let my = (local_y - half).clamp(0, (g.height() - WINDOW_SIZE).max(0));

            self.gtk_window.set_monitor(Some(&mon));
            self.gtk_window.set_margin(Edge::Left, mx);
            self.gtk_window.set_margin(Edge::Top, my);
        }

        self.gtk_window.set_visible(true);
        // TODO: kick off radial-menu open animation.
    }

    pub fn hide_menu(&self) {
        // TODO: play close animation, then hide.
        self.gtk_window.set_visible(false);
    }
}
