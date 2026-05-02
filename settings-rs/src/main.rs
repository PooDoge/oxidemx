//! JuhRadial MX settings GUI.
//!
//! A separate iced application that loads `~/.config/juhradial/config.json`,
//! exposes per-element animation + visual controls, and writes the file
//! back on every change. The overlay's inotify watcher picks up the
//! save within ~150 ms, so edits preview live with no D-Bus or any
//! extra IPC plumbing — the filesystem is the channel.
//!
//! Tabs:
//!   - Visuals     — menu background opacity + slice highlight opacity
//!   - Animation   — per-element (menu / submenu / slice highlight)
//!                   enter/exit transition controls + chain stagger
//!
//! Save is debounced ~250 ms so dragging a slider doesn't hammer
//! the filesystem (and doesn't trigger 60 reload events per second
//! at the overlay).

mod tabs {
    pub mod animation;
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

/// Which tab is currently visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Visuals,
    Animation,
}

#[derive(Debug, Clone)]
pub enum Message {
    SwitchTab(Tab),
    /// One of the visuals sliders moved — `(field, new_value)`.
    SetVisual(VisualField, f32),
    /// One of the animation controls changed — element, direction,
    /// + the new TransitionConfig (the panel rebuilds the whole
    /// config locally and ships it). Simpler than 30 message
    /// variants for each individual field.
    SetTransition(AnimElement, AnimDirection, TransitionConfig),
    /// Per-item chain stagger changed for an element.
    SetChainStagger(AnimElement, u32),
    /// Reset one element's animation to its built-in default.
    ResetElementAnimation(AnimElement),
    /// Reset everything to defaults.
    ResetAll,
    /// Debounced save tick — fires every 200 ms; if there's an
    /// unsaved edit older than 250 ms we flush to disk.
    SaveTick,
    /// Persist completed. Used to toast the user with a status.
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

/// Top-level model.
pub struct State {
    /// Currently-loaded config — the source of truth the UI edits.
    pub config: AppConfig,
    /// Active tab.
    pub tab: Tab,
    /// Path the config was loaded from / will be saved to. None
    /// when no XDG_CONFIG_HOME / HOME (extremely unlikely).
    pub config_path: Option<std::path::PathBuf>,
    /// Most recent edit timestamp; the SaveTick consults this so it
    /// only saves once the user has stopped fiddling for 250 ms.
    pub last_edit: Option<Instant>,
    /// True after a save flush; reset on the next edit.
    pub saved_pending: bool,
    /// Last save status — shown in the footer toast.
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
            tab: Tab::Animation,
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
        Some(Task::perform(
            persist::save(path, cfg),
            Message::Saved,
        ))
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

fn view(state: &State) -> Element<'_, Message> {
    let header = row![
        text("JuhRadial Settings").size(22),
        Space::new().width(Length::Fill),
        button("Reset all to defaults").on_press(Message::ResetAll),
    ]
    .align_y(iced::Alignment::Center)
    .padding(12)
    .spacing(12);

    let tab_row = row![
        tab_button("Visuals", state.tab == Tab::Visuals, Tab::Visuals),
        tab_button("Animation", state.tab == Tab::Animation, Tab::Animation),
    ]
    .spacing(4)
    .padding([0, 12]);

    let body: Element<Message> = match state.tab {
        Tab::Visuals => tabs::visuals::view(state),
        Tab::Animation => tabs::animation::view(state),
    };

    let footer_status: Element<Message> = if state.saved_pending {
        text("Editing… (autosaves shortly)").size(12).into()
    } else if !state.status.is_empty() {
        text(state.status.as_str()).size(12).into()
    } else {
        text("Idle.").size(12).into()
    };

    let footer = row![
        text(
            state
                .config_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(no config path)".into())
        )
        .size(11),
        Space::new().width(Length::Fill),
        footer_status,
    ]
    .padding(10)
    .spacing(12);

    container(
        column![
            header,
            tab_row,
            rule::horizontal(1),
            scrollable(container(body).padding(16)).height(Length::Fill),
            rule::horizontal(1),
            footer,
        ]
        .spacing(0),
    )
    .into()
}

fn tab_button(label: &str, active: bool, target: Tab) -> Element<'_, Message> {
    let mut b = button(text(label));
    if !active {
        b = b.style(button::secondary);
    }
    b.on_press(Message::SwitchTab(target)).into()
}

fn subscription(_state: &State) -> Subscription<Message> {
    // Tick every 200 ms — checks the debounce window in the
    // SaveTick handler. Cheap when there's nothing to save.
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
    window.size = iced::Size::new(880.0, 720.0);
    window.platform_specific.application_id = "org.juhlabs.juhradial.settings".into();

    iced::application(boot, update, view)
        .title("JuhRadial Settings")
        .window(window)
        .subscription(subscription)
        .run()
}

