//! The radial-menu drawing widget. Wraps a `gtk::DrawingArea` and renders
//! slices, icons, the centre puck, and animations via cairo.
//!
//! Most of `overlay/overlay_painting.py` translates into this module
//! without much change — `cr.move_to`, `cr.arc`, `cr.fill`, `cr.stroke`
//! are the cairo equivalents of the QPainter calls. Pango handles the
//! text. Custom-icon resolution lives in `render::icons`.

use gtk4 as gtk;
use gtk::prelude::*;

pub struct RadialWidget {
    pub drawing_area: gtk::DrawingArea,
}

impl RadialWidget {
    pub fn new() -> Self {
        let area = gtk::DrawingArea::builder()
            .content_width(crate::window::WINDOW_SIZE)
            .content_height(crate::window::WINDOW_SIZE)
            .can_focus(false)
            .build();

        area.set_draw_func(|_area, cr, _w, _h| {
            // TODO: port slice / icon / centre-puck / submenu rendering
            // from overlay/overlay_painting.py.
            //
            // Until the port lands, paint a transparent background so
            // the layer-shell surface is visible but empty (useful for
            // verifying positioning before drawing is wired up).
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            let _ = cr.paint();
        });

        // TODO: hook gtk::EventControllerMotion for toggle-mode hover
        // (replaces overlay/juhradial-overlay.py:mouseMoveEvent).
        //
        // TODO: hook gtk::GestureClick for toggle-mode click selection
        // (replaces overlay/juhradial-overlay.py:mousePressEvent).

        RadialWidget {
            drawing_area: area,
        }
    }
}

impl Default for RadialWidget {
    fn default() -> Self {
        Self::new()
    }
}
