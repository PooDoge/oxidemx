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
}

pub fn run() -> iced::Result {
    iced::application(boot, update, view)
        .title("JuhRadial MX")
        .window_size(Size::new(WINDOW_SIZE as f32, WINDOW_SIZE as f32))
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .level(window::Level::AlwaysOnTop)
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
            // Hand off to the GNOME extension to position the
            // window — Mutter won't honour positioning requests
            // from a regular xdg-shell client.
            Task::perform(
                crate::ext_positioner::move_overlay(APP_ID.to_string(), x, y, -1),
                Message::Positioned,
            )
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
    }
}

fn view(state: &RadialState) -> Element<'_, Message> {
    Canvas::new(Painter::new(state))
        .width(Length::Fixed(WINDOW_SIZE as f32))
        .height(Length::Fixed(WINDOW_SIZE as f32))
        .into()
}

fn subscription(_state: &RadialState) -> Subscription<Message> {
    // Two streams merged into the same Message channel:
    //   * D-Bus listener — translates the daemon's three signal
    //     streams into OverlayEvent values.
    //   * 60 Hz frame ticker — keeps animations smooth while a
    //     menu is visible. (Cheap when nothing animates because
    //     update() returns Task::none() immediately.)
    Subscription::batch([
        Subscription::run(crate::dbus::stream).map(Message::Overlay),
        iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Tick),
    ])
}

#[allow(dead_code)]
fn _ensure_link(_e: &OverlayEvent) {
    error!("only here so the OverlayEvent path is referenced from app");
    info!("ditto");
}
