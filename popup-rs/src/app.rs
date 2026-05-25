//! iced application: State, Message, boot, update, view, subscription.
//!
//! Data flow:
//!   boot()        — loads config + fetches device state; returns initial State.
//!   update()      — handles Messages, returns Task<Message>.
//!   view()        — delegates to view::popup_view().
//!   subscription()— merges three streams: config watcher, GSettings poll,
//!                   device-state refresh timer.
//!
//! Positioning: after the first frame the popup asks the GNOME cursor
//! extension to move it below the indicator (same pattern as overlay-rs).
//! Volume-on-scroll and click-outside-to-dismiss are handled here.

use iced::widget::mouse_area;
use iced::{Element, Subscription, Task};
use juhradial_shared::AppConfig;
use juhradial_widgets::palette::Palette;
use std::collections::HashMap;
use tracing::{info, warn};

use crate::actions::{self, Action, DeviceState};
use crate::cli::Args;
use crate::gsettings_bridge::BatteryColors;
use crate::view;
use crate::POPUP_W;

const APP_ID: &str = "org.juhradial.popup";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub struct State {
    pub config: AppConfig,
    pub palette: Palette,
    pub args: Args,
    /// Device state from the daemon, `None` until the first fetch completes.
    pub device: Option<DeviceState>,
    /// Battery colours from GSettings (indicator extension prefs).
    pub battery_colors: BatteryColors,
    /// Current toggle states (id → on/off). Populated from device state +
    /// config; updated optimistically on user interaction.
    pub toggle_states: HashMap<String, bool>,
    /// Currently active Easy-Switch host (0-indexed).
    pub active_host: Option<u8>,
    /// Current DPI (from device state, updated on slider move).
    pub dpi: u16,
    /// Current scroll sensitivity (1–10).
    pub scroll_sensitivity: u8,
    /// Current haptic intensity (0–5).
    pub haptic_intensity: u8,
    /// Current pointer acceleration (-1.0 – 1.0).
    pub pointer_accel: f32,
    /// Whether the popup window currently has keyboard/pointer focus.
    pub focused: bool,
    /// True after the first `Positioned` response — suppresses duplicate moves.
    positioned: bool,
}

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// Initial device-state fetch completed.
    DeviceFetched(Option<DeviceState>),
    /// Config file changed on disk.
    ConfigReloaded(Box<AppConfig>),
    /// GSettings battery-colour prefs changed.
    BatteryColorsChanged(BatteryColors),
    /// Periodic device-state refresh (every 30 s while the popup is open).
    DeviceRefreshTick,
    /// Result of the MoveOverlay D-Bus call after the first frame.
    Positioned(bool),
    /// User toggled a quick-toggle row.
    ToggleAction(String, bool),
    /// User moved the DPI slider.
    SliderDpi(u16),
    /// User moved the scroll slider.
    SliderScroll(u8),
    /// User moved the haptic intensity slider.
    SliderHapticIntensity(u8),
    /// User moved the pointer acceleration slider.
    SliderAccel(f32),
    /// User clicked an Easy-Switch host button.
    SwitchHost(u8),
    /// An action dispatched to the daemon finished (success/failure).
    ActionResult(Result<(), String>),
    /// User clicked the Settings footer button.
    OpenSettings,
    /// Mouse-wheel scroll — volume control when popup is focused.
    WheelScroll(i32),
    /// Window focus changed.
    WindowFocused,
    WindowUnfocused,
    /// Click outside the popup content area → dismiss.
    Dismiss,
    /// Fire-and-forget Task sentinel.
    Noop,
}

// ---------------------------------------------------------------------------
// boot
// ---------------------------------------------------------------------------

pub fn boot(args: Args) -> (State, Task<Message>) {
    let config = match crate::config_watcher::load() {
        Ok(c) => c,
        Err(e) => {
            warn!("could not load config ({e}); using defaults");
            AppConfig::default()
        }
    };

    let palette = Palette::resolve(&config.theme);

    let state = State {
        palette,
        args: args.clone(),
        device: None,
        battery_colors: BatteryColors::default(),
        toggle_states: HashMap::new(),
        active_host: None,
        dpi: 1600,
        scroll_sensitivity: 5,
        haptic_intensity: 2,
        pointer_accel: 0.0,
        focused: false,
        positioned: false,
        config,
    };

    // Kick off an async device-state fetch immediately.
    let fetch_task = Task::perform(actions::fetch_device_state(), Message::DeviceFetched);

    // Position the popup below the indicator on the very next frame.
    let position_task = make_position_task(&args);

    (state, Task::batch([fetch_task, position_task]))
}

fn make_position_task(args: &Args) -> Task<Message> {
    let (anchor_x, anchor_y) = match args.panel_rect {
        Some(r) => {
            let x = (r.x + r.w / 2 - (POPUP_W as i32) / 2).max(0);
            let y = r.y + r.h;
            (x, y)
        }
        None => (0, 0),
    };

    Task::perform(
        juhradial_window::cursor_helper::move_overlay(
            APP_ID.to_string(),
            anchor_x,
            anchor_y,
            -1,
        ),
        Message::Positioned,
    )
}

