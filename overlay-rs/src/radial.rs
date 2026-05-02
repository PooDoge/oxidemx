//! Application state + canvas Painter for the radial menu.
//!
//! `RadialState` is the model `iced::application(boot, update, view)`
//! drives. `Painter` is a per-frame snapshot that implements
//! `canvas::Program` and renders the wedges / icons / centre puck
//! into `iced::widget::canvas::Frame` (cairo-equivalent calls in
//! pure Rust).

use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Event, Point, Rectangle, Renderer, Theme};
use juhradial_shared::{theme::parse_hex_rgba, AppConfig, Slice};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::geometry::{Geometry as RadialGeometry, MENU_RADIUS, WINDOW_SIZE};
use crate::render::icons::IconCache;
use crate::theme::ActiveTheme;

/// Maximum press-to-release duration that still counts as a "tap" —
/// mirrors the Python overlay's TAP_THRESHOLD_MS = 250.
const TAP_THRESHOLD: Duration = Duration::from_millis(250);

const SLICE_DEGREES: f32 = 45.0;
const RING_OUTER_INSET: f32 = 6.0;
const RING_INNER_INSET: f32 = 6.0;
const ICON_BG_RADIUS: f32 = 26.0;

/// Per-slice highlight progress in [0.0, 1.0]. The 60Hz tick from
/// `iced::time::every` advances each entry toward its target value
/// using `ease_out_quad`-shaped steps.
#[derive(Debug, Clone, Copy)]
struct Animation {
    current: f32,
    target: f32,
}

impl Animation {
    const fn at(value: f32) -> Self {
        Self { current: value, target: value }
    }
    fn step(&mut self) {
        let delta = self.target - self.current;
        if delta.abs() < 0.001 {
            self.current = self.target;
            return;
        }
        // Geometric easing — fast at first, settles smoothly. Each
        // tick covers ~25% of the remaining distance, giving a
        // visually pleasing 4-6 frame transition between hover
        // states without the jitter a linear ramp would produce.
        self.current += delta * 0.25;
    }
}

/// Top-level model for the iced app. Owns everything view() needs.
pub struct RadialState {
    pub theme: ActiveTheme,
    pub slices: Vec<Slice>,
    /// Per-slice hover progress. Index `i` corresponds to slot `i`
    /// clockwise from the top.
    highlights: [Animation; 8],
    /// Currently-targeted slice (or `None` for centre/outside).
    /// Drives `highlights` via `step()`.
    target_slice: Option<usize>,
    /// Whether the menu is currently visible (Show received, Hide
    /// not yet).
    visible: bool,
    /// Per-overlay icon cache shared with the painter via `Rc`.
    icons: Rc<IconCache>,
    /// When the daemon's most recent `Show` arrived. Compared
    /// against the matching `Hide` to detect a quick tap (tap →
    /// toggle mode; otherwise drag-select fires the highlighted
    /// slice and closes).
    show_time: Option<Instant>,
    /// Toggle mode is on after a quick tap. The menu stays visible
    /// after the daemon's `Hide`, listens for OS mouse events
    /// (cursor move + clicks via `canvas::Program::update`), and
    /// only closes on click or escape.
    toggle_mode: bool,
}

impl std::fmt::Debug for RadialState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadialState")
            .field("theme", &self.theme)
            .field("slices.len", &self.slices.len())
            .field("target_slice", &self.target_slice)
            .field("visible", &self.visible)
            .field("toggle_mode", &self.toggle_mode)
            .finish()
    }
}

