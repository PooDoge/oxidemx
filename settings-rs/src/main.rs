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
    pub mod devices;
    pub mod haptics;
    pub mod placeholder;
    pub mod scroll;
    pub mod settings_page;
    pub mod visuals;
}
mod fonts;
mod mouse_callouts;
mod palette;
mod persist;
mod radial_preview;
mod raise;
mod singleton;
mod style;
mod widgets;

use iced::widget::{button, column, container, row, rule, scrollable, text, Space};
use iced::{Element, Length, Subscription, Task};
use juhradial_shared::{
    AnimationConfig, AppConfig, ElementAnimation, TransitionConfig, VisualSettings,
};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Receiver for D-Bus Focus events. Set once during main() after
/// the singleton handshake; `subscription()` reads it to wire the
/// focus stream into iced. Only `Some` when we're the primary
/// instance — secondary instances exit before iced starts.
static FOCUS_RX: OnceLock<async_channel::Receiver<()>> = OnceLock::new();

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
    /// Font family override for rendered text (Visuals tab).
    /// Empty string = system default.
    SetFontFamily(String),
    SetTransition(AnimElement, AnimDirection, TransitionConfig),
    SetChainStagger(AnimElement, u32),
    ResetElementAnimation(AnimElement),
    ResetAll,
    /// Easy-Switch shortcut toggle (Buttons tab right column).
    SetEasySwitchShortcuts(bool),
    /// Quit the settings window.
    Exit,
    /// Another `juhradial-settings` invocation called Focus on us
    /// via D-Bus; raise + focus the window.
    Focus,
    /// Debounced save tick — fires every 200 ms; if there's an
    /// unsaved edit older than 250 ms we flush to disk.
    SaveTick,
    /// Persist completed.
    Saved(Result<(), String>),
    /// Sentinel for fire-and-forget Tasks whose completion we don't
    /// need to react to (e.g. the RaiseOverlay D-Bus call from the
    /// Focus handler).
    Noop,
    /// Theme picker selection — re-resolves the palette and writes
    /// the theme name into the config (autosave will persist it).
    SetTheme(String),

    // --- Slices editor (Buttons tab right column) ---
    AddSlice,
    DeleteSlice(usize),
    MoveSliceUp(usize),
    MoveSliceDown(usize),
    SetSliceLabel(usize, String),
    SetSliceCommand(usize, String),
    SetSliceKind(usize, juhradial_shared::ActionKind),
    SetSliceColor(usize, String),
    /// Radial preview interactions.
    SelectSlice(usize),
    DismissSliceSelection,
    SwapSlices { from: usize, to: usize },

    // --- Haptics tab ---
    SetHapticsEnabled(bool),
    SetHapticsPerEvent(tabs::haptics::HapticsEvent, String),
    SetHapticsDefaultPattern(String),
    SetHapticsDebounce(u32),
    SetHapticsSliceDebounce(u32),
    SetHapticsReentryDebounce(u32),

    // --- Point & Scroll tab ---
    SetPointerSpeed(u32),
    SetPointerAcceleration(bool),
    SetScrollNatural(bool),
    SetScrollSmooth(bool),
    SetScrollSmartshift(bool),
    SetScrollSmartshiftThreshold(u32),
    SetScrollMode(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualField {
    MenuBackgroundOpacity,
    SliceHighlightOpacity,
    /// Centre-label font size in px (Visuals tab → "Centre label size").
    CenterLabelSize,
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
    /// Resolved colour palette derived from `config.theme`. Rebuilt
    /// on every theme change so all styled widgets re-skin live.
    pub palette: palette::Palette,
    pub tab: Tab,
    pub config_path: Option<std::path::PathBuf>,
    pub last_edit: Option<Instant>,
    pub saved_pending: bool,
    pub status: String,
    /// Currently-selected slot in the radial preview, if any.
    /// Drives the per-slice editor in the Buttons-tab right column.
    pub selected_slice: Option<usize>,
    /// Shared rasterised icon cache (XDG resolver + tinting). Lives
    /// at State level so it persists across re-renders and across
    /// theme changes (colours change → new cache entries; old
    /// entries stay for free).
    pub icons: std::rc::Rc<juhradial_icons::IconCache>,
    /// Iced-Handle cache layered on top — saves the GPU-upload step
    /// every render, keyed by (source, size, colour).
    pub iced_handles: std::rc::Rc<
        std::cell::RefCell<std::collections::HashMap<radial_preview::IconKey, iced::widget::image::Handle>>,
    >,
}

impl Default for State {
    fn default() -> Self {
        let path = juhradial_shared::config::default_config_path();
        let config = path
            .as_ref()
            .and_then(|p| AppConfig::load_from(p).ok())
            .unwrap_or_default();
        let pal = palette::Palette::resolve(&config.theme);
        State {
            config,
            palette: pal,
            tab: Tab::Buttons,
            config_path: path,
            last_edit: None,
            saved_pending: false,
            status: String::new(),
            selected_slice: None,
            icons: std::rc::Rc::new(juhradial_icons::IconCache::new()),
            iced_handles: std::rc::Rc::new(std::cell::RefCell::new(
                std::collections::HashMap::new(),
            )),
        }
    }
}

// `From<radial_preview::Action>` glue so the Canvas program can
// produce one of our top-level Messages without importing the
// whole enum tree.
impl From<radial_preview::Action> for Message {
    fn from(a: radial_preview::Action) -> Self {
        match a {
            radial_preview::Action::SelectSlice(i) => Message::SelectSlice(i),
            radial_preview::Action::DismissSelection => Message::DismissSliceSelection,
            radial_preview::Action::SwapSlices { from, to } => {
                Message::SwapSlices { from, to }
            }
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
            match field {
                VisualField::MenuBackgroundOpacity => {
                    state.config.radial_menu.visuals.menu_background_opacity =
                        v.clamp(0.0, 1.0);
                }
                VisualField::SliceHighlightOpacity => {
                    state.config.radial_menu.visuals.slice_highlight_opacity =
                        v.clamp(0.0, 1.0);
                }
                VisualField::CenterLabelSize => {
                    // 0 = disabled (the renderer skips drawing); cap
                    // at 32 so an accidental drag doesn't spawn
                    // ridiculous text.
                    state.config.radial_menu.visuals.center_label_size =
                        v.clamp(0.0, 32.0);
                }
            }
            state.touch();
            Task::none()
        }
        Message::SetFontFamily(s) => {
            state.config.radial_menu.visuals.font_family = s;
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
        Message::Focus => {
            // Wayland blocks app-side focus-steal, so do this in two
            // tracks at once:
            //   1. Fire-and-forget RaiseOverlay to the GNOME
            //      extension — it runs inside Mutter and can
            //      raise + activate the window even when the
            //      compositor would block us doing it ourselves.
            //   2. Locally tell iced to un-minimise + try to
            //      gain_focus. Cheap, handles minimisation, and
            //      acts as fallback when the extension is missing
            //      or hasn't been reloaded since the v4 update.
            Task::batch([
                Task::perform(raise::raise_settings_window(), |_| Message::Noop),
                iced::window::latest().and_then(|id| {
                    iced::window::set_mode(id, iced::window::Mode::Windowed)
                        .chain(iced::window::gain_focus(id))
                }),
            ])
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
        Message::Noop => Task::none(),
        Message::SetTheme(name) => {
            // Update both the persisted config and the in-memory
            // palette so the UI re-skins immediately. Auto-save
            // catches the config change.
            state.config.theme = juhradial_shared::theme::ThemeName::from(name.as_str());
            state.palette = palette::Palette::resolve(&state.config.theme);
            state.touch();
            Task::none()
        }
        // --- Slices editor handlers ---
        Message::AddSlice => {
            let slices = &mut state.config.radial_menu.slices;
            slices.push(juhradial_shared::Slice {
                action_id: None,
                label: "New slice".into(),
                kind: juhradial_shared::ActionKind::Exec,
                command: String::new(),
                color: "accent".into(),
                icon: String::new(),
                submenu: Vec::new(),
                visible_if: None,
            });
            state.touch();
            Task::none()
        }
        Message::DeleteSlice(i) => {
            let slices = &mut state.config.radial_menu.slices;
            if i < slices.len() {
                slices.remove(i);
                state.touch();
            }
            Task::none()
        }
        Message::MoveSliceUp(i) => {
            let slices = &mut state.config.radial_menu.slices;
            if i > 0 && i < slices.len() {
                slices.swap(i, i - 1);
                state.touch();
            }
            Task::none()
        }
        Message::MoveSliceDown(i) => {
            let slices = &mut state.config.radial_menu.slices;
            if i + 1 < slices.len() {
                slices.swap(i, i + 1);
                state.touch();
            }
            Task::none()
        }
        Message::SetSliceLabel(i, s) => {
            if let Some(slice) = state.config.radial_menu.slices.get_mut(i) {
                slice.label = s;
                state.touch();
            }
            Task::none()
        }
        Message::SetSliceCommand(i, s) => {
            if let Some(slice) = state.config.radial_menu.slices.get_mut(i) {
                slice.command = s;
                state.touch();
            }
            Task::none()
        }
        Message::SetSliceKind(i, k) => {
            if let Some(slice) = state.config.radial_menu.slices.get_mut(i) {
                slice.kind = k;
                state.touch();
            }
            Task::none()
        }
        Message::SetSliceColor(i, s) => {
            if let Some(slice) = state.config.radial_menu.slices.get_mut(i) {
                slice.color = s;
                state.touch();
            }
            Task::none()
        }

        // --- Radial preview interactions ---
        Message::SelectSlice(i) => {
            state.selected_slice = Some(i);
            Task::none()
        }
        Message::DismissSliceSelection => {
            state.selected_slice = None;
            Task::none()
        }
        Message::SwapSlices { from, to } => {
            let slices = &mut state.config.radial_menu.slices;
            // Pad to N_SLICES so the user can drop into an empty slot.
            while slices.len() < 8.max(from + 1).max(to + 1) {
                slices.push(juhradial_shared::Slice {
                    action_id: None,
                    label: "(empty)".into(),
                    kind: juhradial_shared::ActionKind::None,
                    command: String::new(),
                    color: "accent".into(),
                    icon: String::new(),
                    submenu: Vec::new(),
                    visible_if: None,
                });
            }
            if from < slices.len() && to < slices.len() {
                slices.swap(from, to);
                // Follow the moved slice — the user usually wants to
                // continue editing it.
                state.selected_slice = Some(to);
                state.touch();
            }
            Task::none()
        }

        // --- Haptics handlers ---
        Message::SetHapticsEnabled(on) => {
            state.config.haptics.enabled = on;
            state.touch();
            Task::none()
        }
        Message::SetHapticsPerEvent(evt, pattern) => {
            use tabs::haptics::HapticsEvent;
            let pe = &mut state.config.haptics.per_event;
            match evt {
                HapticsEvent::MenuAppear => pe.menu_appear = pattern,
                HapticsEvent::SliceChange => pe.slice_change = pattern,
                HapticsEvent::Confirm => pe.confirm = pattern,
                HapticsEvent::Invalid => pe.invalid = pattern,
            }
            state.touch();
            Task::none()
        }
        Message::SetHapticsDefaultPattern(p) => {
            state.config.haptics.default_pattern = p;
            state.touch();
            Task::none()
        }
        Message::SetHapticsDebounce(ms) => {
            state.config.haptics.debounce_ms = ms;
            state.touch();
            Task::none()
        }
        Message::SetHapticsSliceDebounce(ms) => {
            state.config.haptics.slice_debounce_ms = ms;
            state.touch();
            Task::none()
        }
        Message::SetHapticsReentryDebounce(ms) => {
            state.config.haptics.reentry_debounce_ms = ms;
            state.touch();
            Task::none()
        }

        // --- Point & Scroll handlers ---
        Message::SetPointerSpeed(v) => {
            state.config.pointer.speed = v;
            state.touch();
            Task::none()
        }
        Message::SetPointerAcceleration(on) => {
            state.config.pointer.acceleration = on;
            state.touch();
            Task::none()
        }
        Message::SetScrollNatural(on) => {
            state.config.scroll.natural = on;
            state.touch();
            Task::none()
        }
        Message::SetScrollSmooth(on) => {
            state.config.scroll.smooth = on;
            state.touch();
            Task::none()
        }
        Message::SetScrollSmartshift(on) => {
            state.config.scroll.smartshift = on;
            state.touch();
            Task::none()
        }
        Message::SetScrollSmartshiftThreshold(v) => {
            state.config.scroll.smartshift_threshold = v;
            state.touch();
            Task::none()
        }
        Message::SetScrollMode(s) => {
            state.config.scroll.mode = s;
            state.touch();
            Task::none()
        }
    }
}

// ============================================================================
// View — the shell
// ============================================================================

fn view(state: &State) -> Element<'_, Message> {
    let header = header_view(state);
    let sidebar = sidebar_view(state);
    let body: Element<Message> = match state.tab {
        Tab::Buttons => tabs::buttons::view(state),
        Tab::Settings => tabs::settings_page::view(state),
        Tab::PointScroll => tabs::scroll::view(state),
        Tab::Haptic => tabs::haptics::view(state),
        Tab::Devices => tabs::devices::view(state),
        Tab::EasySwitch => tabs::placeholder::view(state, 
            "Easy-Switch",
            "Configure each Easy-Switch host slot (label, OS icon). Coming soon.",
        ),
        Tab::Flow => tabs::placeholder::view(state, 
            "Flow",
            "Cross-machine cursor-and-clipboard hand-off. Coming soon.",
        ),
        Tab::Macros => tabs::placeholder::view(state, 
            "Macros",
            "Record and edit macros that slices can dispatch via the daemon. \
             Coming soon.",
        ),
        Tab::Gaming => tabs::placeholder::view(state, 
            "Gaming",
            "Per-game profiles + DPI overrides + low-latency mode. Coming soon.",
        ),
    };

    let main_area = row![
        sidebar,
        container(
            scrollable(container(body).padding(20))
                .style(style::scrollable_style(&state.palette))
        )
        .padding(0)
        .style(style::page(&state.palette))
        .width(Length::Fill)
        .height(Length::Fill),
    ]
    .height(Length::Fill);

    let footer = footer_view(state);

    container(
        column![
            header,
            rule::horizontal(1).style(style::rule_style(&state.palette)),
            main_area,
            rule::horizontal(1).style(style::rule_style(&state.palette)),
            footer,
        ]
        .spacing(0),
    )
    .style(style::window(&state.palette))
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// ----------------------------------------------------------------------------
// Header
// ----------------------------------------------------------------------------

fn header_view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    container(
        row![
            text("JuhRadial").size(20),
            text("MX").size(13).style(style::text_accent(pal)),
            text("MOUSE CONFIGURATION")
                .size(10)
                .style(style::text_faint(pal)),
            Space::new().width(Length::Fixed(16.0)),
            chip(state, "MX MASTER 4"),
            Space::new().width(Length::Fill),
            button(text("Exit").size(12))
                .style(style::btn_secondary(pal))
                .on_press(Message::Exit),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(10),
    )
    .style(style::header(pal))
    .padding(12)
    .into()
}

fn chip<'a>(state: &'a State, label: &str) -> Element<'a, Message> {
    container(text(label.to_string()).size(11))
        .padding([4, 10])
        .style(style::chip(&state.palette))
        .into()
}

// ----------------------------------------------------------------------------
// Sidebar
// ----------------------------------------------------------------------------

fn sidebar_view(state: &State) -> Element<'_, Message> {
    let mut col = column![].spacing(2).padding(12);
    for tab in Tab::ALL {
        col = col.push(sidebar_button(state, tab, state.tab == tab));
    }
    container(col)
        .width(Length::Fixed(220.0))
        .height(Length::Fill)
        .style(style::sidebar(&state.palette))
        .into()
}