// ---------------------------------------------------------------------------
// update
// ---------------------------------------------------------------------------

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::DeviceFetched(dev) => {
            if let Some(d) = dev {
                // Note: DPI is not yet exposed by GetActiveDeviceState; use a
                // sensible default until the daemon adds it.
                state.dpi = 1600;
                state.device = Some(d);
            }
            Task::none()
        }

        Message::DeviceRefreshTick => {
            // Re-fetch device state on the periodic timer tick.
            Task::perform(actions::fetch_device_state(), Message::DeviceFetched)
        }

        Message::ConfigReloaded(cfg) => {
            info!("popup: config reloaded");
            state.palette = Palette::resolve(&cfg.theme);
            state.config = *cfg;
            Task::none()
        }

        Message::BatteryColorsChanged(colors) => {
            state.battery_colors = colors;
            Task::none()
        }

        Message::Positioned(ok) => {
            if !ok && !state.positioned {
                warn!(
                    "MoveOverlay failed — extension not running? \
                     Popup will open wherever Mutter placed it."
                );
            }
            state.positioned = true;
            Task::none()
        }

        Message::ToggleAction(id, value) => {
            // Optimistic update — flip the local state immediately so
            // the UI responds without waiting for the D-Bus round-trip.
            state.toggle_states.insert(id.clone(), value);

            let action = match id.as_str() {
                "gaming" => Some(Action::Gaming(value)),
                "haptics" => Some(Action::Haptics(value)),
                "radial" => Some(Action::Radial(value)),
                "smart" => Some(Action::Smart(value)),
                "flow" => Some(Action::Flow(value)),
                "highlight" => Some(Action::Highlight(value)),
                unknown => {
                    warn!("ToggleAction: unknown id {unknown:?}");
                    None
                }
            };

            if let Some(a) = action {
                let close = state.config.popup.close_on_action;
                Task::batch([
                    Task::perform(actions::apply(a), Message::ActionResult),
                    if close {
                        iced::exit()
                    } else {
                        Task::none()
                    },
                ])
            } else {
                Task::none()
            }
        }

        Message::SliderDpi(dpi) => {
            state.dpi = dpi;
            Task::perform(actions::apply(Action::Dpi(dpi)), Message::ActionResult)
        }

        Message::SliderScroll(level) => {
            state.scroll_sensitivity = level;
            Task::perform(
                actions::apply(Action::Scroll(level)),
                Message::ActionResult,
            )
        }

        Message::SliderHapticIntensity(level) => {
            state.haptic_intensity = level;
            Task::perform(
                actions::apply(Action::HapticIntensity(level)),
                Message::ActionResult,
            )
        }

        Message::SliderAccel(accel) => {
            state.pointer_accel = accel;
            Task::perform(
                actions::apply(Action::Accel(accel)),
                Message::ActionResult,
            )
        }

        Message::SwitchHost(host) => {
            state.active_host = Some(host);
            let close = state.config.popup.close_on_action;
            Task::batch([
                Task::perform(
                    actions::apply(Action::EasySwitch(host)),
                    Message::ActionResult,
                ),
                if close { iced::exit() } else { Task::none() },
            ])
        }

        Message::ActionResult(res) => {
            if let Err(e) = res {
                warn!("popup action failed: {e}");
            }
            Task::none()
        }

        Message::OpenSettings => {
            // Spawn the settings binary as a sibling or from PATH.
            let settings_bin = sibling_or_path("juhradial-settings");
            tokio::task::spawn(async move {
                match tokio::process::Command::new(&settings_bin).spawn() {
                    Ok(_) => info!("launched {settings_bin}"),
                    Err(e) => warn!("could not launch {settings_bin}: {e}"),
                }
            });
            iced::exit()
        }

        Message::WheelScroll(dir) => {
            if state.focused && state.config.popup.volume_on_scroll {
                let arg = if dir > 0 { "5%+" } else { "5%-" };
                return Task::perform(
                    async move {
                        let _ = tokio::process::Command::new("wpctl")
                            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", arg])
                            .status()
                            .await;
                    },
                    |_| Message::Noop,
                );
            }
            Task::none()
        }

        Message::WindowFocused => {
            state.focused = true;
            Task::none()
        }

        Message::WindowUnfocused => {
            state.focused = false;
            // Dismiss the popup when focus leaves — matches GTK popovers behaviour.
            iced::exit()
        }

        Message::Dismiss => iced::exit(),

        Message::Noop => Task::none(),
    }
}

// ---------------------------------------------------------------------------
// view
// ---------------------------------------------------------------------------

pub fn view(state: &State) -> Element<'_, Message> {
    // Wrap the popup content in a fullscreen mouse_area that catches
    // clicks outside the card and dismisses the popup.
    mouse_area(view::popup_view(state))
        .on_press(Message::Noop) // clicks inside the content pass through
        .into()
}

// ---------------------------------------------------------------------------
// subscription
// ---------------------------------------------------------------------------

pub fn subscription(_state: &State) -> Subscription<Message> {
    use iced::time;
    Subscription::batch([
        // Config file inotify watcher.
        Subscription::run(crate::config_watcher::watch_stream)
            .map(|cfg| Message::ConfigReloaded(Box::new(cfg))),
        // GSettings battery-colour poll (1 s).
        Subscription::run(crate::gsettings_bridge::poll_stream).map(Message::BatteryColorsChanged),
        // Refresh device state every 30 s.
        time::every(std::time::Duration::from_secs(30)).map(|_| Message::DeviceRefreshTick),
        // Window focus events for volume-on-scroll gating and auto-dismiss.
        iced::window::events().map(|(_id, event)| match event {
            iced::window::Event::Focused => Message::WindowFocused,
            iced::window::Event::Unfocused => Message::WindowUnfocused,
            _ => Message::Noop,
        }),
    ])
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn sibling_or_path(name: &str) -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(name);
            if candidate.exists() {
                return candidate.display().to_string();
            }
        }
    }
    name.to_string()
}