impl RadialState {
    pub fn new(config: &AppConfig) -> Self {
        let theme = ActiveTheme::resolve(&config.theme);
        let slices = config.radial_menu.slices.clone();
        RadialState {
            theme,
            slices,
            highlights: [Animation::at(0.0); 8],
            target_slice: None,
            visible: false,
            icons: Rc::new(IconCache::new()),
            show_time: None,
            toggle_mode: false,
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.show_time = Some(Instant::now());
        self.toggle_mode = false;
        // Reset any stale highlights from a previous show.
        for a in &mut self.highlights {
            a.target = 0.0;
        }
        self.target_slice = None;
    }

    /// Handle the daemon's `Hide` signal (gesture button release).
    ///
    /// Two paths:
    ///   * **Tap** (release within `TAP_THRESHOLD` of show, no
    ///     drag highlight) → enter toggle mode. The menu stays
    ///     visible; OS mouse events (forwarded by
    ///     `Painter::update`) drive the selection until the user
    ///     clicks or escapes.
    ///   * **Drag-select** → run the highlighted slice's action
    ///     and close.
    pub fn hide(&mut self) {
        let was_drag_select = self.target_slice.is_some();
        let dur = self.show_time.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
        if !was_drag_select && dur < TAP_THRESHOLD {
            // Tap detected — switch to toggle mode rather than closing.
            self.toggle_mode = true;
            tracing::debug!(?dur, "tap → entering toggle mode");
            return;
        }
        self.dispatch_and_close();
    }

    /// Toggle-mode click handler — fires the highlighted slice and
    /// closes the menu.
    pub fn click_select(&mut self) {
        self.dispatch_and_close();
    }

    /// Toggle-mode dismiss without dispatching (right click / Esc /
    /// click outside).
    pub fn dismiss(&mut self) {
        self.target_slice = None;
        for a in &mut self.highlights {
            a.target = 0.0;
        }
        self.visible = false;
        self.toggle_mode = false;
    }

    fn dispatch_and_close(&mut self) {
        self.visible = false;
        let selected = self.target_slice;
        if let Some(idx) = selected {
            if let Some(slice) = self.slices.get(idx) {
                let visible = slice
                    .visible_if
                    .as_ref()
                    .map(|c| c.eval())
                    .unwrap_or(true);
                if visible {
                    crate::actions::dispatch(slice);
                }
            }
        }
        for a in &mut self.highlights {
            a.target = 0.0;
        }
        self.target_slice = None;
        self.toggle_mode = false;
    }

    /// True when the menu is in toggle mode and the canvas should
    /// react to OS-level mouse events.
    pub fn is_toggle_mode(&self) -> bool {
        self.toggle_mode
    }

    /// Update the highlight from a toggle-mode cursor position.
    /// `local_x`/`local_y` are widget-local pixels (the canvas's
    /// own coordinate space — `(0,0)` at the top-left of the
    /// 484×484 surface, centre at `(WINDOW_SIZE/2, WINDOW_SIZE/2)`).
    pub fn on_toggle_cursor(&mut self, local_x: f64, local_y: f64) {
        let dx = local_x - (WINDOW_SIZE / 2.0);
        let dy = local_y - (WINDOW_SIZE / 2.0);
        let new_target = crate::input::slice_index_at(
            dx,
            dy,
            crate::geometry::CENTER_ZONE_RADIUS,
            crate::geometry::MENU_RADIUS,
        );
        if new_target != self.target_slice {
            if let Some(prev) = self.target_slice {
                self.highlights[prev].target = 0.0;
            }
            if let Some(next) = new_target {
                self.highlights[next].target = 1.0;
            }
            self.target_slice = new_target;
        }
    }

    /// Drag-mode delta from the daemon's CursorMoved signal.
    /// `dx, dy` are accumulated REL_X / REL_Y values from the
    /// gesture-button press point (NOT absolute screen coords).
    pub fn on_cursor_moved(&mut self, dx: i32, dy: i32) {
        let new_target = crate::input::slice_index_at(
            dx as f64,
            dy as f64,
            crate::geometry::CENTER_ZONE_RADIUS,
            crate::geometry::MENU_RADIUS,
        );
        if new_target != self.target_slice {
            // Drop the previously-targeted slice's highlight,
            // raise the new one's.
            if let Some(prev) = self.target_slice {
                self.highlights[prev].target = 0.0;
            }
            if let Some(next) = new_target {
                self.highlights[next].target = 1.0;
            }
            self.target_slice = new_target;
        }
    }

    /// Step the per-slice highlight animations one frame.
    pub fn advance_animations(&mut self) {
        for a in &mut self.highlights {
            a.step();
        }
    }

    /// Refresh from a freshly-loaded config (called by the inotify
    /// watcher after the editor saves).
    pub fn reload_from(&mut self, config: &AppConfig) {
        self.theme = ActiveTheme::resolve(&config.theme);
        self.slices = config.radial_menu.slices.clone();
        for a in &mut self.highlights {
            *a = Animation::at(0.0);
        }
        self.target_slice = None;
    }
}

/// Per-frame canvas state.
pub struct Painter<'a> {
    state: &'a RadialState,
}

