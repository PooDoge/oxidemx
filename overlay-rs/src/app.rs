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
    /// Sentinel for fire-and-forget Tasks whose completion we
    /// don't need to react to (e.g. haptic pulses).
    Noop,
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
        Message::Noop => Task::none(),
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
            // Three parallel D-Bus round-trips: window position,
            // focus query, and the menu_appear haptic pulse.
            // Batching keeps perceived open latency bounded by
            // the slowest of the three.
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
                Task::perform(
                    crate::haptic_client::trigger_haptic("menu_appear".to_string()),
                    |_| Message::Noop,
                ),
            ])
        }
        Message::Overlay(OverlayEvent::Hide) => {
            debug!("Hide event from daemon");
            // Decide haptic outcome BEFORE hide() resets target.
            // - actionable target → confirm
            // - target that can't actually dispatch → invalid
            // - no target (tap to toggle) → silent (menu_appear
            //   already played; nothing was attempted)
            let outcome = haptic_outcome_for(state);
            state.hide();
            haptic_outcome_task(outcome)
        }
        Message::Overlay(OverlayEvent::CursorMoved { dx, dy }) => {
            // Compare target_slice before/after so we can fire
            // a NotifySliceHover only when the user crosses into
            // a *new* slot — otherwise the daemon would get a
            // spam of pulses on every mouse-move event.
            let before = state.target_slice();
            state.on_cursor_moved(dx, dy);
            haptic_on_target_change(before, state.target_slice())
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
            // Same slice-change debounce as drag-mode CursorMoved.
            let before = state.target_slice();
            state.on_toggle_cursor(x, y);
            haptic_on_target_change(before, state.target_slice())
        }
        Message::ToggleClickSelect => {
            debug!("Toggle-mode click select");
            // Same outcome split as Hide — confirm if a real
            // action will dispatch, invalid if the targeted slice
            // can't actually do anything, silent if no slice.
            let outcome = haptic_outcome_for(state);
            state.click_select();
            haptic_outcome_task(outcome)
        }
        Message::ToggleDismiss => {
            debug!("Toggle-mode dismiss");
            state.dismiss();
            Task::none()
        }
        Message::CyclePage(direction) => {
            debug!(direction, "Cycle radial-menu page");
            // cycle_page is a no-op when fewer than two pages are
            // in the cycle, so detect actual transitions by
            // comparing the active page index before/after.
            let before = state.active_page;
            state.cycle_page(direction);
            if state.active_page != before {
                Task::perform(
                    crate::haptic_client::trigger_haptic("page_change".to_string()),
                    |_| Message::Noop,
                )
            } else {
                Task::none()
            }
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

/// Outcome of a dispatch attempt — drives which haptic event
/// to fire on Hide / ToggleClickSelect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DispatchOutcome {
    /// No slice targeted — tap-to-toggle, click on empty space.
    NoTarget,
    /// Slice targeted with a non-empty command and a visible
    /// predicate — `confirm` haptic fires.
    Actionable,
    /// Slice targeted but its command is empty OR its visible_if
    /// predicate evaluates false — `invalid` haptic fires.
    Unactionable,
}

/// Inspect the current state to decide what would happen if we
/// dispatched right now. Mirrors the actionability checks in
/// `RadialState::dispatch_and_close` (sub-item beats parent;
/// visible_if false → unactionable; empty command → unactionable).
fn haptic_outcome_for(state: &RadialState) -> DispatchOutcome {
    // Sub-item dispatch wins over the parent slice when a submenu
    // sub-item is highlighted.
    if let Some(sub) = state.submenu.as_ref() {
        if let Some(child_idx) = sub.highlighted {
            if let Some(parent) = state.slices.get(sub.parent) {
                if let Some(child) = parent.submenu.get(child_idx) {
                    return classify(child);
                }
            }
        }
    }
    let idx = match state.target_slice() {
        Some(i) => i,
        None => return DispatchOutcome::NoTarget,
    };
    let slice = match state.slices.get(idx) {
        Some(s) => s,
        None => return DispatchOutcome::NoTarget,
    };
    classify(slice)
}

fn classify(slice: &juhradial_shared::Slice) -> DispatchOutcome {
    let visible = slice
        .visible_if
        .as_ref()
        .map(|c| c.eval())
        .unwrap_or(true);
    if !visible {
        return DispatchOutcome::Unactionable;
    }
    // Submenu slices are "actionable" in that hovering them is
    // useful, but pressing them with no sub-item highlighted
    // doesn't dispatch anything — treat as unactionable so the
    // user gets `invalid` feedback for that confused state.
    if matches!(slice.kind, juhradial_shared::ActionKind::Submenu) && !slice.submenu.is_empty() {
        return DispatchOutcome::Unactionable;
    }
    if slice.command.trim().is_empty() {
        return DispatchOutcome::Unactionable;
    }
    DispatchOutcome::Actionable
}

/// Convert a DispatchOutcome into the corresponding haptic Task.
fn haptic_outcome_task(outcome: DispatchOutcome) -> Task<Message> {
    let event = match outcome {
        DispatchOutcome::Actionable => "confirm",
        DispatchOutcome::Unactionable => "invalid",
        DispatchOutcome::NoTarget => return Task::none(),
    };
    Task::perform(
        crate::haptic_client::trigger_haptic(event.to_string()),
        |_| Message::Noop,
    )
}

/// Fire a slice-change haptic when the cursor crosses into a new
/// slot. No-op when the user enters empty space (target → None)
/// — only positive transitions get a pulse, otherwise the daemon
/// would burn the motor on every drag-to-cancel.
///
/// Calls `TriggerHaptic("slice_change")` directly rather than
/// `NotifySliceHover(idx)` because the daemon's
/// `notify_slice_hover` handler only emits a SliceSelected D-Bus
/// signal — it doesn't touch the haptic motor. `trigger_haptic`
/// is the haptic-firing method.
fn haptic_on_target_change(before: Option<usize>, after: Option<usize>) -> Task<Message> {
    if before == after {
        return Task::none();
    }
    if after.is_none() {
        return Task::none();
    }
    Task::perform(
        crate::haptic_client::trigger_haptic("slice_change".to_string()),
        |_| Message::Noop,
    )
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
