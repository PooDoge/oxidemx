//! JuhRadial MX settings GUI.
//!
//! Layout mirrors the legacy juhradial settings dialog: a left
//! sidebar with the 9 top-level sections (Buttons by default), a
//! header, the section's main + side content in the middle, and a
//! footer with credits + exit. The on-disk channel to the live
//! overlay is unchanged — every edit is debounced + atomic-written
//! back to `~/.config/juhradial/config.json` and the overlay's
//! existing inotify watcher previews changes within ~150 ms.

mod tabs {
    pub mod animation;
    pub mod buttons;
    pub mod placeholder;
    pub mod settings_page;
    pub mod visuals;
}
mod persist;
mod widgets;

use iced::widget::{button, column, container, row, rule, scrollable, text, Space};
use iced::{Element, Length, Subscription, Task};
use juhradial_shared::{
    AnimationConfig, AppConfig, ElementAnimation, TransitionConfig, VisualSettings,
};
use std::time::{Duration, Instant};
use tracing::{info, warn};

// ============================================================================
// Tabs
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Buttons,
    PointScroll,
    Haptic,
    Devices,
    EasySwitch,
    Flow,
    Macros,
    Gaming,
    Settings,
}

impl Tab {
    pub fn label(&self) -> &'static str {
        match self {
            Tab::Buttons => "Buttons",
            Tab::PointScroll => "Point & Scroll",
            Tab::Haptic => "Haptic Feedback",
            Tab::Devices => "Devices",
            Tab::EasySwitch => "Easy-Switch",
            Tab::Flow => "Flow",
            Tab::Macros => "Macros",
            Tab::Gaming => "Gaming",
            Tab::Settings => "Settings",
        }
    }

    /// Single-glyph icon shown in the sidebar. Picked to read at
    /// a glance; the legacy UI uses bespoke SVGs which we'll
    /// substitute later when we add an icon resolver here too.
    pub fn glyph(&self) -> &'static str {
        match self {
            Tab::Buttons => "M",
            Tab::PointScroll => "S",
            Tab::Haptic => "H",
            Tab::Devices => "D",
            Tab::EasySwitch => "E",
            Tab::Flow => "F",
            Tab::Macros => "P",
            Tab::Gaming => "G",
            Tab::Settings => "*",
        }
    }

    pub const ALL: [Tab; 9] = [
        Tab::Buttons,
        Tab::PointScroll,
        Tab::Haptic,
        Tab::Devices,
        Tab::EasySwitch,
        Tab::Flow,
        Tab::Macros,
        Tab::Gaming,
        Tab::Settings,
    ];
}

// ============================================================================
// Messages
// ============================================================================

#[derive(Debug, Clone)]
pub enum Message {
    SwitchTab(Tab),
    SetVisual(VisualField, f32),
    SetTransition(AnimElement, AnimDirection, TransitionConfig),
    SetChainStagger(AnimElement, u32),
    ResetElementAnimation(AnimElement),
    ResetAll,
    /// Easy-Switch shortcut toggle (Buttons tab right column).
    SetEasySwitchShortcuts(bool),
    /// Quit the settings window.
    Exit,
    /// Debounced save tick — fires every 200 ms; if there's an
    /// unsaved edit older than 250 ms we flush to disk.
    SaveTick,
    /// Persist completed.
    Saved(Result<(), String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualField {
    MenuBackgroundOpacity,
    SliceHighlightOpacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimElement {
    Menu,
    Submenu,
    SliceHighlight,
}

impl AnimElement {
    pub fn label(&self) -> &'static str {
        match self {
            AnimElement::Menu => "Menu",
            AnimElement::Submenu => "Submenu",
            AnimElement::SliceHighlight => "Slice highlight",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            AnimElement::Menu => "The whole radial wheel — open + close.",
            AnimElement::Submenu => {
                "Sub-item arc that pops out when hovering a submenu slice."
            }
            AnimElement::SliceHighlight => {
                "Per-slice hover glow — fades in when the cursor enters a slice."
            }
        }
    }

    pub fn supports_chain(&self) -> bool {
        matches!(self, AnimElement::Submenu)
    }

    pub fn get<'a>(&self, anim: &'a AnimationConfig) -> &'a ElementAnimation {
        match self {
            AnimElement::Menu => &anim.menu,
            AnimElement::Submenu => &anim.submenu,
            AnimElement::SliceHighlight => &anim.slice_highlight,
        }
    }