impl<'a> Painter<'a> {
    pub fn new(state: &'a RadialState) -> Self {
        Self { state }
    }
}

impl<'a> canvas::Program<crate::app::Message> for Painter<'a> {
    type State = ();

    /// Forward toggle-mode mouse events as `Message`s the iced
    /// app loop dispatches into `RadialState`. We only react when
    /// the menu is in toggle mode — drag mode is driven entirely
    /// by the daemon's CursorMoved signals (see app.rs).
    fn update(
        &self,
        _canvas_state: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<crate::app::Message>> {
        if !self.state.visible || !self.state.is_toggle_mode() {
            return None;
        }
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let p = cursor.position_in(bounds)?;
                Some(Action::publish(crate::app::Message::ToggleCursor {
                    x: p.x as f64,
                    y: p.y as f64,
                }))
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) => match button {
                mouse::Button::Left => {
                    Some(Action::publish(crate::app::Message::ToggleClickSelect))
                }
                _ => Some(Action::publish(crate::app::Message::ToggleDismiss)),
            },
            Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) => {
                if matches!(
                    key,
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                ) {
                    Some(Action::publish(crate::app::Message::ToggleDismiss))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        if !self.state.visible {
            // Menu not requested — leave canvas transparent.
            return vec![frame.into_geometry()];
        }

        let geom = RadialGeometry::default();
        let center = Point::new(geom.cx as f32, geom.cy as f32);
        let palette = &self.state.theme.theme.colors;

        // Faint shadow halo so the disc reads against transparent
        // backgrounds.
        let halo = Path::circle(center, MENU_RADIUS as f32 + 6.0);
        frame.fill(&halo, Color::from_rgba(0.0, 0.0, 0.0, 0.35));

        let outer_r = (MENU_RADIUS as f32) - RING_OUTER_INSET;
        let inner_r = (geom.center_radius as f32) + RING_INNER_INSET;
        let icon_r = geom.icon_radius as f32;

        for i in 0..8 {
            let highlight = self.state.highlights[i].current;
            // Apply the slice's visible_if predicate. Slot stays
            // empty (we still draw an unfilled wedge so the ring is
            // continuous) when the predicate is false — preserves
            // muscle-memory layout, only hides the glyph.
            let slice_for_render = self.state.slices.get(i).filter(|s| {
                s.visible_if.as_ref().map(|c| c.eval()).unwrap_or(true)
            });
            crate::render::slices::draw_slice(
                &mut frame,
                center,
                inner_r,
                outer_r,
                icon_r,
                ICON_BG_RADIUS,
                i,
                slice_for_render,
                palette,
                highlight,
                &self.state.icons,
            );
        }

        crate::render::slices::draw_center(
            &mut frame,
            center,
            geom.center_radius as f32,
            palette,
        );

        vec![frame.into_geometry()]
    }
}

#[allow(dead_code)]
fn _link_window_size() -> u32 {
    WINDOW_SIZE as u32
}

#[allow(dead_code)]
fn _link_parse() -> Option<(f64, f64, f64, f64)> {
    parse_hex_rgba("#000000")
}

// Slice references kept private but visible to the painter via the
// state borrow above; this `_use` ensures cargo doesn't drop the
// type from the public surface when we add it elsewhere.
#[allow(dead_code)]
fn _slice_ref(_s: &Slice) {}
