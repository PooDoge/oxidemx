//! The radial-menu drawing widget. Wraps a `gtk::DrawingArea` and
//! holds the per-frame state (active theme, slice list, hover progress
//! per slice). Render passes are dispatched to `crate::render::slices`.
//!
//! State lives in a `RefCell<RadialState>` so callers (the D-Bus event
//! pump, the config watcher, the editor preview) can mutate it
//! without juggling channels for every minor update. The widget runs
//! on the GTK main thread so single-threaded interior mutability is
//! the right tool here.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use juhradial_shared::{AppConfig, Slice};

use crate::geometry::{Geometry, WINDOW_SIZE};
use crate::theme::ActiveTheme;

#[derive(Debug, Clone)]
pub struct RadialState {
    pub theme: ActiveTheme,
    pub slices: Vec<Slice>,
    /// Per-slice hover progress in `[0.0, 1.0]`. Index `i` corresponds
    /// to slot `i` clockwise from the top.
    pub highlights: [f64; 8],
    /// Currently highlighted slice index, or `None` when the cursor
    /// is in the centre deadzone or outside the menu radius.
    pub highlighted: Option<usize>,
    /// Toggle-mode means the menu stays open after a quick tap and
    /// closes on the next click; drag-mode closes on button release.
    pub toggle_mode: bool,
}

impl RadialState {
    pub fn new(config: &AppConfig) -> Self {
        let theme = ActiveTheme::resolve(&config.theme);
        let slices = config.radial_menu.slices.clone();
        RadialState {
            theme,
            slices,
            highlights: [0.0; 8],
            highlighted: None,
            toggle_mode: false,
        }
    }
}

pub struct RadialWidget {
    pub drawing_area: gtk::DrawingArea,
    pub state: Rc<RefCell<RadialState>>,
}

impl RadialWidget {
    pub fn new(state: Rc<RefCell<RadialState>>) -> Self {
        let area = gtk::DrawingArea::builder()
            .content_width(WINDOW_SIZE as i32)
            .content_height(WINDOW_SIZE as i32)
            .can_focus(false)
            .build();

        let state_for_draw = state.clone();
        area.set_draw_func(move |_area, cr, _w, _h| {
            // Transparent background; the layer-shell surface already
            // has its own alpha channel.
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            cr.set_operator(cairo::Operator::Source);
            let _ = cr.paint();
            cr.set_operator(cairo::Operator::Over);

            let s = state_for_draw.borrow();
            let geom = Geometry::default();
            crate::render::slices::draw_slices(
                cr,
                &geom,
                &s.slices,
                &s.theme,
                &s.highlights,
            );
            crate::render::slices::draw_center(cr, &geom, &s.theme);
        });

        // TODO: hook gtk::EventControllerMotion for toggle-mode hover
        // (see `overlay/juhradial-overlay.py:mouseMoveEvent`).
        //
        // TODO: hook gtk::GestureClick for toggle-mode click selection
        // (see `overlay/juhradial-overlay.py:mousePressEvent`).

        RadialWidget {
            drawing_area: area,
            state,
        }
    }

    /// Drag-mode cursor delta from the daemon's CursorMoved signal.
    /// `dx`/`dy` are accumulated REL_X / REL_Y values relative to the
    /// gesture-button press point.
    pub fn on_cursor_moved(&self, dx: i32, dy: i32) {
        let new_slice = crate::input::slice_index_at(
            dx as f64,
            dy as f64,
            crate::geometry::CENTER_ZONE_RADIUS,
            crate::geometry::MENU_RADIUS,
        );

        let mut s = self.state.borrow_mut();
        if s.highlighted != new_slice {
            // Drop the old highlight and bring up the new one.
            if let Some(prev) = s.highlighted {
                s.highlights[prev] = 0.0;
            }
            if let Some(idx) = new_slice {
                s.highlights[idx] = 1.0;
            }
            s.highlighted = new_slice;
            self.drawing_area.queue_draw();
        }
    }

    /// Refresh the slice list + theme from a freshly-loaded config —
    /// called after the config file is written by the editor or
    /// edited by hand.
    pub fn reload_from(&self, config: &AppConfig) {
        let mut s = self.state.borrow_mut();
        s.theme = ActiveTheme::resolve(&config.theme);
        s.slices = config.radial_menu.slices.clone();
        // Reset transient state so the next show starts clean.
        s.highlights = [0.0; 8];
        s.highlighted = None;
        self.drawing_area.queue_draw();
    }
}