    pub fn get_mut<'a>(&self, anim: &'a mut AnimationConfig) -> &'a mut ElementAnimation {
        match self {
            AnimElement::Menu => &mut anim.menu,
            AnimElement::Submenu => &mut anim.submenu,
            AnimElement::SliceHighlight => &mut anim.slice_highlight,
        }
    }

    pub fn default_for(&self) -> ElementAnimation {
        match self {
            AnimElement::Menu => ElementAnimation::menu_default(),
            AnimElement::Submenu => ElementAnimation::submenu_default(),
            AnimElement::SliceHighlight => ElementAnimation::slice_highlight_default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimDirection {
    Enter,
    Exit,
}

impl AnimDirection {
    pub fn label(&self) -> &'static str {
        match self {
            AnimDirection::Enter => "Enter",
            AnimDirection::Exit => "Exit",
        }
    }

    pub fn pick<'a>(&self, anim: &'a ElementAnimation) -> &'a TransitionConfig {
        match self {
            AnimDirection::Enter => &anim.enter,
            AnimDirection::Exit => &anim.exit,
        }
    }

    pub fn pick_mut<'a>(&self, anim: &'a mut ElementAnimation) -> &'a mut TransitionConfig {
        match self {
            AnimDirection::Enter => &mut anim.enter,
            AnimDirection::Exit => &mut anim.exit,
        }
    }
}

// ============================================================================
// State
// ============================================================================

pub struct State {
    pub config: AppConfig,
    pub tab: Tab,
    pub config_path: Option<std::path::PathBuf>,
    pub last_edit: Option<Instant>,
    pub saved_pending: bool,
    pub status: String,
}

impl Default for State {
    fn default() -> Self {
        let path = juhradial_shared::config::default_config_path();
        let config = path
            .as_ref()
            .and_then(|p| AppConfig::load_from(p).ok())
            .unwrap_or_default();
        State {
            config,
            tab: Tab::Buttons,
            config_path: path,
            last_edit: None,
            saved_pending: false,
            status: String::new(),
        }
    }
}

impl State {
    fn touch(&mut self) {
        self.last_edit = Some(Instant::now());
        self.saved_pending = true;
    }

    fn maybe_save(&mut self) -> Option<Task<Message>> {
        let last = self.last_edit?;
        if !self.saved_pending {
            return None;
        }
        if last.elapsed() < Duration::from_millis(250) {
            return None;
        }
        let path = self.config_path.clone()?;
        let cfg = self.config.clone();
        self.saved_pending = false;
        Some(Task::perform(persist::save(path, cfg), Message::Saved))
    }
}

fn boot() -> State {
    State::default()
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::SwitchTab(t) => {
            state.tab = t;
            Task::none()
        }
        Message::SetVisual(field, v) => {
            let v = v.clamp(0.0, 1.0);
            match field {
                VisualField::MenuBackgroundOpacity => {
                    state.config.radial_menu.visuals.menu_background_opacity = v;
                }
                VisualField::SliceHighlightOpacity => {
                    state.config.radial_menu.visuals.slice_highlight_opacity = v;
                }
            }
            state.touch();
            Task::none()
        }
        Message::SetTransition(elem, dir, cfg) => {
            let anim = elem.get_mut(&mut state.config.radial_menu.animation);
            *dir.pick_mut(anim) = cfg;
            state.touch();
            Task::none()
        }
        Message::SetChainStagger(elem, ms) => {
            let anim = elem.get_mut(&mut state.config.radial_menu.animation);
            anim.chain = Some(juhradial_shared::ChainConfig { stagger_ms: ms });
            state.touch();
            Task::none()
        }
        Message::ResetElementAnimation(elem) => {
            let anim = elem.get_mut(&mut state.config.radial_menu.animation);
            *anim = elem.default_for();
            state.touch();
            Task::none()
        }
        Message::ResetAll => {
            state.config.radial_menu.animation = AnimationConfig::default();
            state.config.radial_menu.visuals = VisualSettings::default();
            state.touch();
            Task::none()
        }
        Message::SetEasySwitchShortcuts(on) => {
            state.config.radial_menu.easy_switch_shortcuts = on;
            state.touch();
            Task::none()
        }
        Message::Exit => iced::window::latest().and_then(iced::window::close),
        Message::SaveTick => state.maybe_save().unwrap_or_else(Task::none),
        Message::Saved(Ok(())) => {
            info!("config saved");
            state.status = "Saved".to_string();
            Task::none()
        }
        Message::Saved(Err(e)) => {
            warn!("save failed: {e}");
            state.status = format!("Save error: {e}");
            Task::none()
        }
    }
}

// ============================================================================
// View — the shell
// ============================================================================