fn sidebar_button<'a>(state: &'a State, tab: Tab, active: bool) -> Element<'a, Message> {
    let inner = row![
        text(tab.glyph()).size(13),
        text(tab.label()).size(13),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(12);

    button(inner)
        .width(Length::Fill)
        .padding([10, 14])
        .style(style::nav_item(&state.palette, active))
        .on_press(Message::SwitchTab(tab))
        .into()
}

// ----------------------------------------------------------------------------
// Footer
// ----------------------------------------------------------------------------

fn footer_view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let path = state
        .config_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no config path)".into());

    let status: Element<Message> = if state.saved_pending {
        text("Editing… (autosaves shortly)")
            .size(11)
            .style(style::text_dim(pal))
            .into()
    } else if !state.status.is_empty() {
        text(state.status.as_str())
            .size(11)
            .style(style::text_accent(pal))
            .into()
    } else {
        text("Idle.").size(11).style(style::text_faint(pal)).into()
    };

    container(
        row![
            text("JuhLabs · Free & open source software")
                .size(11)
                .style(style::text_dim(pal)),
            Space::new().width(Length::Fixed(16.0)),
            text(path).size(10).style(style::text_faint(pal)),
            Space::new().width(Length::Fill),
            status,
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8),
    )
    .style(style::footer(pal))
    .padding(10)
    .into()
}

