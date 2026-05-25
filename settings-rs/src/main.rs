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
    pub mod easyswitch;
    pub mod gaming;
    pub mod haptics;
    pub mod macros;
    pub mod placeholder;
    pub mod scroll;
    pub mod settings_page;
    pub mod visuals;
}
mod app_picker;
mod battery;
mod cursor_helper;
mod daemon;
mod fonts;
mod icon_picker;
mod mouse_callouts;
mod persist;
mod radial_preview;
mod raise;
mod recents;
mod singleton;

use iced::widget::{button, column, container, row, rule, scrollable, text, Space};
use iced::{Element, Length, Subscription, Task};
use juhradial_shared::{
    AnimationConfig, AppConfig, ElementAnimation, TransitionConfig, VisualSettings,
};
use juhradial_widgets::{palette, style};
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

    /// True when the tab's content is still a stub / placeholder
    /// (no real wiring to the daemon or device yet). Drives the
    /// `STUB` badge on the sidebar so users can tell at a glance
    /// which tabs do anything.
    pub fn is_stub(&self) -> bool {
        matches!(self, Tab::Flow)
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
    /// Set the icon name (freedesktop symbolic name, an absolute
    /// path to an SVG/PNG, or the legacy internal id) for a slice
    /// on the active page.
    SetSliceIcon(usize, String),
    /// Set the slice's description / tooltip text. Stored as
    /// metadata; not currently rendered in the overlay.
    SetSliceDescription(usize, String),
    /// Same for a submenu sub-item.
    SetSubItemDescription { parent: usize, idx: usize, value: String },
    /// Replace a slice's visibility predicate. `None` clears the
    /// predicate (slice is always visible). `Some(Always)` is
    /// equivalent at runtime; we write the slimmer `None` shape on
    /// disk for that case.
    SetSliceVisibility { slice: usize, condition: Option<juhradial_shared::Condition> },
    /// Spawn the slice's command via `sh -c` so the user can
    /// validate shell quoting + that the command actually launches
    /// before relying on the radial menu to dispatch it. Non-Exec
    /// slice kinds (Macro, EasySwitch, …) emit a status hint
    /// instead — those round-trip through the daemon and aren't
    /// useful to test in isolation.
    TestSliceAction(usize),
    /// Same as TestSliceAction but for a submenu sub-item.
    TestSubItemAction { parent: usize, idx: usize },
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

    // --- Mouse-button assignments (Buttons tab) ---
    SetButtonAssignment(juhradial_shared::MouseButton, juhradial_shared::ButtonAction),

    // --- Battery (UPower poll) ---
    /// Periodic tick — kicks off a UPower probe.
    BatteryTick,
    /// Probe finished; latest reading.
    BatteryUpdate(Option<battery::BatteryStatus>),

    // --- Macros tab ---
    RefreshMacros,
    OpenMacrosFolder,
    DeleteMacro(String),

    // --- Daemon snapshot (battery + name + DPI + Easy-Switch) ---
    DaemonTick,
    DaemonSnapshotReceived(daemon::DaemonSnapshot),

    // --- DPI (Point & Scroll tab) ---
    /// User dragged the DPI slider. Fired on release; the change-
    /// gating is in the daemon's update path.
    SetDpi(u16),
    /// Result of the SetDpi async call.
    DpiSet(Result<(), String>),

    // --- Easy-Switch tab ---
    SwitchHost(u8),
    HostSwitched(Result<(), String>),

    // --- Macros: in-place edit ---
    /// Open inline edit form for an existing macro.
    StartEditMacro(String),
    /// User typed in the rename or trigger field.
    EditMacroField { id: String, field: MacroEditField, value: String },
    /// Persist changes to disk.
    CommitMacroEdit(String),
    /// Cancel without writing.
    CancelMacroEdit,

    // --- Macros: recording ---
    /// User clicked Record (or Stop, depending on `recording_state`).
    ToggleMacroRecord,
    /// Daemon ack'd start.
    MacroRecordStarted(Result<(), String>),
    /// Daemon returned the captured events JSON.
    MacroRecordStopped(Result<String, String>),
    /// User typed in the post-record name field.
    EditRecordedName(String),
    /// User clicked Save in the post-record form.
    SaveRecordedMacro,
    /// User clicked Discard in the post-record form.
    DiscardRecordedMacro,
    MacroSaved(Result<(), String>),

    // --- Gaming ---
    SetGamingMode(bool),
    GamingModeSet(Result<(), String>),
    CycleGamingDpi,
    GamingDpiCycled(Result<String, String>),

    // --- HiResScroll (Point & Scroll tab) ---
    SetHiResScrollHires(bool),
    SetHiResScrollInvert(bool),
    SetHiResScrollTarget(bool),
    HiResScrollSet(Result<(), String>),

    // --- Per-app profile bindings (Settings tab) ---
    /// User typed in the WIP "app class" or "profile name" fields.
    SetAppBindingDraft { class: String, profile: String },
    /// Save the current draft as a new app→profile entry.
    AddAppBinding,
    /// Remove the binding for `class`.
    RemoveAppBinding(String),

    // --- Custom theme palette editor (Settings tab → Theme card) ---
    /// Toggle the "Customise theme" expander.
    ToggleThemeCustomiser,
    /// Edit one palette field. Field name is one of the
    /// ThemeColors keys ("crust", "accent", etc.); value is the
    /// new "#rrggbb" hex.
    SetThemeColor { field: String, value: String },
    /// Save the active palette as a user theme with the typed slug.
    SaveCustomTheme,
    /// Slug being typed into the "Save as" input.
    SetCustomThemeName(String),
    /// Custom theme save completed.
    CustomThemeSaved(Result<String, String>),

    // --- Multi-page menu (Buttons tab right column) ---
    /// User picked a different page in the page picker. The slice
    /// editor + radial preview both pin to this index.
    SetActivePage(usize),
    /// Append a new empty global page; auto-selects it.
    AddPage,
    /// Delete a page by index. Won't go below one page (pages list
    /// always retains at least one entry).
    DeletePage(usize),
    /// Rename the page at `idx`.
    SetPageName { page: usize, name: String },
    /// Update the comma-separated app-classes list for `page`.
    /// Empty string clears the list (turns the page into a global
    /// page); non-empty makes it an app-context page.
    SetPageAppClasses { page: usize, value: String },
    /// Toggle whether an app-context page also participates in the
    /// scroll-wheel cycle.
    SetPageIncludeInScroll { page: usize, value: bool },
    /// Move page left in the order (lower index → earlier in cycle).
    MovePageLeft(usize),
    /// Move page right in the order.
    MovePageRight(usize),
    /// Clone the page at `idx` and append the copy to the end of
    /// `pages`. The new page inherits the source's slices, app
    /// classes (cleared so the original keeps its auto-select
    /// claim), and include_in_scroll flag. Auto-selects the
    /// duplicate so the user can immediately edit it.
    DuplicatePage(usize),
    /// Kick off a GNOME-extension call to capture the currently
    /// focused window's class. Used by the "Detect from focused
    /// window" button on the page editor — saves the user from
    /// having to look up the WM_CLASS by hand.
    DetectFocusedClass(usize),
    /// Result of `DetectFocusedClass`. When `Some`, append it to
    /// the target page's app_classes; when `None`, surface a hint
    /// in the status bar. `generation` filters stale results from
    /// a previously-cancelled run.
    DetectedFocusedClass { page: usize, generation: u64, class: Option<String> },
    /// Cancel an in-flight detect (the user changed their mind
    /// before the 4-second sample fired).
    CancelFocusedClassDetect,

    // --- Visual icon picker (slice + sub-item editor) ---
    /// Open the icon picker against a slice or sub-item. Replaces
    /// any currently-open picker.
    OpenIconPicker(icon_picker::IconPickerTarget),
    /// Close the picker without applying an icon.
    CloseIconPicker,
    /// Live filter — case-insensitive substring match. Filters
    /// against icon name (Catalogue source) or app display name
    /// (Apps source) depending on the current source.
    SetIconPickerSearch(String),
    /// Switch picker source between the curated symbolic catalogue
    /// and installed-applications. Resets the search filter so a
    /// query that matched in one mode doesn't carry into the
    /// other (where it would show zero results unhelpfully).
    SetIconPickerSource(icon_picker::IconSource),
    /// User clicked an icon thumbnail — apply it to the picker's
    /// stored target and close.
    PickIcon(String),

    // --- App-for-command picker (slice + sub-item editor) ---
    /// Open the app picker against a slice or sub-item; fills
    /// command + icon + label (if empty) + Full colour mode in
    /// one click.
    OpenAppCommandPicker(app_picker::AppCommandTarget),
    CloseAppCommandPicker,
    SetAppCommandSearch(String),
    /// Toggle whether the app pick replaces the slice's icon
    /// (defaults to true). Off = command + label only, leave the
    /// existing icon + colour mode alone.
    SetAppCommandReplaceIcon(bool),
    /// User clicked an app in the list. Carries the cleaned exec
    /// line (placeholder %F/%U/etc. stripped), icon string, and
    /// display name so the handler doesn't need to look back into
    /// the apps cache.
    PickAppForCommand { command: String, icon: String, label: String },
    /// Async write of the recents list completed; result is the
    /// updated list (most-recent first). Used to update the
    /// in-memory `recent_icons` so the picker re-renders with
    /// the new ordering on the next message.
    RecentIconsPersisted(Vec<String>),
    /// Open a native file dialog to pick an icon from disk
    /// (PNG/SVG). The chosen path's absolute string lands in the
    /// target's icon field. Target identifies which slice or
    /// sub-item to apply to.
    BrowseIconFile(icon_picker::IconPickerTarget),
    /// Result of the file dialog. `Some(path)` = user picked a
    /// file, `None` = cancelled.
    IconFileChosen { target: icon_picker::IconPickerTarget, path: Option<String> },
    /// Bulk-rasterise the icon-picker catalogue off the UI thread.
    /// Carries the tint colour (so the worker thread can tint
    /// without referencing palette state) and reports back via
    /// `IconsPrewarmed` once everything is rasterised. Single
    /// message → single re-render of the settings UI when results
    /// arrive (vs. the older trickle approach which fired N
    /// messages and triggered N re-renders).
    PrewarmIcons,
    /// Result of the off-thread prewarm. The Vec is the full
    /// catalogue with one entry per icon — `Some(RasterIcon)` when
    /// the icon resolved, `None` when it didn't (theme miss). The
    /// handler installs every Some into the iced_handles cache,
    /// then the next render finds them all in one go.
    IconsPrewarmed(Vec<(String, Option<juhradial_icons::RasterIcon>)>),
    /// Result of the untinted (apps-source) prewarm pass — same
    /// shape as `IconsPrewarmed` but the install path uses the
    /// `peek/install_icon_handle_untinted` cache key (color = 0)
    /// so app icons retain their brand colours in the picker.
    AppIconsPrewarmed(Vec<(String, Option<juhradial_icons::RasterIcon>)>),

    // --- Submenu sub-items (slice editor) ---
    AddSubItem(usize),
    DeleteSubItem(usize, usize),
    SetSubItemLabel(usize, usize, String),
    SetSubItemCommand(usize, usize, String),
    SetSubItemColor(usize, usize, String),
    /// Icon name / path for a submenu sub-item.
    SetSubItemIcon(usize, usize, String),
    /// Action kind (Exec / Macro / EasySwitch / etc.) for a sub-item.
    SetSubItemKind(usize, usize, juhradial_shared::ActionKind),
    MoveSubItemUp(usize, usize),
    MoveSubItemDown(usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualField {
    MenuBackgroundOpacity,
    SliceHighlightOpacity,
    /// Centre-label font size in px (Visuals tab → "Centre label size").
    CenterLabelSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroEditField {
    Name,
    Description,
    Trigger,
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
    /// Latest UPower battery reading. None until the first poll
    /// completes (or if UPower / a Logitech mouse aren't around).
    pub battery: Option<battery::BatteryStatus>,
    /// Cached list of macros in `~/.config/juhradial/macros/`.
    /// Refreshed on tab switch + user-triggered Refresh.
    pub macros: Vec<tabs::macros::MacroSummary>,
    /// Latest snapshot from the daemon — battery, device name,
    /// DPI, Easy-Switch state. Refreshed on a 5s timer + on tab
    /// entry where it matters.
    pub daemon: daemon::DaemonSnapshot,
    /// Macro recording flow state. Three values: idle / recording
    /// / naming-after-stop. Drives the Macros-tab Record button
    /// + the post-record name form.
    pub recording: RecordingState,
    /// Custom-theme editor state. None = collapsed; Some =
    /// expanded with the WIP palette + the slug typed by the user.
    pub theme_editor: Option<ThemeEditor>,
    /// In-place macro editor — when populated, the matching row in
    /// the Macros list shows an inline form instead of the
    /// read-only summary.
    pub macro_edit: Option<MacroEditDraft>,
    /// In-flight "Add application binding" form. Lives at State
    /// level so the typed text survives re-renders.
    pub app_binding_draft: AppBindingDraft,
    /// Currently-edited radial page. The slice editor + radial
    /// preview both work against this index. Persists across edits
    /// so navigating away and back keeps the same page open.
    pub active_page: usize,
    /// Per-page raw text for the "App classes" input — keeps the
    /// user's literal typing (incl. trailing commas / spaces)
    /// across re-renders. Without this the textbox would erase
    /// trailing commas as the user types because the canonical
    /// `Vec<String>` filters empties.
    pub app_classes_drafts: std::collections::BTreeMap<usize, String>,
    /// In-flight focused-class detection: which page asked + when
    /// the sample fires. The view uses this to render a countdown
    /// + Cancel button while waiting; the deferred Task::perform
    /// races independently. `None` = no detect in flight.
    pub detect_in_flight: Option<DetectInFlight>,
    /// Visual icon-picker dialog state. `Some` while the picker
    /// is open against a slice or sub-item; `None` when closed.
    /// Picker `target` is set when opening and consumed when an
    /// icon is clicked.
    pub icon_picker: Option<icon_picker::IconPickerState>,
    /// Most-recently-used icon names. Front of the list is the
    /// last-picked icon. Persisted to
    /// `~/.config/juhradial/recent-icons.json` after each pick;
    /// surfaces as a row at the top of the icon picker so common
    /// choices are one click away.
    pub recent_icons: Vec<String>,
    /// Snapshot of installed `.desktop` apps + their declared
    /// `Icon=` fields. Used by the picker's "Apps" source so the
    /// user can pick an icon by app rather than by symbolic name.
    /// Loaded once at startup; static enough to skip live
    /// reloading.
    pub installed_apps: Vec<juhradial_shared::DesktopEntry>,
    /// In-flight app picker for the slice command field. `Some`
    /// while the inline list is open; `None` when closed. Picker
    /// fills the slice's command + icon + label-if-empty + flips
    /// to "Full colour" mode in one click.
    pub app_command_picker: Option<app_picker::AppCommandPickerState>,
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
        let mut config = path
            .as_ref()
            .and_then(|p| AppConfig::load_from(p).ok())
            .unwrap_or_default();
        // `unwrap_or_default()` skips the loader's normalize step,
        // so make sure the multi-page invariant holds (>=1 page)
        // before any slice-editor message can mutate state.
        config.radial_menu.normalize_pages();
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
            battery: None,
            macros: tabs::macros::list(),
            daemon: daemon::DaemonSnapshot::default(),
            recording: RecordingState::Idle,
            theme_editor: None,
            macro_edit: None,
            app_binding_draft: AppBindingDraft::default(),
            active_page: 0,
            app_classes_drafts: std::collections::BTreeMap::new(),
            detect_in_flight: None,
            icon_picker: None,
            recent_icons: recents::load(),
            installed_apps: juhradial_shared::enumerate_applications(),
            app_command_picker: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppBindingDraft {
    pub class: String,
    pub profile: String,
}

/// Active focused-class detection state.
#[derive(Debug, Clone, Copy)]
pub struct DetectInFlight {
    /// Page index the detect was triggered for.
    pub page: usize,
    /// Wall-clock when the deferred sample will fire. The view
    /// uses this to render the remaining seconds; the actual
    /// sampling happens in a Task::perform that races us.
    pub deadline: Instant,
    /// Generation counter so cancel + restart don't deliver a
    /// stale `DetectedFocusedClass` from a prior run.
    pub generation: u64,
}

#[derive(Debug, Clone)]
pub struct MacroEditDraft {
    pub id: String,
    pub name: String,
    pub description: String,
    pub trigger: String,
}

/// In-flight state of the custom-theme editor.
#[derive(Debug, Clone)]
pub struct ThemeEditor {
    /// Working palette — starts as a clone of the active theme's
    /// colours and accumulates the user's edits. Saved on click.
    pub working: juhradial_shared::theme::ThemeColors,
    /// is_dark flag for the working theme. Mirrors the theme this
    /// was forked from.
    pub is_dark: bool,
    /// Slug typed into the "Save as" input. Becomes both the
    /// filename and the picker entry on save.
    pub slug: String,
}

/// Apply a `#rrggbb` (or any string the user typed) to the named
/// field on a `ThemeColors`. Unknown field name → no-op. Used by
/// the custom-palette editor to thread one Message back into the
/// working struct.
fn set_theme_color_field(c: &mut juhradial_shared::theme::ThemeColors, field: &str, value: String) {
    match field {
        "crust" => c.crust = value,
        "mantle" => c.mantle = value,
        "base" => c.base = value,
        "surface0" => c.surface0 = value,
        "surface1" => c.surface1 = value,
        "surface2" => c.surface2 = value,
        "overlay0" => c.overlay0 = value,
        "overlay1" => c.overlay1 = value,
        "text" => c.text = value,
        "subtext1" => c.subtext1 = value,
        "subtext0" => c.subtext0 = value,
        "accent" => c.accent = value,
        "accent2" => c.accent2 = value,
        "accent_dim" => c.accent_dim = value,
        "green" => c.green = value,
        "yellow" => c.yellow = value,
        "red" => c.red = value,
        "blue" => c.blue = value,
        "mauve" => c.mauve = value,
        "pink" => c.pink = value,
        "peach" => c.peach = value,
        "teal" => c.teal = value,
        "sapphire" => c.sapphire = value,
        "lavender" => c.lavender = value,
        _ => {}
    }
}

/// Spawn a slice's Exec command via `sh -c` so the user can
/// validate it from the editor without going through the radial
/// menu. Mirrors the daemon's exec convention (see
/// `daemon/src/actions.rs::execute_command`) so what tests here
/// is what the daemon will run later. Status messages surface
/// failures; the spawn itself is non-blocking.
fn run_test_action(state: &mut State, slice: Option<&juhradial_shared::Slice>) {
    let slice = match slice {
        Some(s) => s,
        None => {
            state.status = "Couldn't find slice to test.".into();
            return;
        }
    };
    if !matches!(slice.kind, juhradial_shared::ActionKind::Exec) {
        state.status = format!(
            "Test only supports Exec actions; this slice is {:?}. \
             Trigger via the radial menu to test other kinds.",
            slice.kind
        );
        return;
    }
    let cmd = slice.command.trim();
    if cmd.is_empty() {
        state.status = "Cannot test: command is empty.".into();
        return;
    }
    match std::process::Command::new("sh")
        .args(["-c", cmd])
        .spawn()
    {
        Ok(child) => {
            state.status = format!("Spawned (PID {}): {}", child.id(), cmd);
        }
        Err(e) => {
            state.status = format!("Failed to spawn: {e}");
        }
    }
}

/// Apply an icon name (or absolute file path) to whichever picker
/// target the caller specifies. `prefer_untinted` is set when the
/// source is naturally full-colour (Apps grid, file picker) — the
/// slice's `icon_untinted` flag flips on so the colour pick_list
/// shows "Full colour" and the radial menu skips the alpha-mask
/// tint at render time.
fn apply_icon_to_target(
    state: &mut State,
    target: Option<icon_picker::IconPickerTarget>,
    name: String,
    prefer_untinted: bool,
) {
    let target = match target {
        Some(t) => t,
        None => return,
    };
    match target {
        icon_picker::IconPickerTarget::Slice(idx) => {
            if let Some(slice) = state.active_slices_mut().get_mut(idx) {
                slice.icon = name;
                if prefer_untinted {
                    slice.icon_untinted = true;
                }
                state.touch();
            }
        }
        icon_picker::IconPickerTarget::SubItem { parent, idx } => {
            if let Some(item) = state
                .active_slices_mut()
                .get_mut(parent)
                .and_then(|p| p.submenu.get_mut(idx))
            {
                item.icon = name;
                if prefer_untinted {
                    item.icon_untinted = true;
                }
                state.touch();
            }
        }
    }
}

/// Lower-case + replace anything non-alphanumeric with `-`. Used
/// for both the custom-theme save filename and the macro id.
fn sanitize_slug(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Optimistically update the HiResScroll snapshot field + fire the
/// async D-Bus write. The 5s daemon poll will reconcile if the
/// device reports something different.
fn apply_hiresscroll_field<F>(state: &mut State, mutate: F) -> Task<Message>
where
    F: FnOnce(&mut daemon::HiResScroll),
{
    let mut current = state.daemon.hiresscroll.unwrap_or_default();
    mutate(&mut current);
    state.daemon.hiresscroll = Some(current);
    Task::perform(
        daemon::set_hiresscroll(current.hires, current.invert, current.target),
        Message::HiResScrollSet,
    )
}

/// Three-state model for the Macros-tab record flow.
#[derive(Debug, Clone, Default)]
pub enum RecordingState {
    /// Outside the recording flow — Record button is enabled.
    #[default]
    Idle,
    /// Daemon's recorder is buffering events; Record becomes Stop.
    Recording,
    /// Recording stopped — `events_json` is the daemon's
    /// `{events, actions}` payload. The UI swaps in a name field
    /// + Save / Discard buttons until the user picks one.
    Naming { events_json: String, name: String },
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

    /// Mutable access to the slice list of the currently-active
    /// page. Guarantees `pages` is non-empty + clamps
    /// `active_page` to a valid index — defensive against state
    /// arriving from a partially-migrated config.
    fn active_slices_mut(&mut self) -> &mut Vec<juhradial_shared::Slice> {
        if self.config.radial_menu.pages.is_empty() {
            self.config
                .radial_menu
                .pages
                .push(juhradial_shared::RadialPage::default());
        }
        let max = self.config.radial_menu.pages.len() - 1;
        if self.active_page > max {
            self.active_page = max;
        }
        &mut self.config.radial_menu.pages[self.active_page].slices
    }

    /// Read-only counterpart for view code. Returns an empty slice
    /// rather than panicking when the index is stale.
    fn active_slices(&self) -> &[juhradial_shared::Slice] {
        self.config
            .radial_menu
            .pages
            .get(self.active_page)
            .map(|p| p.slices.as_slice())
            .unwrap_or(&[])
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

fn boot() -> (State, Task<Message>) {
    // Kick off both probes immediately so the indicators aren't
    // blank for the full poll interval after launch.
    (
        State::default(),
        Task::batch([
            Task::perform(battery::poll(), Message::BatteryUpdate),
            Task::perform(daemon::poll(), Message::DaemonSnapshotReceived),
        ]),
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::SwitchTab(t) => {
            state.tab = t;
            // Refresh tab-specific caches on entry. Cheap when the
            // tab doesn't need it.
            if t == Tab::Macros {
                state.macros = tabs::macros::list();
            }
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
        // --- Slices editor handlers (operate on the active page) ---
        Message::AddSlice => {
            let slices = state.active_slices_mut();
            slices.push(juhradial_shared::Slice {
                action_id: None,
                label: "New slice".into(),
                kind: juhradial_shared::ActionKind::Exec,
                command: String::new(),
                color: "accent".into(),
                icon: String::new(),
                submenu: Vec::new(),
                visible_if: None,
                icon_untinted: false,
                description: String::new(),
            });
            state.touch();
            Task::none()
        }
        Message::DeleteSlice(i) => {
            let slices = state.active_slices_mut();
            if i < slices.len() {
                slices.remove(i);
                state.touch();
            }
            Task::none()
        }
        Message::MoveSliceUp(i) => {
            let slices = state.active_slices_mut();
            if i > 0 && i < slices.len() {
                slices.swap(i, i - 1);
                state.touch();
            }
            Task::none()
        }
        Message::MoveSliceDown(i) => {
            let slices = state.active_slices_mut();
            if i + 1 < slices.len() {
                slices.swap(i, i + 1);
                state.touch();
            }
            Task::none()
        }
        Message::SetSliceLabel(i, s) => {
            if let Some(slice) = state.active_slices_mut().get_mut(i) {
                slice.label = s;
                state.touch();
            }
            Task::none()
        }
        Message::SetSliceCommand(i, s) => {
            if let Some(slice) = state.active_slices_mut().get_mut(i) {
                slice.command = s;
                state.touch();
            }
            Task::none()
        }
        Message::SetSliceKind(i, k) => {
            if let Some(slice) = state.active_slices_mut().get_mut(i) {
                slice.kind = k;
                state.touch();
            }
            Task::none()
        }
        Message::SetSliceColor(i, s) => {
            if let Some(slice) = state.active_slices_mut().get_mut(i) {
                if s == tabs::buttons::FULL_COLOR_KEY {
                    // "Full colour" choice → flip to original icon
                    // colours, leave the underlying tint colour
                    // unchanged so toggling back to a tint later
                    // restores the previous selection.
                    slice.icon_untinted = true;
                } else {
                    slice.color = s;
                    slice.icon_untinted = false;
                }
                state.touch();
            }
            Task::none()
        }
        Message::SetSliceIcon(i, s) => {
            if let Some(slice) = state.active_slices_mut().get_mut(i) {
                slice.icon = s;
                state.touch();
            }
            Task::none()
        }
        Message::SetSliceDescription(i, s) => {
            if let Some(slice) = state.active_slices_mut().get_mut(i) {
                slice.description = s;
                state.touch();
            }
            Task::none()
        }
        Message::SetSubItemDescription { parent, idx, value } => {
            if let Some(item) = state
                .active_slices_mut()
                .get_mut(parent)
                .and_then(|p| p.submenu.get_mut(idx))
            {
                item.description = value;
                state.touch();
            }
            Task::none()
        }
        Message::TestSliceAction(idx) => {
            let slice = state.active_slices().get(idx).cloned();
            run_test_action(state, slice.as_ref());
            Task::none()
        }
        Message::TestSubItemAction { parent, idx } => {
            let item = state
                .active_slices()
                .get(parent)
                .and_then(|p| p.submenu.get(idx))
                .cloned();
            run_test_action(state, item.as_ref());
            Task::none()
        }
        Message::SetSliceVisibility { slice, condition } => {
            if let Some(s) = state.active_slices_mut().get_mut(slice) {
                // Collapse `Always` to None so the on-disk shape is
                // minimal (Some(Always) and None evaluate the same).
                s.visible_if = match condition {
                    Some(juhradial_shared::Condition::Always) | None => None,
                    other => other,
                };
                state.touch();
            }
            Task::none()
        }

        // --- Radial preview interactions ---
        Message::SelectSlice(i) => {
            // Close any picker bound to a different slot —
            // leaving it open would render the panel under the
            // editor of a slot the user just navigated away from.
            if let Some(picker) = state.icon_picker.as_ref() {
                let still_relevant = matches!(
                    picker.target,
                    icon_picker::IconPickerTarget::Slice(t) if t == i
                );
                if !still_relevant {
                    state.icon_picker = None;
                }
            }
            if let Some(picker) = state.app_command_picker.as_ref() {
                let still_relevant = matches!(
                    picker.target,
                    app_picker::AppCommandTarget::Slice(t) if t == i
                );
                if !still_relevant {
                    state.app_command_picker = None;
                }
            }
            state.selected_slice = Some(i);
            Task::none()
        }
        Message::DismissSliceSelection => {
            state.selected_slice = None;
            // Close anything bound to the deselected slot.
            state.icon_picker = None;
            state.app_command_picker = None;
            Task::none()
        }
        Message::SwapSlices { from, to } => {
            let slices = state.active_slices_mut();
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
                icon_untinted: false,
                description: String::new(),
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
                HapticsEvent::PageChange => pe.page_change = pattern,
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
        Message::SetButtonAssignment(button, action) => {
            button.set(&mut state.config.buttons, action);
            state.touch();
            Task::none()
        }

        // --- Battery ---
        Message::BatteryTick => Task::perform(battery::poll(), Message::BatteryUpdate),
        Message::BatteryUpdate(s) => {
            state.battery = s;
            Task::none()
        }

        // --- Macros ---
        Message::RefreshMacros => {
            state.macros = tabs::macros::list();
            Task::none()
        }
        Message::OpenMacrosFolder => {
            if let Some(dir) = tabs::macros::macros_dir() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("xdg-open")
                    .arg(&dir)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
            Task::none()
        }
        Message::DeleteMacro(id) => {
            match tabs::macros::delete_macro(&id) {
                Ok(_) => {
                    state.status = format!("Deleted macro {id}");
                    state.macros = tabs::macros::list();
                }
                Err(e) => {
                    state.status = format!("Delete failed: {e}");
                }
            }
            Task::none()
        }

        // --- Daemon snapshot ---
        Message::DaemonTick => Task::perform(daemon::poll(), Message::DaemonSnapshotReceived),
        Message::DaemonSnapshotReceived(snap) => {
            // Prefer the daemon's battery reading over UPower when
            // it's present — it's instant rather than UPower's
            // ~30s lag. Fall back to UPower if daemon battery is
            // missing.
            if let Some((p, c)) = snap.battery {
                state.battery = Some(battery::BatteryStatus {
                    percent: p,
                    charging: c,
                });
            }
            state.daemon = snap;
            Task::none()
        }

        // --- DPI ---
        Message::SetDpi(dpi) => {
            // Optimistic update so the slider doesn't snap back.
            state.daemon.dpi = Some(dpi);
            Task::perform(daemon::set_dpi(dpi), Message::DpiSet)
        }
        Message::DpiSet(Ok(_)) => {
            state.status = format!("DPI updated");
            Task::none()
        }
        Message::DpiSet(Err(e)) => {
            state.status = format!("DPI set failed: {e}");
            Task::none()
        }

        // --- Easy-Switch ---
        Message::SwitchHost(idx) => {
            if let Some(es) = state.daemon.easy_switch.as_mut() {
                es.current_host = idx;
            }
            Task::perform(daemon::set_host(idx), Message::HostSwitched)
        }
        Message::HostSwitched(Ok(_)) => {
            state.status = format!("Host switched");
            // Re-poll so we get the device's actual confirmed slot.
            Task::perform(daemon::poll(), Message::DaemonSnapshotReceived)
        }
        Message::HostSwitched(Err(e)) => {
            state.status = format!("Host switch failed: {e}");
            Task::none()
        }

        // --- Macros: in-place edit ---
        Message::StartEditMacro(id) => {
            // Pull current values from the on-disk JSON so the
            // form fields seed correctly.
            match tabs::macros::read_raw(&id) {
                Ok(v) => {
                    state.macro_edit = Some(MacroEditDraft {
                        id: id.clone(),
                        name: v
                            .get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        description: v
                            .get("description")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        trigger: v
                            .get("assigned_trigger")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    });
                }
                Err(e) => {
                    state.status = format!("Read failed: {e}");
                }
            }
            Task::none()
        }
        Message::EditMacroField { id, field, value } => {
            if let Some(draft) = state.macro_edit.as_mut() {
                if draft.id == id {
                    match field {
                        MacroEditField::Name => draft.name = value,
                        MacroEditField::Description => draft.description = value,
                        MacroEditField::Trigger => draft.trigger = value,
                    }
                }
            }
            Task::none()
        }
        Message::CommitMacroEdit(id) => {
            if let Some(draft) = state.macro_edit.take() {
                if draft.id == id {
                    if let Ok(mut v) = tabs::macros::read_raw(&id) {
                        if let Some(obj) = v.as_object_mut() {
                            obj.insert(
                                "name".into(),
                                serde_json::Value::String(draft.name.trim().to_string()),
                            );
                            obj.insert(
                                "description".into(),
                                serde_json::Value::String(draft.description.trim().to_string()),
                            );
                            obj.insert(
                                "assigned_trigger".into(),
                                if draft.trigger.trim().is_empty() {
                                    serde_json::Value::Null
                                } else {
                                    serde_json::Value::String(draft.trigger.trim().to_string())
                                },
                            );
                        }
                        match tabs::macros::write_raw(&id, &v) {
                            Ok(_) => {
                                state.status = format!("Saved \"{}\"", draft.name);
                                state.macros = tabs::macros::list();
                            }
                            Err(e) => {
                                state.status = format!("Save failed: {e}");
                            }
                        }
                    }
                }
            }
            Task::none()
        }
        Message::CancelMacroEdit => {
            state.macro_edit = None;
            Task::none()
        }

        // --- Macros: recording flow ---
        Message::ToggleMacroRecord => match &state.recording {
            RecordingState::Idle => {
                state.status = "Starting macro recording…".into();
                Task::perform(daemon::start_macro_recording(), Message::MacroRecordStarted)
            }
            RecordingState::Recording => {
                state.status = "Stopping recording…".into();
                Task::perform(daemon::stop_macro_recording(), Message::MacroRecordStopped)
            }
            RecordingState::Naming { .. } => Task::none(),
        },
        Message::MacroRecordStarted(Ok(_)) => {
            state.recording = RecordingState::Recording;
            state.status = "Recording — press buttons / keys, then click Stop".into();
            Task::none()
        }
        Message::MacroRecordStarted(Err(e)) => {
            state.status = format!("Record failed: {e}");
            Task::none()
        }
        Message::MacroRecordStopped(Ok(events_json)) => {
            state.recording = RecordingState::Naming {
                events_json,
                name: String::new(),
            };
            state.status = "Recording captured — name it and Save".into();
            Task::none()
        }
        Message::MacroRecordStopped(Err(e)) => {
            state.recording = RecordingState::Idle;
            state.status = format!("Stop failed: {e}");
            Task::none()
        }
        Message::EditRecordedName(s) => {
            if let RecordingState::Naming { name, .. } = &mut state.recording {
                *name = s;
            }
            Task::none()
        }
        Message::SaveRecordedMacro => {
            // Pull the events JSON + name from state, build a
            // MacroConfig, ship it through the daemon's SaveMacro.
            if let RecordingState::Naming { events_json, name } = &state.recording {
                if name.trim().is_empty() {
                    state.status = "Macro needs a name".into();
                    return Task::none();
                }
                let id = name
                    .trim()
                    .to_lowercase()
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '-' })
                    .collect::<String>();
                // The daemon's stop_macro_recording returns
                // `{events, actions}`; we wrap into a MacroConfig.
                // Pull `actions` out and embed.
                let parsed: serde_json::Value =
                    serde_json::from_str(events_json).unwrap_or_default();
                let actions = parsed
                    .get("actions")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([]));
                let cfg = serde_json::json!({
                    "id": id,
                    "name": name.trim(),
                    "description": "",
                    "repeat_mode": "once",
                    "repeat_count": 1,
                    "actions": actions,
                    "standard_delay_ms": 50,
                    "use_standard_delay": false,
                    "assigned_trigger": null,
                });
                let json = cfg.to_string();
                state.recording = RecordingState::Idle;
                Task::perform(daemon::save_macro(json), Message::MacroSaved)
            } else {
                Task::none()
            }
        }
        Message::DiscardRecordedMacro => {
            state.recording = RecordingState::Idle;
            state.status = "Recording discarded".into();
            Task::none()
        }
        Message::MacroSaved(Ok(_)) => {
            state.status = "Macro saved".into();
            state.macros = tabs::macros::list();
            Task::none()
        }
        Message::MacroSaved(Err(e)) => {
            state.status = format!("Save failed: {e}");
            Task::none()
        }

        // --- Gaming ---
        Message::SetGamingMode(on) => {
            state.daemon.gaming_mode = on;
            Task::perform(daemon::set_gaming_mode(on), Message::GamingModeSet)
        }
        Message::GamingModeSet(Ok(_)) => Task::none(),
        Message::GamingModeSet(Err(e)) => {
            state.status = format!("Gaming mode failed: {e}");
            Task::none()
        }
        Message::CycleGamingDpi => {
            Task::perform(daemon::cycle_gaming_dpi(), Message::GamingDpiCycled)
        }
        Message::GamingDpiCycled(Ok(label)) => {
            state.status = if label.is_empty() {
                "DPI cycled".into()
            } else {
                format!("DPI → {label}")
            };
            Task::perform(daemon::poll(), Message::DaemonSnapshotReceived)
        }
        Message::GamingDpiCycled(Err(e)) => {
            state.status = format!("Cycle failed: {e}");
            Task::none()
        }

        // --- App profile bindings ---
        Message::SetAppBindingDraft { class, profile } => {
            state.app_binding_draft = AppBindingDraft { class, profile };
            Task::none()
        }
        Message::AddAppBinding => {
            let class = state.app_binding_draft.class.trim().to_string();
            let profile = state.app_binding_draft.profile.trim().to_string();
            if class.is_empty() || profile.is_empty() {
                state.status = "Both app class and profile name required".into();
                return Task::none();
            }
            state
                .config
                .app_profiles
                .insert(class.clone(), profile.clone());
            state.app_binding_draft = AppBindingDraft::default();
            state.status = format!("Bound {class} → {profile}");
            state.touch();
            Task::none()
        }
        Message::RemoveAppBinding(class) => {
            state.config.app_profiles.remove(&class);
            state.touch();
            Task::none()
        }

        // --- Custom theme editor ---
        Message::ToggleThemeCustomiser => {
            state.theme_editor = match state.theme_editor.take() {
                Some(_) => None,
                None => {
                    let active = juhradial_shared::theme::Theme::load(&state.config.theme)
                        .unwrap_or_else(|| {
                            juhradial_shared::theme::Theme::load(
                                &juhradial_shared::theme::ThemeName::CatppuccinMocha,
                            )
                            .expect("bundled mocha")
                        });
                    Some(ThemeEditor {
                        working: active.colors.clone(),
                        is_dark: active.is_dark,
                        slug: String::new(),
                    })
                }
            };
            Task::none()
        }
        Message::SetThemeColor { field, value } => {
            if let Some(editor) = state.theme_editor.as_mut() {
                set_theme_color_field(&mut editor.working, &field, value);
                // Live-preview the WIP palette on the running UI.
                let preview = juhradial_shared::theme::Theme {
                    name: "(custom)".into(),
                    description: String::new(),
                    is_dark: editor.is_dark,
                    radial_image: None,
                    radial_params: None,
                    colors: editor.working.clone(),
                };
                state.palette = palette::Palette::from_theme(&preview);
            }
            Task::none()
        }
        Message::SetCustomThemeName(s) => {
            if let Some(editor) = state.theme_editor.as_mut() {
                editor.slug = s;
            }
            Task::none()
        }
        Message::SaveCustomTheme => {
            if let Some(editor) = state.theme_editor.as_ref() {
                let slug = sanitize_slug(&editor.slug);
                if slug.is_empty() {
                    state.status = "Theme name required".into();
                    return Task::none();
                }
                let theme = juhradial_shared::theme::Theme {
                    name: editor.slug.trim().to_string(),
                    description: "User-customised theme".into(),
                    is_dark: editor.is_dark,
                    radial_image: None,
                    radial_params: None,
                    colors: editor.working.clone(),
                };
                let result = juhradial_shared::theme::save_user_theme(&slug, &theme)
                    .map(|_| slug)
                    .map_err(|e| e.to_string());
                return Task::perform(async move { result }, Message::CustomThemeSaved);
            }
            Task::none()
        }
        Message::CustomThemeSaved(Ok(slug)) => {
            state.config.theme = juhradial_shared::theme::ThemeName::from(slug.as_str());
            state.palette = palette::Palette::resolve(&state.config.theme);
            state.theme_editor = None;
            state.status = format!("Saved custom theme \"{slug}\"");
            state.touch();
            Task::none()
        }
        Message::CustomThemeSaved(Err(e)) => {
            state.status = format!("Save failed: {e}");
            Task::none()
        }

        // --- HiResScroll ---
        Message::SetHiResScrollHires(v) => apply_hiresscroll_field(state, |h| h.hires = v),
        Message::SetHiResScrollInvert(v) => apply_hiresscroll_field(state, |h| h.invert = v),
        Message::SetHiResScrollTarget(v) => apply_hiresscroll_field(state, |h| h.target = v),
        Message::HiResScrollSet(Ok(_)) => Task::none(),
        Message::HiResScrollSet(Err(e)) => {
            state.status = format!("HiResScroll set failed: {e}");
            Task::none()
        }

        // --- Submenu sub-item editor (active page) ---
        Message::AddSubItem(parent) => {
            if let Some(slice) = state.active_slices_mut().get_mut(parent) {
                slice.submenu.push(juhradial_shared::Slice {
                    action_id: None,
                    label: "New item".into(),
                    kind: juhradial_shared::ActionKind::Exec,
                    command: String::new(),
                    color: "accent".into(),
                    icon: String::new(),
                    submenu: Vec::new(),
                    visible_if: None,
                icon_untinted: false,
                description: String::new(),
                });
                state.touch();
            }
            Task::none()
        }
        Message::DeleteSubItem(parent, idx) => {
            if let Some(slice) = state.active_slices_mut().get_mut(parent) {
                if idx < slice.submenu.len() {
                    slice.submenu.remove(idx);
                    state.touch();
                }
            }
            Task::none()
        }
        Message::SetSubItemLabel(parent, idx, s) => {
            if let Some(item) = state
                .active_slices_mut()
                .get_mut(parent)
                .and_then(|p| p.submenu.get_mut(idx))
            {
                item.label = s;
                state.touch();
            }
            Task::none()
        }
        Message::SetSubItemCommand(parent, idx, s) => {
            if let Some(item) = state
                .active_slices_mut()
                .get_mut(parent)
                .and_then(|p| p.submenu.get_mut(idx))
            {
                item.command = s;
                state.touch();
            }
            Task::none()
        }
        Message::SetSubItemColor(parent, idx, s) => {
            if let Some(item) = state
                .active_slices_mut()
                .get_mut(parent)
                .and_then(|p| p.submenu.get_mut(idx))
            {
                if s == tabs::buttons::FULL_COLOR_KEY {
                    item.icon_untinted = true;
                } else {
                    item.color = s;
                    item.icon_untinted = false;
                }
                state.touch();
            }
            Task::none()
        }
        Message::SetSubItemIcon(parent, idx, s) => {
            if let Some(item) = state
                .active_slices_mut()
                .get_mut(parent)
                .and_then(|p| p.submenu.get_mut(idx))
            {
                item.icon = s;
                state.touch();
            }
            Task::none()
        }
        Message::SetSubItemKind(parent, idx, k) => {
            if let Some(item) = state
                .active_slices_mut()
                .get_mut(parent)
                .and_then(|p| p.submenu.get_mut(idx))
            {
                item.kind = k;
                state.touch();
            }
            Task::none()
        }
        Message::MoveSubItemUp(parent, idx) => {
            if let Some(slice) = state.active_slices_mut().get_mut(parent) {
                if idx > 0 && idx < slice.submenu.len() {
                    slice.submenu.swap(idx, idx - 1);
                    state.touch();
                }
            }
            Task::none()
        }
        Message::MoveSubItemDown(parent, idx) => {
            if let Some(slice) = state.active_slices_mut().get_mut(parent) {
                if idx + 1 < slice.submenu.len() {
                    slice.submenu.swap(idx, idx + 1);
                    state.touch();
                }
            }
            Task::none()
        }

        // --- Multi-page editor handlers ---
        Message::SetActivePage(idx) => {
            if idx < state.config.radial_menu.pages.len() {
                state.active_page = idx;
                // Drop drafts so the textboxes pick up the canonical
                // value when re-rendering for the new page.
                state.app_classes_drafts.clear();
                // Selected slice index belonged to the old page —
                // reset to avoid pointing at a stale slot. Same for
                // the icon picker (its slot index is page-relative).
                state.selected_slice = None;
                state.icon_picker = None;
                state.app_command_picker = None;
            }
            Task::none()
        }
        Message::AddPage => {
            state.config.radial_menu.pages.push(juhradial_shared::RadialPage {
                name: format!("Page {}", state.config.radial_menu.pages.len() + 1),
                slices: Vec::new(),
                app_classes: Vec::new(),
                include_in_scroll: true,
            });
            state.active_page = state.config.radial_menu.pages.len() - 1;
            state.app_classes_drafts.clear();
            state.selected_slice = None;
            state.touch();
            Task::none()
        }
        Message::DeletePage(idx) => {
            // Refuse to delete the last page — the menu always has
            // at least one slice list to render.
            if state.config.radial_menu.pages.len() <= 1 {
                state.status = "Can't delete the only page.".into();
                return Task::none();
            }
            if idx < state.config.radial_menu.pages.len() {
                state.config.radial_menu.pages.remove(idx);
                if state.active_page >= state.config.radial_menu.pages.len() {
                    state.active_page = state.config.radial_menu.pages.len() - 1;
                }
                state.app_classes_drafts.clear();
                state.selected_slice = None;
                state.touch();
            }
            Task::none()
        }
        Message::SetPageName { page, name } => {
            if let Some(p) = state.config.radial_menu.pages.get_mut(page) {
                p.name = name;
                state.touch();
            }
            Task::none()
        }
        Message::SetPageAppClasses { page, value } => {
            // Parse the comma-separated draft into a clean Vec for
            // the on-disk + overlay-side semantics. Keep the raw
            // string in `app_classes_drafts` so the textbox doesn't
            // erase trailing commas mid-typing.
            let parsed: Vec<String> = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if let Some(p) = state.config.radial_menu.pages.get_mut(page) {
                p.app_classes = parsed;
                state.app_classes_drafts.insert(page, value);
                state.touch();
            }
            Task::none()
        }
        Message::SetPageIncludeInScroll { page, value } => {
            if let Some(p) = state.config.radial_menu.pages.get_mut(page) {
                p.include_in_scroll = value;
                state.touch();
            }
            Task::none()
        }
        Message::MovePageLeft(idx) => {
            if idx > 0 && idx < state.config.radial_menu.pages.len() {
                state.config.radial_menu.pages.swap(idx, idx - 1);
                if state.active_page == idx {
                    state.active_page = idx - 1;
                } else if state.active_page == idx - 1 {
                    state.active_page = idx;
                }
                state.app_classes_drafts.clear();
                state.touch();
            }
            Task::none()
        }
        Message::DuplicatePage(idx) => {
            if let Some(src) = state.config.radial_menu.pages.get(idx).cloned() {
                let copy = juhradial_shared::RadialPage {
                    name: format!("{} (copy)", src.name),
                    slices: src.slices,
                    // Clear app_classes on the copy: two pages
                    // matching the same class would create an
                    // ambiguous auto-select. The user can re-add
                    // app classes deliberately.
                    app_classes: Vec::new(),
                    include_in_scroll: src.include_in_scroll,
                };
                state.config.radial_menu.pages.push(copy);
                state.active_page = state.config.radial_menu.pages.len() - 1;
                state.app_classes_drafts.clear();
                state.selected_slice = None;
                state.icon_picker = None;
                state.touch();
            }
            Task::none()
        }
        Message::MovePageRight(idx) => {
            if idx + 1 < state.config.radial_menu.pages.len() {
                state.config.radial_menu.pages.swap(idx, idx + 1);
                if state.active_page == idx {
                    state.active_page = idx + 1;
                } else if state.active_page == idx + 1 {
                    state.active_page = idx;
                }
                state.app_classes_drafts.clear();
                state.touch();
            }
            Task::none()
        }
        Message::DetectFocusedClass(page) => {
            // 4-second deferred sample. Clicking the button steals
            // focus to the settings window, so an immediate
            // GetFocusedWindowClass would just return our own
            // class (or fall back to the next-most-recent app via
            // the extension's stack walk — usually not what the
            // user means). The countdown lets the user alt-tab or
            // click into the target app before the sample fires.
            let delay_secs: u64 = 4;
            let generation = state
                .detect_in_flight
                .map(|d| d.generation.wrapping_add(1))
                .unwrap_or(1);
            state.detect_in_flight = Some(DetectInFlight {
                page,
                deadline: Instant::now() + Duration::from_secs(delay_secs),
                generation,
            });
            state.status = format!(
                "Switch to your target app — sampling focused window in {delay_secs}s…"
            );
            Task::perform(
                async move {
                    let class = crate::cursor_helper::detect_focused_class_after(delay_secs).await;
                    (page, generation, class)
                },
                |(page, generation, class)| Message::DetectedFocusedClass {
                    page,
                    generation,
                    class,
                },
            )
        }
        Message::OpenAppCommandPicker(target) => {
            state.app_command_picker = Some(app_picker::AppCommandPickerState {
                target,
                search: String::new(),
                replace_icon: true,
            });
            // Prewarm app icons (untinted) so the picker rows
            // show real thumbnails instead of placeholders.
            // Cache-aware — repeated opens skip already-rasterised
            // entries.
            let pending: Vec<String> = state
                .installed_apps
                .iter()
                .filter_map(|a| {
                    if a.icon.is_empty() {
                        return None;
                    }
                    if radial_preview::peek_icon_handle_untinted(
                        &state.iced_handles,
                        &a.icon,
                        app_picker::THUMB_PX,
                    )
                    .is_some()
                    {
                        return None;
                    }
                    Some(a.icon.clone())
                })
                .collect();
            if pending.is_empty() {
                return Task::none();
            }
            let size = app_picker::THUMB_PX;
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        pending
                            .into_iter()
                            .map(|name| {
                                let icon =
                                    juhradial_icons::rasterize_icon_untinted(&name, size);
                                (name, icon)
                            })
                            .collect::<Vec<_>>()
                    })
                    .await
                    .unwrap_or_default()
                },
                Message::AppIconsPrewarmed,
            )
        }
        Message::CloseAppCommandPicker => {
            state.app_command_picker = None;
            Task::none()
        }
        Message::SetAppCommandSearch(q) => {
            if let Some(p) = state.app_command_picker.as_mut() {
                p.search = q;
            }
            Task::none()
        }
        Message::SetAppCommandReplaceIcon(v) => {
            if let Some(p) = state.app_command_picker.as_mut() {
                p.replace_icon = v;
            }
            Task::none()
        }
        Message::PickAppForCommand { command, icon, label } => {
            // Read the picker's flags BEFORE we take() it, so the
            // replace_icon toggle is honoured.
            let (target, replace_icon) = match state.app_command_picker.take() {
                Some(p) => (p.target, p.replace_icon),
                None => return Task::none(),
            };
            match target {
                app_picker::AppCommandTarget::Slice(idx) => {
                    if let Some(slice) = state.active_slices_mut().get_mut(idx) {
                        slice.command = command;
                        slice.kind = juhradial_shared::ActionKind::Exec;
                        if slice.label.trim().is_empty() {
                            slice.label = label;
                        }
                        if replace_icon {
                            slice.icon = icon;
                            slice.icon_untinted = true;
                        }
                        state.touch();
                    }
                }
                app_picker::AppCommandTarget::SubItem { parent, idx } => {
                    if let Some(item) = state
                        .active_slices_mut()
                        .get_mut(parent)
                        .and_then(|p| p.submenu.get_mut(idx))
                    {
                        item.command = command;
                        item.kind = juhradial_shared::ActionKind::Exec;
                        if item.label.trim().is_empty() {
                            item.label = label;
                        }
                        if replace_icon {
                            item.icon = icon;
                            item.icon_untinted = true;
                        }
                        state.touch();
                    }
                }
            }
            Task::none()
        }
        Message::OpenIconPicker(target) => {
            state.icon_picker = Some(icon_picker::IconPickerState {
                target,
                search: String::new(),
                source: icon_picker::IconSource::Catalogue,
            });
            // Kick off the off-thread prewarm so the catalogue
            // grid populates without blocking the UI thread.
            Task::done(Message::PrewarmIcons)
        }
        Message::SetIconPickerSource(src) => {
            if let Some(p) = state.icon_picker.as_mut() {
                p.source = src;
                p.search = String::new();
            }
            // Switching to Apps may need a different prewarm pass
            // — the catalogue prewarm rasterised symbolic icons
            // tinted to the theme text colour, but apps render
            // untinted. Kick another pass; it filters cache hits
            // so already-warmed entries are skipped.
            Task::done(Message::PrewarmIcons)
        }
        Message::PrewarmIcons => {
            if state.icon_picker.is_none() {
                return Task::none();
            }
            let source = state
                .icon_picker
                .as_ref()
                .map(|p| p.source)
                .unwrap_or(icon_picker::IconSource::Catalogue);

            match source {
                icon_picker::IconSource::Catalogue => {
                    // Catalogue source: rasterise tinted variants
                    // of curated icons + recents.
                    let tint = state.palette.text;
                    let mut candidates: Vec<&str> =
                        icon_picker::COMMON_ICONS.iter().copied().collect();
                    for r in state.recent_icons.iter() {
                        if !candidates.iter().any(|c| *c == r.as_str()) {
                            candidates.push(r.as_str());
                        }
                    }
                    let pending: Vec<String> = candidates
                        .into_iter()
                        .filter(|n| {
                            radial_preview::peek_icon_handle(
                                &state.iced_handles,
                                n,
                                icon_picker::THUMB_PX,
                                tint,
                            )
                            .is_none()
                        })
                        .map(|s| s.to_string())
                        .collect();
                    if pending.is_empty() {
                        return Task::none();
                    }
                    let size = icon_picker::THUMB_PX;
                    let color = (tint.r, tint.g, tint.b, tint.a);
                    Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                pending
                                    .into_iter()
                                    .map(|name| {
                                        let icon = juhradial_icons::rasterize_icon(&name, size, color);
                                        (name, icon)
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .await
                            .unwrap_or_default()
                        },
                        Message::IconsPrewarmed,
                    )
                }
                icon_picker::IconSource::Apps => {
                    // Apps source: rasterise untinted variants
                    // (preserve brand colours). Sentinel colour
                    // 1.0,1.0,1.0,1.0 on the rasteriser keeps the
                    // alpha mask from being repainted with a
                    // single colour — it's still tint-aware, but
                    // a white-on-white tint preserves the source
                    // pixels.
                    let pending: Vec<String> = state
                        .installed_apps
                        .iter()
                        .filter_map(|a| {
                            if a.icon.is_empty() {
                                return None;
                            }
                            if radial_preview::peek_icon_handle_untinted(
                                &state.iced_handles,
                                &a.icon,
                                icon_picker::THUMB_PX,
                            )
                            .is_some()
                            {
                                return None;
                            }
                            Some(a.icon.clone())
                        })
                        .collect();
                    if pending.is_empty() {
                        return Task::none();
                    }
                    let size = icon_picker::THUMB_PX;
                    Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                pending
                                    .into_iter()
                                    .map(|name| {
                                        // True untinted path:
                                        // preserves the original
                                        // RGBA pixels of the
                                        // app's icon (Firefox
                                        // orange, Chromium blue,
                                        // etc.). Symbolic icons
                                        // routed through this
                                        // path show as black-on-
                                        // transparent, which is
                                        // fine — the picker
                                        // shouldn't have many
                                        // symbolic-only apps.
                                        let icon = juhradial_icons::rasterize_icon_untinted(
                                            &name, size,
                                        );
                                        (name, icon)
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .await
                            .unwrap_or_default()
                        },
                        Message::AppIconsPrewarmed,
                    )
                }
            }
        }
        Message::IconsPrewarmed(results) => {
            // Picker may have closed during the worker pass —
            // installing handles is still cheap and keeps the cache
            // useful for next time, so we don't bail.
            let tint = state.palette.text;
            for (name, icon) in results {
                if let Some(icon) = icon {
                    radial_preview::install_icon_handle(
                        &state.iced_handles,
                        &name,
                        icon_picker::THUMB_PX,
                        tint,
                        icon,
                    );
                }
            }
            Task::none()
        }
        Message::AppIconsPrewarmed(results) => {
            // Same install path as IconsPrewarmed but with the
            // untinted cache key (color = 0). The picker's
            // `peek_icon_handle_untinted` reads from the same key,
            // so apps render in their original brand colours.
            for (name, icon) in results {
                if let Some(icon) = icon {
                    radial_preview::install_icon_handle_untinted(
                        &state.iced_handles,
                        &name,
                        icon_picker::THUMB_PX,
                        icon,
                    );
                }
            }
            Task::none()
        }
        Message::CloseIconPicker => {
            state.icon_picker = None;
            Task::none()
        }
        Message::SetIconPickerSearch(q) => {
            if let Some(p) = state.icon_picker.as_mut() {
                p.search = q;
            }
            Task::none()
        }
        Message::PickIcon(name) => {
            // Apply the picked icon to whichever target the picker
            // was opened against, then close. The Apps source is
            // naturally full-colour, so we flip icon_untinted on
            // by default — that way the radial menu shows the
            // brand colours the user picked the app for.
            let prefer_untinted = state
                .icon_picker
                .as_ref()
                .map(|p| p.source == icon_picker::IconSource::Apps)
                .unwrap_or(false);
            apply_icon_to_target(
                state,
                state.icon_picker.as_ref().map(|p| p.target),
                name.clone(),
                prefer_untinted,
            );
            state.icon_picker = None;
            // Persist the recents list so next session opens with
            // the user's frequently-reached icons at the top of
            // the picker. Async because file IO; result message
            // updates state.recent_icons when the write lands.
            let prev = state.recent_icons.clone();
            Task::perform(
                recents::save_after_pick(name, prev),
                Message::RecentIconsPersisted,
            )
        }
        Message::RecentIconsPersisted(items) => {
            state.recent_icons = items;
            Task::none()
        }
        Message::BrowseIconFile(target) => {
            // Native file dialog (xdg-desktop-portal on Wayland).
            // Filtered to common icon formats. The dialog blocks
            // until the user picks or cancels — done off the iced
            // runtime via Task::perform so the UI stays responsive.
            Task::perform(
                async move {
                    let chosen = rfd::AsyncFileDialog::new()
                        .set_title("Pick an icon file")
                        .add_filter("Icons", &["svg", "png", "jpg", "jpeg", "webp", "ico"])
                        .add_filter("All files", &["*"])
                        .pick_file()
                        .await
                        .map(|h| h.path().to_string_lossy().into_owned());
                    (target, chosen)
                },
                |(target, path)| Message::IconFileChosen { target, path },
            )
        }
        Message::IconFileChosen { target, path } => {
            if let Some(p) = path {
                // File-picker selections are typically PNGs/SVGs
                // with their own colours — default to untinted so
                // the user's chosen artwork survives unmodified.
                apply_icon_to_target(state, Some(target), p.clone(), true);
                // Close any open inline picker — the user used the
                // file dialog instead, no reason to leave the grid
                // open over the editor.
                state.icon_picker = None;
                // File-picker selections feed into recents the
                // same way grid clicks do, so the next time the
                // user opens the inline picker the file path is
                // one click away (with a thumbnail, since the
                // prewarmer rasterises recents too).
                let prev = state.recent_icons.clone();
                return Task::perform(
                    recents::save_after_pick(p, prev),
                    Message::RecentIconsPersisted,
                );
            }
            Task::none()
        }
        Message::CancelFocusedClassDetect => {
            // The Task::perform sleep is still running, but we
            // bump the generation by clearing detect_in_flight
            // and the result handler will discard the stale
            // sample when it finally fires.
            state.detect_in_flight = None;
            state.status = "Detect cancelled.".into();
            Task::none()
        }
        Message::DetectedFocusedClass { page, generation, class } => {
            // Drop stale samples from runs the user cancelled or
            // restarted before this one finished.
            let in_flight = state.detect_in_flight;
            let stale = match in_flight {
                Some(d) => d.generation != generation,
                None => true, // cancelled
            };
            if stale {
                return Task::none();
            }
            state.detect_in_flight = None;
            match class {
                Some(c) => {
                    if let Some(p) = state.config.radial_menu.pages.get_mut(page) {
                        // Only append when the class isn't already
                        // present — repeated clicks shouldn't grow
                        // the list with duplicates.
                        if !p.app_classes.iter().any(|existing| existing == &c) {
                            p.app_classes.push(c.clone());
                            state.touch();
                        }
                        // Drop any in-flight CSV draft so the input
                        // re-renders from the canonical Vec — gives
                        // the user immediate visual feedback that
                        // the class landed.
                        state.app_classes_drafts.remove(&page);
                        state.status = format!("Captured class \"{c}\"");
                    }
                }
                None => {
                    state.status =
                        "Couldn't detect a focused window (extension missing, or focus is on the desktop?)"
                            .into();
                }
            }
            Task::none()
        }
    }
}

// ============================================================================
// View — the shell
// ============================================================================

/// Wrap a picker's content in a full-panel chrome — top bar with
/// title + Back button, body fills the remaining space. Used when
/// a picker takes over the main content area instead of rendering
/// inline next to the slice editor.
fn full_panel<'a>(
    state: &'a State,
    title: &str,
    body: Element<'a, Message>,
    back_msg: Message,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let back_btn = iced::widget::button(text("← Back").size(12))
        .style(style::btn_secondary(pal))
        .on_press(back_msg);
    let header = row![
        back_btn,
        Space::new().width(Length::Fixed(12.0)),
        text(title.to_string()).size(16),
        Space::new().width(Length::Fill),
    ]
    .align_y(iced::Alignment::Center);
    column![
        header,
        rule::horizontal(1).style(style::rule_style(pal)),
        Space::new().height(Length::Fixed(8.0)),
        body,
    ]
    .spacing(6)
    .into()
}

fn view(state: &State) -> Element<'_, Message> {
    let header = header_view(state);
    let sidebar = sidebar_view(state);
    // Full-panel pickers take over the entire content area when
    // open — gives the user the full width / height of the
    // settings window for browsing instead of cramming the grid
    // into the right column. Back button at the top returns to
    // whichever tab the user was on. Order matters: app-command
    // picker wins over icon picker if both are somehow open.
    let body: Element<Message> = if let Some(p) = state.app_command_picker.as_ref() {
        full_panel(state, "Pick app for command", app_picker::view(state, p), Message::CloseAppCommandPicker)
    } else if let Some(p) = state.icon_picker.as_ref() {
        full_panel(state, "Pick an icon", icon_picker::view(state, p), Message::CloseIconPicker)
    } else {
        match state.tab {
            Tab::Buttons => tabs::buttons::view(state),
            Tab::Settings => tabs::settings_page::view(state),
            Tab::PointScroll => tabs::scroll::view(state),
            Tab::Haptic => tabs::haptics::view(state),
            Tab::Devices => tabs::devices::view(state),
            Tab::EasySwitch => tabs::easyswitch::view(state),
            Tab::Flow => tabs::placeholder::view(state,
                "Flow",
                "Cross-machine cursor-and-clipboard hand-off. Coming soon.",
            ),
            Tab::Macros => tabs::macros::view(state),
            Tab::Gaming => tabs::gaming::view(state),
        }
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
            chip(
                state,
                &state
                    .daemon
                    .device_name
                    .clone()
                    .unwrap_or_else(|| "MX MASTER 4".to_string())
                    .to_uppercase(),
            ),
            Space::new().width(Length::Fixed(10.0)),
            // Battery icon — body + nub fit in the canvas, % text
            // overlaid on the body. Compact (50 px) since we don't
            // need to leave room for an external label any more.
            battery::widget(pal, state.battery, 56.0),
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
    let pal = &state.palette;
    let mut inner = row![
        text(tab.glyph()).size(13),
        text(tab.label()).size(13),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(12);

    if tab.is_stub() {
        inner = inner.push(Space::new().width(Length::Fill));
        inner = inner.push(
            container(text("STUB").size(8))
                .padding([1, 5])
                .style(style::chip(pal)),
        );
    }

    button(inner)
        .width(Length::Fill)
        .padding([10, 14])
        .style(style::nav_item(pal, active))
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
    let mut subs = vec![
        iced::time::every(Duration::from_millis(200)).map(|_| Message::SaveTick),
        // UPower poll — 30 s is plenty for steady state. The first
        // probe fires from `boot()` so the indicator isn't blank
        // for the full 30 s after launch.
        iced::time::every(Duration::from_secs(30)).map(|_| Message::BatteryTick),
        // Daemon snapshot — tighter cadence (5 s) since DPI / host
        // / battery from HID++ are essentially free to query
        // compared to UPower.
        iced::time::every(Duration::from_secs(5)).map(|_| Message::DaemonTick),
    ];
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
    // Default filter: info for our crates, error-only for usvg (it
    // floods at warn level on freedesktop icons that use legitimate
    // `marker-start="none"` CSS — rendering is unaffected).
    let default_filter = "info,usvg=error";
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_filter)),
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