fn view(state: &State) -> Element<'_, Message> {
    let header = header_view();
    let sidebar = sidebar_view(state);
    let body: Element<Message> = match state.tab {
        Tab::Buttons => tabs::buttons::view(state),
        Tab::Settings => tabs::settings_page::view(state),
        Tab::PointScroll => tabs::placeholder::view(
            "Point & Scroll",
            "Pointer speed, scroll direction, and acceleration. \
             Coming soon — track in the daemon's button-mapping module.",
        ),
        Tab::Haptic => tabs::placeholder::view(
            "Haptic Feedback",
            "Per-event haptic intensity for slice changes, gesture recognition, \
             and dispatch. Coming soon.",
        ),
        Tab::Devices => tabs::placeholder::view(
            "Devices",
            "Paired Logitech devices + battery + firmware status. Coming soon.",
        ),
        Tab::EasySwitch => tabs::placeholder::view(
            "Easy-Switch",
            "Configure each Easy-Switch host slot (label, OS icon). Coming soon.",
        ),
        Tab::Flow => tabs::placeholder::view(
            "Flow",
            "Cross-machine cursor-and-clipboard hand-off. Coming soon.",
        ),
        Tab::Macros => tabs::placeholder::view(
            "Macros",
            "Record and edit macros that slices can dispatch via the daemon. \
             Coming soon.",
        ),
        Tab::Gaming => tabs::placeholder::view(
            "Gaming",
            "Per-game profiles + DPI overrides + low-latency mode. Coming soon.",
        ),
    };

    let main_area = row![
        sidebar,
        container(scrollable(container(body).padding(20)))
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .height(Length::Fill);

    let footer = footer_view(state);

    container(
        column![header, rule::horizontal(1), main_area, rule::horizontal(1), footer,]
            .spacing(0),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// ----------------------------------------------------------------------------
// Header
// ----------------------------------------------------------------------------

fn header_view<'a>() -> Element<'a, Message> {
    row![
        text("JuhRadial").size(20),
        text("MX").size(13),
        Space::new().width(Length::Fixed(16.0)),
        chip("MX MASTER 4"),
        Space::new().width(Length::Fill),
        button(text("Exit").size(12))
            .style(button::secondary)
            .on_press(Message::Exit),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(8)
    .padding(12)
    .into()
}

fn chip<'a>(label: &str) -> Element<'a, Message> {
    container(text(label.to_string()).size(11))
        .padding([4, 10])
        .style(container::bordered_box)
        .into()
}

// ----------------------------------------------------------------------------
// Sidebar
// ----------------------------------------------------------------------------

fn sidebar_view(state: &State) -> Element<'_, Message> {
    let mut col = column![].spacing(6).padding(12);
    for tab in Tab::ALL {
        col = col.push(sidebar_button(tab, state.tab == tab));
    }
    container(col)
        .width(Length::Fixed(220.0))
        .height(Length::Fill)
        .style(container::bordered_box)
        .into()
}

fn sidebar_button(tab: Tab, active: bool) -> Element<'static, Message> {
    let inner = row![
        container(text(tab.glyph()).size(13))
            .padding([2, 8])
            .style(container::bordered_box),
        text(tab.label()).size(13),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(10);

    let mut b = button(inner).width(Length::Fill).padding([8, 10]);
    if !active {
        b = b.style(button::secondary);
    }
    b.on_press(Message::SwitchTab(tab)).into()
}

// ----------------------------------------------------------------------------
// Footer
// ----------------------------------------------------------------------------

fn footer_view(state: &State) -> Element<'_, Message> {
    let path = state
        .config_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no config path)".into());

    let status: Element<Message> = if state.saved_pending {
        text("Editing… (autosaves shortly)").size(11).into()
    } else if !state.status.is_empty() {
        text(state.status.as_str()).size(11).into()
    } else {
        text("Idle.").size(11).into()
    };

    row![
        text("JuhLabs · Free & open source software").size(11),
        Space::new().width(Length::Fixed(16.0)),
        text(path).size(10),
        Space::new().width(Length::Fill),
        status,
    ]
    .align_y(iced::Alignment::Center)
    .spacing(8)
    .padding(10)
    .into()
}

// ============================================================================
// Subscription + main
// ============================================================================

fn subscription(_state: &State) -> Subscription<Message> {
    iced::time::every(Duration::from_millis(200)).map(|_| Message::SaveTick)
}

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut window = iced::window::Settings::default();
    window.size = iced::Size::new(1280.0, 820.0);
    window.platform_specific.application_id = "org.juhlabs.juhradial.settings".into();

    iced::application(boot, update, view)
        .title("JuhRadial Settings")
        .window(window)
        .subscription(subscription)
        .run()
}