// ============================================================================
// Subscription + main
// ============================================================================

fn subscription(_state: &State) -> Subscription<Message> {
    let mut subs = vec![iced::time::every(Duration::from_millis(200)).map(|_| Message::SaveTick)];
    if FOCUS_RX.get().is_some() {
        // The singleton handshake gave us a receiver — wire it in
        // so subsequent `juhradial-settings` invocations call
        // `Focus` on us and the running window pops to the front.
        // iced::Subscription::run takes a fn() pointer (no
        // captures), so the stream builder reads the receiver out
        // of the OnceLock.
        subs.push(Subscription::run(focus_subscription_builder).map(|_| Message::Focus));
    }
    Subscription::batch(subs)
}

fn focus_subscription_builder() -> impl futures_util::stream::Stream<Item = ()> {
    let rx = FOCUS_RX
        .get()
        .cloned()
        .expect("FOCUS_RX present; checked in subscription()");
    singleton::focus_stream(rx)
}

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Singleton enforcement BEFORE iced starts. If another
    // settings process is already running, this sends Focus to it
    // and exits cleanly — the user gets the existing window
    // raised instead of a duplicate.
    match singleton::try_acquire_or_focus_existing() {
        singleton::Acquisition::Primary(rx) => {
            FOCUS_RX
                .set(rx)
                .map_err(|_| ())
                .expect("FOCUS_RX set once");
        }
        singleton::Acquisition::SecondaryFocused => {
            info!("Existing settings instance focused; exiting.");
            return Ok(());
        }
        singleton::Acquisition::BusUnavailable => {
            warn!("Session bus unavailable; running without singleton enforcement.");
        }
    }

    let mut window = iced::window::Settings::default();
    window.size = iced::Size::new(1280.0, 820.0);
    window.platform_specific.application_id = "org.juhlabs.juhradial.settings".into();

    iced::application(boot, update, view)
        .title("JuhRadial Settings")
        .window(window)
        .theme(|state: &State| {
            // Anchor iced's built-in theme to our palette's dark/light
            // orientation so widgets we haven't custom-styled still
            // look right.
            if state.palette.is_dark {
                iced::Theme::Dark
            } else {
                iced::Theme::Light
            }
        })
        .subscription(subscription)
        .run()
}
