//! iced application: state, view, update, subscription wiring.
//!
//! The overlay is a single iced application that owns:
//!   * a `RadialState` (active theme + slices + per-slice highlight
//!     + visible? toggle for the menu),
//!   * a `Painter` (struct that implements `canvas::Program` and
//!     does the cairo-style frame painting via iced primitives),
//!   * a tokio-free zbus listener that forwards daemon signals into
//!     `Message::Overlay(OverlayEvent)`.
//!
//! Positioning happens after each `Show` event: we fire-and-forget a
//! D-Bus call to the GNOME extension's `MoveOverlay` method. The
//! extension places our xdg-toplevel at the cursor; iced never sees
//! coordinates the way the gtk4-layer-shell prototype tried to.

use iced::widget::canvas::Canvas;
use iced::window;
use iced::{Color, Element, Length, Size, Subscription, Task};
use tracing::{debug, error, info, warn};

use juhradial_shared::AppConfig;

use crate::dbus::OverlayEvent;
use crate::geometry::WINDOW_SIZE;
use crate::radial::{RadialState, Painter};

const APP_ID: &str = "org.kde.juhradialmx.overlay";

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    Overlay(OverlayEvent),
    /// Result of asking the GNOME extension to position the window.
    /// Used only for logging / future retries.
    Positioned(bool),
    /// Toggle-mode cursor moved over the canvas. Coords are widget-
    /// local pixels (origin at canvas top-left).
    ToggleCursor { x: f64, y: f64 },
    /// Toggle-mode left-click — dispatch the highlighted slice and
    /// close the menu.
    ToggleClickSelect,
    /// Toggle-mode dismiss without dispatching (right-click / Esc /
    /// click outside the menu).
    ToggleDismiss,
    /// Mouse-wheel scroll over the centre puck — cycles to the
    /// next/previous radial-menu page (positive = next, negative
    /// = previous). Only emitted in toggle mode where there's a
    /// real cursor over the canvas.
    CyclePage(i32),
    /// Result of asking the GNOME extension for the currently
    /// focused window's class. Fired immediately after a `Show`
    /// event so the menu can swap to the matching app-context
    /// page. `None` = extension missing or no app focused.
    FocusedClassResolved(Option<String>),
    /// Config reload from inotify watcher — replace theme + slices
    /// in the live state without restarting the overlay.
    ConfigReloaded(juhradial_shared::AppConfig),
}

pub fn run() -> iced::Result {
    // Build window settings as a struct so we can set
    // `platform_specific.application_id` — this becomes the
    // xdg-toplevel app_id on Wayland, which is how the
    // juhradial-cursor GNOME extension's MoveOverlay method finds
    // our window. Without this, the extension's WM_CLASS lookup
    // fails and the menu opens wherever Mutter chose.
    let mut window = iced::window::Settings::default();
    window.size = Size::new(WINDOW_SIZE as f32, WINDOW_SIZE as f32);
    window.decorations = false;
    window.transparent = true;
    window.resizable = false;
    window.level = window::Level::AlwaysOnTop;
    window.platform_specific.application_id = APP_ID.to_string();

    iced::application(boot, update, view)
        .title("JuhRadial MX")
        .window(window)
        .style(|_state, _theme| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: Color::WHITE,
        })
        .subscription(subscription)
        .run()
}

fn boot() -> RadialState {
    let config = match crate::config::load() {
        Ok(c) => c,
        Err(e) => {
            warn!("could not load config ({e}); using defaults");
            AppConfig::default()
        }
    };
    RadialState::new(&config)
}

fn update(state: &mut RadialState, message: Message) -> Task<Message> {
    match message {
        Message::Tick => {
            state.advance_animations();
            Task::none()
        }
        Message::Overlay(OverlayEvent::Show { x, y }) => {
            debug!(x, y, "Show event from daemon");
            state.show();
            // Daemon's cursor coord is the *cursor position*; we
            // want the window *centred* on it. MoveOverlay places
            // the window's top-left, so subtract half the window
            // size before sending. Pass monitor=-1 (absolute stage
            // coords) — the extension figures out which monitor
            // contains the requested point.
            let half = (WINDOW_SIZE / 2.0) as i32;
            // Run the position call AND the focused-class query in
            // parallel — both are independent zbus round-trips, so
            // batching them keeps the perceived open latency
            // bounded by the slower of the two (~5 ms each on local
            // session bus). The class result swaps to the matching
            // app-context page during the menu's open fade-in.
            Task::batch([
                Task::perform(
                    crate::ext_positioner::move_overlay(
                        APP_ID.to_string(),
                        x - half,
                        y - half,
                        -1,
                    ),
                    Message::Positioned,
                ),
                Task::perform(
                    crate::ext_positioner::get_focused_window_class(APP_ID.to_string()),
                    Message::FocusedClassResolved,
                ),
            ])
        }
        Message::Overlay(OverlayEvent::Hide) => {
            debug!("Hide event from daemon");
            state.hide();
            Task::none()
        }
        Message::Overlay(OverlayEvent::CursorMoved { dx, dy }) => {
            state.on_cursor_moved(dx, dy);
            Task::none()
        }
        Message::Positioned(success) => {
            if !success {
                warn!(
                    "MoveOverlay failed — extension couldn't find window with app_id={}; \
                     check that juhradial-cursor extension is enabled",
                    APP_ID
                );
            }
            Task::none()
        }
        Message::ToggleCursor { x, y } => {
            state.on_toggle_cursor(x, y);
            Task::none()
        }
        Message::ToggleClickSelect => {
            debug!("Toggle-mode click select");
            state.click_select();
            Task::none()
        }
        Message::ToggleDismiss => {
            debug!("Toggle-mode dismiss");
            state.dismiss();
            Task::none()
        }
        Message::CyclePage(direction) => {
            debug!(direction, "Cycle radial-menu page");
            state.cycle_page(direction);
            Task::none()
        }
        Message::FocusedClassResolved(class) => {
            debug!(?class, "Focused window class resolved");
            state.apply_focused_class(class);
            Task::none()
        }
        Message::ConfigReloaded(cfg) => {
            info!("config reloaded — refreshing theme + slices");
            state.reload_from(&cfg);
            Task::none()
        }
    }
}

fn view(state: &RadialState) -> Element<'_, Message> {
    Canvas::new(Painter::new(state))
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32))
        .into()
}

fn subscription(_state: &RadialState) -> Subscription<Message> {
    // Three streams merged into the same Message channel:
    //   * D-Bus listener — translates the daemon's three signal
    //     streams into OverlayEvent values.
    //   * Inotify config watcher — yields a fresh AppConfig each
    //     time `~/.config/juhradial/config.json` is saved.
    //   * 60 Hz frame ticker — keeps animations smooth while a
    //     menu is visible. (Cheap when nothing animates because
    //     update() returns Task::none() immediately.)
    Subscription::batch([
        Subscription::run(crate::dbus::stream).map(Message::Overlay),
        Subscription::run(crate::config::watch_stream).map(Message::ConfigReloaded),
        iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Tick),
    ])
}

#[allow(dead_code)]
fn _ensure_link(_e: &OverlayEvent) {
    error!("only here so the OverlayEvent path is referenced from app");
    info!("ditto");
}
