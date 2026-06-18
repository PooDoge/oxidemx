//! OxideMX settings GUI.
//!
//! Layout mirrors the legacy oxidemx settings dialog: a left
//! sidebar with the 9 top-level sections (Buttons by default), a
//! header, the section's main + side content in the middle, and a
//! footer with credits + exit. The on-disk channel to the live
//! overlay is unchanged — every edit is debounced + atomic-written
//! back to `~/.config/oxidemx/config.json` and the overlay's
//! existing inotify watcher previews changes within ~150 ms.

mod tabs {
    pub mod agents;
    pub mod ai;
    pub mod animation;
    pub mod buttons;
    pub mod devices;
    pub mod easyswitch;
    pub mod gaming;
    pub mod haptics;
    pub mod indicator_popup;
    pub mod macros;
    pub mod mouse_buttons;
    pub mod placeholder;
    pub mod scroll;
    pub mod settings_page;
    pub mod visuals;
}
mod animation_editor;
mod app_picker;
mod battery;
mod bundle;
mod color_canvas;
mod cursor_helper;
mod daemon;
mod fonts;
mod geocode;
mod icon_picker;
mod mouse_callouts;
mod persist;
mod radial_preview;
mod raise;
mod recents;
mod singleton;
mod theme_customiser;
mod ui_state;
mod widget_preview;
mod widget_store;

use iced::widget::{button, column, container, row, rule, scrollable, text, Space};
use iced::{Element, Length, Subscription, Task};
use oxidemx_shared::{
    AnimationConfig, AppConfig, ElementAnimation, HapticEventMode, HapticRedirectCurve,
    HapticRedirectMode, TransitionConfig, VisualSettings,
};
use oxidemx_widgets::icons::icon;
use oxidemx_widgets::{palette, style};
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
    /// Per-button action assignments for the MX Master 4 itself
    /// (left, right, side, gesture, etc. → action mapping).
    MouseButtons,
    /// Radial menu editor — page picker, slice editor, preview.
    /// Was bundled with `MouseButtons` in the original "Buttons"
    /// tab; split out here so each concern gets the full window.
    Menu,
    PointScroll,
    IndicatorPopup,
    Haptic,
    Devices,
    EasySwitch,
    Flow,
    Macros,
    Gaming,
    /// Agent runtime config — backend, model, API key, allowlist.
    Ai,
    /// Multi-agent flows, roster, tool registry, MCP servers — the
    /// conductor's management surface (browse/validate/launch).
    Agents,
    Settings,
}

impl Tab {
    pub fn label(&self) -> &'static str {
        match self {
            Tab::MouseButtons => "Mouse Buttons",
            Tab::Menu => "Menu",
            Tab::PointScroll => "Point & Scroll",
            Tab::IndicatorPopup => "Indicator Popup",
            Tab::Haptic => "Haptic Feedback",
            Tab::Devices => "Devices",
            Tab::EasySwitch => "Easy-Switch",
            Tab::Flow => "Flow",
            Tab::Macros => "Macros",
            Tab::Gaming => "Gaming",
            Tab::Ai => "AI",
            Tab::Agents => "Agents",
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

    /// Single-glyph icon shown in the sidebar — used as a fallback
    /// when the freedesktop symbolic icon named by `icon_name()`
    /// isn't available in the current theme. Picked to read at a
    /// glance.
    pub fn glyph(&self) -> &'static str {
        match self {
            Tab::MouseButtons => "M",
            Tab::Menu => "R",
            Tab::PointScroll => "S",
            Tab::IndicatorPopup => "I",
            Tab::Haptic => "H",
            Tab::Devices => "D",
            Tab::EasySwitch => "E",
            Tab::Flow => "F",
            Tab::Macros => "P",
            Tab::Gaming => "G",
            Tab::Ai => "A",
            Tab::Agents => "@",
            Tab::Settings => "*",
        }
    }

    /// freedesktop / hicolor symbolic icon name for the tab.
    /// The sidebar tries to resolve this through the shared
    /// `IconCache` first; an unresolved name falls back to
    /// `glyph()` so the sidebar always renders something useful
    /// even on bare-bones themes.
    pub fn icon_name(&self) -> &'static str {
        match self {
            Tab::MouseButtons => "input-mouse-symbolic",
            Tab::Menu => "applications-graphics-symbolic",
            Tab::PointScroll => "preferences-desktop-cursors-symbolic",
            Tab::IndicatorPopup => "applications-system-symbolic",
            Tab::Haptic => "audio-volume-high-symbolic",
            Tab::Devices => "computer-symbolic",
            Tab::EasySwitch => "system-switch-user-symbolic",
            Tab::Flow => "view-grid-symbolic",
            Tab::Macros => "media-playback-start-symbolic",
            Tab::Gaming => "applications-games-symbolic",
            Tab::Ai => "applications-science-symbolic",
            Tab::Agents => "system-run-symbolic",
            Tab::Settings => "preferences-system-symbolic",
        }
    }

    /// Stable, lower-case tag for serialising the active tab to
    /// disk. Matches no user-facing string so renaming a label
    /// won't invalidate saved state.
    pub fn tag(self) -> &'static str {
        match self {
            Tab::MouseButtons => "mouse_buttons",
            Tab::Menu => "menu",
            Tab::PointScroll => "point_scroll",
            Tab::IndicatorPopup => "indicator_popup",
            Tab::Haptic => "haptic",
            Tab::Devices => "devices",
            Tab::EasySwitch => "easy_switch",
            Tab::Flow => "flow",
            Tab::Macros => "macros",
            Tab::Gaming => "gaming",
            Tab::Ai => "ai",
            Tab::Agents => "agents",
            Tab::Settings => "settings",
        }
    }

    /// Inverse of `tag()` — None when the input is unknown
    /// (handles the case of a tag stored before a tab was
    /// added/removed). Legacy `"buttons"` tag (from before the
    /// Mouse-buttons / Menu split) maps to `Tab::Menu` so users
    /// who saved their state on the old combined tab don't get
    /// kicked back to the default on first launch after upgrade.
    pub fn from_tag(s: &str) -> Option<Self> {
        Some(match s {
            "mouse_buttons" => Tab::MouseButtons,
            "menu" => Tab::Menu,
            // Legacy: combined "Buttons" tab → split. The radial
            // menu editor (now `Tab::Menu`) is the more
            // commonly-used half of the two, so default to it.
            "buttons" => Tab::Menu,
            "point_scroll" => Tab::PointScroll,
            "indicator_popup" => Tab::IndicatorPopup,
            "haptic" => Tab::Haptic,
            "devices" => Tab::Devices,
            "easy_switch" => Tab::EasySwitch,
            "flow" => Tab::Flow,
            "macros" => Tab::Macros,
            "gaming" => Tab::Gaming,
            "ai" => Tab::Ai,
            "agents" => Tab::Agents,
            "settings" => Tab::Settings,
            _ => return None,
        })
    }

    pub const ALL: [Tab; 13] = [
        Tab::MouseButtons,
        Tab::Menu,
        Tab::PointScroll,
        Tab::IndicatorPopup,
        Tab::Haptic,
        Tab::Devices,
        Tab::EasySwitch,
        Tab::Flow,
        Tab::Macros,
        Tab::Gaming,
        Tab::Ai,
        Tab::Agents,
        Tab::Settings,
    ];
}

// ============================================================================
// Messages
// ============================================================================

#[derive(Debug, Clone)]
pub enum Message {
    SwitchTab(Tab),
    /// Weather-location geocoder (Settings tab).
    SetWeatherQuery(String),
    WeatherSearch,
    WeatherResults(Result<Vec<geocode::GeoHit>, String>),
    /// Pick result `idx` → persist overlay.weather_location/_place.
    WeatherPick(usize),
    WeatherClearLocation,
    /// Temperature unit toggle — `true` = Celsius (°F is default).
    SetWeatherCelsius(bool),
    /// Live-widget data source for slice `idx` (Buttons tab editor).
    SetSliceWidgetSource(usize, oxidemx_shared::WidgetSource),
    /// Dial target for slice `idx`.
    SetSliceDial(usize, oxidemx_shared::DialKind),
    SetVisual(VisualField, f32),
    /// AI-chat status effect picker: (status index 0=thinking
    /// 1=awaiting 2=idle, effect slug).
    SetAiFxEffect(usize, String),
    SetAiFxIntensity(usize, f32),
    SetAiFxSpeed(usize, f32),
    /// Toggle the inline colour picker for (status, palette slot).
    ToggleAiFxColorPicker(usize, usize),
    /// SV-square change for (status, slot).
    SetAiFxColorSv(usize, usize, f32, f32),
    /// Hue-strip change for (status, slot).
    SetAiFxColorHue(usize, usize, f32),
    /// Hex text input for (status, slot).
    SetAiFxColorHex(usize, usize, String),
    /// Drop a status's custom palette → follow the theme again.
    ResetAiFxColors(usize),
    /// Font family override for rendered text (Visuals tab).
    /// Empty string = system default.
    SetFontFamily(String),
    /// Arced-tooltip hover delay in milliseconds. 0 = instant.
    SetTooltipDelay(u32),
    /// Toggle the centre-puck page-name flash on/off.
    SetPageNameShow(bool),
    /// Page-name flash visible duration (full-opacity hold) ms.
    SetPageNameVisibleMs(u32),
    /// Page-name slide-in / fade-out duration ms.
    SetPageNameTransitionMs(u32),
    /// Horizontal slide distance for page-name flash. 0 = pure
    /// crossfade.
    SetPageNameSlideDistance(f32),
    /// Toggle the arced (above-puck) layout vs the flat
    /// (centre-puck) layout for the page-name flash.
    SetPageNameArced(bool),
    /// Toggle monospace for the page-name flash. Strongly
    /// recommended for arced layouts (proportional fonts leave
    /// uneven gaps because every char gets a uniform angular
    /// slot).
    SetPageNameUseMonospace(bool),
    /// Page-name-specific font family override. Empty = inherit
    /// the menu font_family. Ignored when monospace is forced.
    SetPageNameFontFamily(String),
    /// Toggle whether the tooltip forces monospace.
    SetTooltipUseMonospace(bool),
    /// Tooltip-specific font family override. Empty string =
    /// inherit `visuals.font_family`.
    SetTooltipFontFamily(String),
    /// Palette key for the tooltip's background ribbon. One of
    /// the surface keys (`crust`, `surface0`, …).
    SetTooltipBgColor(String),
    /// Background ribbon alpha multiplier in [0, 1].
    SetTooltipBgAlpha(f32),
    /// Palette key for the tooltip's text colour.
    SetTooltipTextColor(String),
    /// Reset every tooltip-styling field back to its default.
    /// Doesn't touch `tooltip_font_size` or `tooltip_delay_ms` —
    /// those are layout knobs, not styling.
    ResetTooltipStyle,
    /// Fire-and-forget haptic preview from the Haptics tab.
    /// `event` is the daemon's per-event identifier
    /// (`"menu_appear"`, `"slice_change"`, etc.).
    TestHapticEvent(String),
    /// Result of a `TestHapticEvent` — only used to swallow the
    /// completion of the fire-and-forget D-Bus call so iced has a
    /// concrete message to dispatch back. No state changes.
    HapticTestFired,
    SetTransition(AnimElement, AnimDirection, TransitionConfig),
    SetChainStagger(AnimElement, u32),
    ResetElementAnimation(AnimElement),
    /// Replace the page-cycle transition config (Animation tab,
    /// Page-transition card). Carries the full block so the picker
    /// + sliders can each just clone-mutate-emit.
    SetPageTransition(oxidemx_shared::PageTransitionConfig),
    /// Pick the dispatch-burst shader style (Sparks / Shockwave / Glow).
    SetDispatchBurstStyle(oxidemx_shared::DispatchBurstStyle),
    /// Reset the page-cycle transition to its default (spin +
    /// crossfade, 220 ms, ease-out, 22.5° rotation).
    ResetPageTransition,
    /// Empty tick fired during the status auto-fade tail to keep
    /// the alpha ramp rendering at ~30 fps. Handler does nothing
    /// — the redraw side effect is the whole point.
    StatusFadeTick,
    /// Tab-persistence fire-and-forget completion. We just need a
    /// concrete Message to dispatch back so iced has a typed
    /// completion; the handler is a no-op.
    LastTabPersisted,
    /// AI Assistant: the Gemini API key draft text changed
    /// (Settings tab — the field is write-only; the stored key is
    /// never loaded back into the UI).
    AiKeyDraftChanged(String),
    /// Agents tab: re-scan flows/roster/MCP from disk.
    AgentsRefresh,
    /// Agents tab: open Mission Control, pre-selecting this flow id.
    AgentsRunFlow(String),
    /// Agents tab: edit the "new flow" name field.
    AgentsNewFlowDraft(String),
    /// Agents tab: scaffold a starter flow from the draft name.
    AgentsCreateFlow,
    /// Agents tab: open a flow's flow.md in the in-GUI editor.
    AgentsEditFlow(String),
    /// Agents tab: a text_editor action in the flow editor.
    AgentsEditorAction(iced::widget::text_editor::Action),
    /// Agents tab: save the editor buffer back to flow.md.
    AgentsSaveFlow,
    /// Agents tab: close the flow editor.
    AgentsCloseEditor,
    /// AI tab: Gemini transport backend changed.
    AiProviderChanged(oxidemx_shared::config::AiProvider),
    /// AI tab: agent model id edited/picked.
    AiModelChanged(String),
    /// AI tab: local OpenAI-compatible endpoint edited (MistralRs provider).
    AiLocalEndpointChanged(String),
    /// AI tab: hybrid routing toggled.
    AiRoutingToggled(bool),
    /// AI tab: fast/local routing provider changed.
    AiFastProviderChanged(oxidemx_shared::config::AiProvider),
    /// AI tab: fast/local routing model edited/picked.
    AiFastModelChanged(String),
    /// AI tab: allowlist add-form draft edited.
    AiAllowlistDraftChanged(String),
    /// AI tab: commit the allowlist draft as a new entry.
    AiAllowlistAdd,
    /// AI tab: remove the allowlist entry at this index.
    AiAllowlistRemove(usize),
    /// AI tab: local-model download directory path edited directly.
    AiModelDirChanged(String),
    /// AI tab: open a native folder picker to choose the model download dir.
    AiModelDirPick,
    /// AI tab: idle-timeout (seconds) text field edited.
    AiIdleTimeoutChanged(String),
    /// AI Assistant: persist the drafted key to
    /// `~/.config/oxidemx/gemini.key` (created 0600). The overlay
    /// re-reads the file on every prompt, so no restart is needed.
    AiKeySave,
    /// AI Assistant: delete the stored key file.
    AiKeyRemove,
    /// Open a save dialog to write the current config to a JSON
    /// file. Useful for backups, sharing setups, or migrating
    /// between machines.
    ExportConfig,
    /// Result of `ExportConfig` — `Ok(path)` on success,
    /// `Err(message)` on failure or user cancellation.
    ConfigExported(Result<String, String>),
    /// Open a file picker to import a config JSON. Replaces the
    /// current config wholesale; the existing reset-armed timer
    /// gates the destructive part with a status warning.
    ImportConfig,
    /// Result of `ImportConfig` — `Ok((parsed_config, errors))`
    /// if the file loaded + parsed (errors are non-fatal issues
    /// from individual macro/theme writes), `Err(message)` on
    /// read/parse failure or cancellation.
    ConfigImported(Result<(Box<oxidemx_shared::AppConfig>, Vec<String>), String>),
    /// Open the config directory in the user's file manager via
    /// `xdg-open`. Spawned detached so the settings UI doesn't
    /// block on the file manager's startup.
    OpenConfigFolder,
    /// Import a theme JSON from disk. Picks a file, parses it as
    /// `Theme`, saves under `~/.local/share/oxidemx/themes/`
    /// using a slug derived from the file's stem, and switches
    /// the picker to it.
    ImportTheme,
    /// Result of `ImportTheme` — `Ok(slug)` to switch to the
    /// imported theme, `Err(message)` for status feedback.
    ThemeImported(Result<String, String>),
    /// Save the named user theme to a user-chosen file. The
    /// theme is loaded fresh from disk so any in-flight customiser
    /// edits don't leak in unexpectedly.
    ExportTheme(String),
    /// Result of `ExportTheme` — `Ok(path)` for status, `Err(msg)`
    /// for failure or cancellation.
    ThemeExported(Result<String, String>),
    /// Open the rename editor for a saved user theme — flips
    /// `state.renaming_theme` to `Some(slug)` so the picker row
    /// shows a text input.
    BeginRenameTheme(String),
    /// Update the in-flight rename draft. Stored on State because
    /// the user can edit across multiple frames before committing.
    SetRenameThemeDraft(String),
    /// Commit the in-flight rename — saves the theme under the
    /// new slug, deletes the old file, switches the active picker
    /// when the renamed theme was active.
    CommitRenameTheme,
    /// Cancel the in-flight rename, discarding the draft.
    CancelRenameTheme,
    /// Result of the rename's async file work.
    ThemeRenamed(Result<(String, String), String>),
    /// Begin capturing the next key chord pressed by the user
    /// into the named slice / sub-item's command field. Switches
    /// the editor into a "press a chord…" mode and arms the
    /// keyboard subscription. Cancel by pressing Esc or clicking
    /// the Capture button again.
    BeginShortcutCapture(ShortcutCaptureTarget),
    /// Cancel an in-flight capture without writing a value.
    CancelShortcutCapture,
    /// A key chord arrived from the keyboard subscription.
    /// `chord` is already formatted in xdotool style
    /// (`"ctrl+shift+v"`); writing it triggers `SetSliceCommand` /
    /// `SetSubItemCommand` for the captured target and clears
    /// `capturing_shortcut`.
    ShortcutCaptured(String),
    /// Open the radial overlay so the user can preview the
    /// currently-tweaked animation / theme without lifting hands
    /// off the keyboard. Daemon picks up the show request and the
    /// overlay positions itself at the user's cursor (or screen
    /// centre when cursor pos isn't available).
    OpenOverlayForPreview,
    /// Result of `OpenOverlayForPreview` — fire-and-forget; we
    /// just need a Message to dispatch back so iced has a typed
    /// completion.
    OverlayPreviewFired,
    ResetAll,
    /// Easy-Switch shortcut toggle (Buttons tab right column).
    SetEasySwitchShortcuts(bool),
    /// Quit the settings window.
    Exit,
    /// Another `oxidemx-settings` invocation called Focus on us
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
    SetSliceKind(usize, oxidemx_shared::ActionKind),
    SetSliceColor(usize, String),
    /// Set the icon name (freedesktop symbolic name, an absolute
    /// path to an SVG/PNG, or the legacy internal id) for a slice
    /// on the active page.
    SetSliceIcon(usize, String),
    /// Set the slice's description / tooltip text. Stored as
    /// metadata; not currently rendered in the overlay.
    SetSliceDescription(usize, String),
    /// Same for a submenu sub-item.
    SetSubItemDescription {
        parent: usize,
        idx: usize,
        value: String,
    },
    /// Replace a slice's visibility predicate. `None` clears the
    /// predicate (slice is always visible). `Some(Always)` is
    /// equivalent at runtime; we write the slimmer `None` shape on
    /// disk for that case.
    SetSliceVisibility {
        slice: usize,
        condition: Option<oxidemx_shared::Condition>,
    },
    /// Sub-item analogue of `SetSliceVisibility`. Same shape, just
    /// addressed under a parent slice's submenu.
    SetSubItemVisibility {
        parent: usize,
        idx: usize,
        condition: Option<oxidemx_shared::Condition>,
    },
    /// Spawn the slice's command via `sh -c` so the user can
    /// validate shell quoting + that the command actually launches
    /// before relying on the radial menu to dispatch it. Non-Exec
    /// slice kinds (Macro, EasySwitch, …) emit a status hint
    /// instead — those round-trip through the daemon and aren't
    /// useful to test in isolation.
    TestSliceAction(usize),
    /// Same as TestSliceAction but for a submenu sub-item.
    TestSubItemAction {
        parent: usize,
        idx: usize,
    },
    /// Window resize (width, logical px) — keeps `State::
    /// window_width` current for the picker grid's 3/4-up split.
    WindowResized(f32),
    /// Radial preview interactions.
    SelectSlice(usize),
    DismissSliceSelection,
    SwapSlices {
        from: usize,
        to: usize,
    },

    // --- Behavior chip + action/widget picker panel (spec §10b/c) ---
    /// `Change…` on a slice's behavior chip — expands the picker
    /// panel for that slot and snapshots the slice for
    /// undo-by-reselect.
    OpenPicker(usize),
    /// `Cancel` on the chip / explicit close — collapses the panel
    /// without applying and drops the undo snapshot.
    ClosePicker,
    /// Live text of the picker's search field.
    PickerSearch(String),
    /// Click on a built-in action tile: applies the kind (or
    /// restores the undo snapshot when the tile matches it) and
    /// closes the panel.
    PickAction(usize, oxidemx_shared::ActionKind),
    /// Click on a widget tile (built-in source or installed
    /// `Custom(id)`): sets kind=Widget + a fresh `WidgetConfig`
    /// (instance_key for custom widgets), auto-labels, closes the
    /// panel. Restores the undo snapshot when the tile matches it.
    PickWidget(usize, oxidemx_shared::WidgetSource),
    /// "Get more widgets…" tile (picker) / Reinstall button
    /// (missing-widget chip). Opens the store/downloader dialog.
    OpenWidgetStore,
    /// Re-scan `~/.config/oxidemx/widgets` into the settings-side
    /// registry cache. Triggered after store install/uninstall.
    RescanWidgets,
    /// "Convert" on a legacy NATIVE widget slice whose bundled
    /// plugin replacement is installed+ready (spec §16): rewrites
    /// `source` → `Custom(id)`, assigns an instance_key, lifts
    /// legacy weather settings into the instance bag. Label and
    /// colour are kept.
    ConvertSliceToPlugin(usize),

    // --- Widget store / downloader dialog (spec §11) ---
    /// Back button on the store panel (also the "Settings" jump on
    /// an installed row — the options card lives on the slice
    /// editor behind the dialog).
    CloseWidgetStore,
    /// Live text of the store's list filter field.
    StoreSearch(String),
    /// Live text of the "Install from URL…" field.
    StoreUrlInput(String),
    /// "Install from file…" — native picker → verified install.
    StoreInstallFromFile,
    /// "Install from URL…" — curl to a temp file → same install.
    StoreInstallFromUrl,
    /// Outcome of any install attempt (file, URL, or consent
    /// retry). Success rescans the registry; sideload refusals
    /// surface the consent prompt.
    StoreInstallResult(widget_store::StoreInstallOutcome),
    /// "Install anyway" on the consent prompt — retry the parked
    /// bundle with `force`.
    StoreConsentAccept,
    /// Dismiss the consent prompt without installing.
    StoreConsentCancel,
    /// Two-step uninstall: first click arms the row's confirm,
    /// second click on the same id removes the widget directory
    /// (settings bags kept, spec §9) and rescans.
    StoreUninstall(String),

    // --- Widget options card (spec §5/§6/§10d) ---
    /// Scope toggle at the top of the options card. → Global just
    /// flips the pointer (instance bag kept, ignored); → Instance
    /// seeds the instance bag as a copy of the current resolved
    /// values so it diverges from there (spec §6 table).
    SetWidgetScope(usize, oxidemx_shared::WidgetScope),
    /// One option edit from any card control. Writes to the bag the
    /// slice's current scope selects: Global → `widgets.global[id]`,
    /// Instance → `widgets.instances[ikey][id]`.
    SetWidgetOption {
        slice: usize,
        key: String,
        value: serde_json::Value,
    },
    /// Per-option "↺": Instance scope drops the instance override
    /// ("Reset to global"), Global drops the global value ("Reset
    /// to default").
    ResetWidgetOption {
        slice: usize,
        key: String,
    },
    /// Live text of a location option's geocoder search field. Also
    /// claims the shared search state for that (slice, option key).
    WidgetLocQuery {
        slice: usize,
        key: String,
        text: String,
    },
    /// Kick off the Open-Meteo lookup for the current query.
    WidgetLocSearch {
        slice: usize,
        key: String,
    },
    /// Geocoder results (or error) for the in-flight search.
    WidgetLocResults(Result<Vec<geocode::GeoHit>, String>),
    /// Pick one geocoder hit → stores `{"name", "lat", "lon"}`
    /// through the same scoped write path as `SetWidgetOption`.
    WidgetLocPick {
        slice: usize,
        key: String,
        name: String,
        lat: f64,
        lon: f64,
    },
    /// Event from the options-card live-preview worker (Task 5):
    /// fresh scenes + instance failures, streamed via `Task::run`.
    WidgetPreviewEvent(oxidemx_widget_host::HostEvent),

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
    /// Toggle horizontal-axis scroll inversion (ThumbWheel HID++
    /// 0x2150). Independent of the main wheel's `natural` flag —
    /// flips just the side scroll.
    SetScrollHorizontalInvert(bool),
    SetScrollSmooth(bool),
    SetScrollSmartshift(bool),
    SetScrollSmartshiftThreshold(u32),
    SetScrollMode(String),

    // --- Mouse-button assignments (Buttons tab) ---
    SetButtonAssignment(oxidemx_shared::MouseButton, oxidemx_shared::ButtonAction),

    // --- Battery (UPower poll) ---
    /// Periodic tick — kicks off a UPower probe.
    BatteryTick,
    /// Probe finished; latest reading.
    BatteryUpdate(Option<battery::BatteryStatus>),

    // --- Macros tab ---
    RefreshMacros,
    OpenMacrosFolder,
    DeleteMacro(String),
    /// Export the named macro to a user-chosen file. Useful for
    /// sharing single macros without exporting the whole config
    /// bundle.
    ExportMacro(String),
    /// Result of `ExportMacro` — `Ok(path)` for status feedback,
    /// `Err(msg)` on cancellation or write failure.
    MacroExported(Result<String, String>),
    /// Import one macro JSON from disk. File stem becomes the
    /// new macro id, body is written verbatim under
    /// `~/.config/oxidemx/macros/{stem}.json`. Existing macros
    /// with the same id are overwritten.
    ImportMacro,
    /// Result of `ImportMacro` — `Ok(id)` (so we can refresh +
    /// surface the new entry), `Err(msg)` on read/parse failure.
    MacroImported(Result<String, String>),

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
    EditMacroField {
        id: String,
        field: MacroEditField,
        value: String,
    },
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

    // --- Gaming → haptic-redirect bridge ---
    // Phase-2 UI: write to config.gaming.haptic_redirect.*; daemon
    // reads via inotify. No D-Bus apply path yet (Phase 3+ will wire
    // the daemon).
    SetHapticRedirectEnabled(bool),
    SetHapticRedirectMode(HapticRedirectMode),
    SetHapticRedirectCurve(HapticRedirectCurve),
    SetHapticRedirectEventMode(HapticEventMode),
    SetHapticRedirectIntensityScale(f32),
    SetHapticRedirectMinIntensity(f32),
    SetHapticRedirectStrongWeight(f32),
    SetHapticRedirectWeakWeight(f32),
    SetHapticRedirectThrottleMs(u16),
    SetHapticRedirectPassthroughToPad(bool),
    SetHapticRedirectHardHide(bool),
    /// Period (s) for the "keep gamepad awake" pulse; 0 = disabled.
    SetHapticRedirectKeepGamepadActive(u16),
    /// Fire a one-shot test pulse through the daemon.
    TestHapticRedirect,
    HapticRedirectTested(Result<(), String>),
    /// Request a diagnostic report from the daemon.
    DiagnoseHapticRedirect,
    HapticRedirectDiagnosed(Result<String, String>),

    // --- HiResScroll (Point & Scroll tab) ---
    SetHiResScrollHires(bool),
    SetHiResScrollInvert(bool),
    SetHiResScrollTarget(bool),
    HiResScrollSet(Result<(), String>),

    // --- Per-app profile bindings (Settings tab) ---
    /// User typed in the WIP "app class" or "profile name" fields.
    SetAppBindingDraft {
        class: String,
        profile: String,
    },
    /// Save the current draft as a new app→profile entry.
    AddAppBinding,
    /// Remove the binding for `class`.
    RemoveAppBinding(String),

    // --- Custom theme palette editor (Settings tab → Theme card) ---
    /// Set the wedge count for a specific radial page. Clamped to
    /// 2..=8 by the overlay's `RadialPage::effective_slot_count`.
    SetPageSlotCount {
        page: usize,
        count: u8,
    },
    /// Toggle the "Customise theme" expander.
    ToggleThemeCustomiser,
    /// Remove a user-saved theme from disk. Bundled themes can't
    /// be deleted (the slug just won't match anything), but we
    /// still cover that no-op silently. If the user is currently
    /// on the deleted theme, fall back to the default so the UI
    /// doesn't render with a stale palette reference.
    DeleteUserTheme(String),
    /// Restore every palette field to the snapshot taken when the
    /// customiser was opened. No-op if no editor is active. Doesn't
    /// touch the typed slug — the user might want to keep their
    /// "Save as" name across a revert.
    RevertCustomTheme,
    /// Edit one palette field. Field name is one of the
    /// ThemeColors keys ("crust", "accent", etc.); value is the
    /// new "#rrggbb" hex.
    SetThemeColor {
        field: String,
        value: String,
    },
    /// Open the inline color-picker for a named palette field, or
    /// close it when the same field is already open. Clicking a
    /// row's swatch toggles it.
    ToggleThemeColorPicker(String),
    /// Drag any of the R/G/B sliders in the open color-picker
    /// panel — recomputes the field's hex string and forwards to
    /// `SetThemeColor` so the live-preview path stays single-source.
    SetThemeColorChannel {
        field: String,
        channel: ColorChannel,
        value: u8,
    },
    /// Drag in the HSV square — sets saturation + value for the
    /// editing field while keeping the current hue. Hue updates
    /// flow through `SetThemeColorHue` to keep the message shape
    /// flat (saves us threading an `enum` of {SV, H, RGB}).
    SetThemeColorSv {
        field: String,
        s: f32,
        v: f32,
    },
    /// Drag in the rainbow hue strip. SV are preserved — when the
    /// current colour is greyscale (s == 0), saturation jumps to
    /// 1.0 and value preserves so the user actually *sees* their
    /// new hue land.
    SetThemeColorHue {
        field: String,
        h: f32,
    },
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
    SetPageName {
        page: usize,
        name: String,
    },
    /// Update the comma-separated app-classes list for `page`.
    /// Empty string clears the list (turns the page into a global
    /// page); non-empty makes it an app-context page.
    SetPageAppClasses {
        page: usize,
        value: String,
    },
    /// Toggle whether an app-context page also participates in the
    /// scroll-wheel cycle.
    SetPageIncludeInScroll {
        page: usize,
        value: bool,
    },
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
    DetectedFocusedClass {
        page: usize,
        generation: u64,
        class: Option<String>,
    },
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
    // --- Custom animation editor (full panel) ---
    /// Open the animation editor on a specific element. Mirrors
    /// the "Customize" button on each element's row in the
    /// Animation tab.
    OpenAnimationEditor(animation_editor::AnimEditorElement),
    /// Close the editor — Back button.
    CloseAnimationEditor,
    /// Select a track on either side for parameter editing.
    AnimationEditorSelectTrack(animation_editor::AnimEditorDirection, usize),
    /// Append a new track to the given direction. Track type
    /// name comes from the type combobox; the new track gets
    /// `TrackKind::default_for(name)` + sensible defaults for
    /// delay (0) / duration (250 ms) / easing (EaseOut).
    AnimationEditorAddTrack(animation_editor::AnimEditorDirection, &'static str),
    /// Remove a track by index. Adjusts the selected track if
    /// the deleted one was selected (or shifted by the removal).
    AnimationEditorDeleteTrack(animation_editor::AnimEditorDirection, usize),
    /// Replace a track's `kind` while preserving timing + easing.
    /// Used by the type picker in the parameter editor.
    AnimationEditorChangeKind(animation_editor::AnimEditorDirection, usize, &'static str),
    /// Mutate a single field of the selected track. The
    /// `TrackParam` enum collapses ~8 different setters into one
    /// message so the update handler stays compact.
    AnimationEditorSetParam(
        animation_editor::AnimEditorDirection,
        usize,
        animation_editor::TrackParam,
    ),
    /// Switch the easing variant on a track. Spring keeps any
    /// existing stiffness/damping; non-Spring variants ignore
    /// those (they're ineffective on Linear / EaseIn / EaseOut /
    /// EaseInOut anyway).
    AnimationEditorSetEasingKind(
        animation_editor::AnimEditorDirection,
        usize,
        animation_editor::EasingPickOption,
    ),
    /// Reset both Enter and Exit `custom_tracks` for the active
    /// element to the empty list — falls back to the preset.
    AnimationEditorReset,
    SetAppCommandSearch(String),
    /// Toggle whether the app pick replaces the slice's icon
    /// (defaults to true). Off = command + label only, leave the
    /// existing icon + colour mode alone.
    SetAppCommandReplaceIcon(bool),
    /// User clicked an app in the list. Carries the cleaned exec
    /// line (placeholder %F/%U/etc. stripped), icon string, and
    /// display name so the handler doesn't need to look back into
    /// the apps cache.
    PickAppForCommand {
        command: String,
        icon: String,
        label: String,
    },
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
    IconFileChosen {
        target: icon_picker::IconPickerTarget,
        path: Option<String>,
    },
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
    IconsPrewarmed(Vec<(String, Option<oxidemx_icons::RasterIcon>)>),
    /// Result of the untinted (apps-source) prewarm pass — same
    /// shape as `IconsPrewarmed` but the install path uses the
    /// `peek/install_icon_handle_untinted` cache key (color = 0)
    /// so app icons retain their brand colours in the picker.
    AppIconsPrewarmed(Vec<(String, Option<oxidemx_icons::RasterIcon>)>),

    // --- Submenu sub-items (slice editor) ---
    AddSubItem(usize),
    DeleteSubItem(usize, usize),
    SetSubItemLabel(usize, usize, String),
    SetSubItemCommand(usize, usize, String),
    SetSubItemColor(usize, usize, String),
    /// Icon name / path for a submenu sub-item.
    SetSubItemIcon(usize, usize, String),
    /// Action kind (Exec / Macro / EasySwitch / etc.) for a sub-item.
    SetSubItemKind(usize, usize, oxidemx_shared::ActionKind),
    MoveSubItemUp(usize, usize),
    MoveSubItemDown(usize, usize),

    // ============================================================================
    // Indicator Popup
    // ============================================================================
    SetPopupMode(oxidemx_shared::PopupMode),
    SetPopupShowHostButtons(bool),
    SetPopupHostLabelStyle(oxidemx_shared::HostLabelStyle),
    PopupToggleMoveUp(String),
    PopupToggleMoveDown(String),
    PopupToggleRemove(String),
    PopupToggleAdd(String),
    PopupSliderMoveUp(String),
    PopupSliderMoveDown(String),
    PopupSliderRemove(String),
    PopupSliderAdd(String),
    SetPopupVolumeOnScroll(bool),
    SetPopupCloseOnAction(bool),
    SetPopupAnimations(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualField {
    MenuBackgroundOpacity,
    SliceHighlightOpacity,
    /// Centre-label font size in px (Visuals tab → "Centre label size").
    CenterLabelSize,
    /// Arced-tooltip font size in px. 0 hides the tooltip.
    TooltipFontSize,
    /// Aurora backdrop intensity (0..=1). 0 disables the
    /// shader entirely.
    AuroraIntensity,
    /// Haptic ripple shader intensity (0..=1). 0 disables.
    RippleIntensity,
    /// SDF hover glow shader intensity (0..=1). 0 disables.
    HoverGlowIntensity,
    /// Dispatch-burst shader intensity (0..=1). 0 disables.
    DispatchBurstIntensity,
    /// SDF wedge-ring shader intensity (0..=1). 0 disables —
    /// canvas-only wedge rendering. > 0 fades canvas wedge fills
    /// out and lets the SDF layer paint them instead.
    SdfRingIntensity,
    /// Parallax-tilt shader intensity (0..=1). 0 disables.
    HoverTiltIntensity,
    /// Parallax-tilt shadow strength (0..=1). How much the side
    /// of the wedge facing AWAY from the cursor darkens.
    HoverTiltShadow,
    /// Parallax-tilt specular sharpness (0..=1). Higher =
    /// smaller, sharper highlight; lower = broader wash.
    HoverTiltSharpness,
    /// Disc-bevel rim/inset shader intensity (0..=1). 0 disables.
    DiscBevelIntensity,
    /// Centre-dome Phong-sphere shader intensity (0..=1).
    /// 0 disables.
    CenterDomeIntensity,
    /// Slice-bevel shader intensity (0..=1). 0 disables.
    SliceBevelIntensity,
    /// Drop-shadow shader intensity (0..=1). 0 disables.
    DropShadowIntensity,
    /// Global light direction in radians (canvas convention).
    /// Drives all 3D-framing shaders' `light_angle` uniform so
    /// highlights/shadows stay coherent across the disc.
    LightAngleRad,
    /// Specular-sweep shader intensity (0..=1). 0 disables.
    SpecularSweepIntensity,
    /// Specular-sweep revolution period in seconds.
    SpecularSweepPeriod,
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
    AiMorph,
}

impl AnimElement {
    pub fn label(&self) -> &'static str {
        match self {
            AnimElement::Menu => "Menu",
            AnimElement::Submenu => "Submenu",
            AnimElement::SliceHighlight => "Slice highlight",
            AnimElement::AiMorph => "AI chat morph",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            AnimElement::Menu => "The whole radial wheel — open + close.",
            AnimElement::Submenu => "Sub-item arc that pops out when hovering a submenu slice.",
            AnimElement::SliceHighlight => {
                "Per-slice hover glow — fades in when the cursor enters a slice."
            }
            AnimElement::AiMorph => {
                "Disc → AI chat transform — the wheel splits into the arc shell \
                 and the conversation fades in. Duration + easing control the \
                 whole timeline."
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
            AnimElement::AiMorph => &anim.ai_morph,
        }
    }

    pub fn get_mut<'a>(&self, anim: &'a mut AnimationConfig) -> &'a mut ElementAnimation {
        match self {
            AnimElement::Menu => &mut anim.menu,
            AnimElement::Submenu => &mut anim.submenu,
            AnimElement::SliceHighlight => &mut anim.slice_highlight,
            AnimElement::AiMorph => &mut anim.ai_morph,
        }
    }

    pub fn default_for(&self) -> ElementAnimation {
        match self {
            AnimElement::Menu => ElementAnimation::menu_default(),
            AnimElement::Submenu => ElementAnimation::submenu_default(),
            AnimElement::SliceHighlight => ElementAnimation::slice_highlight_default(),
            AnimElement::AiMorph => ElementAnimation::ai_morph_default(),
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
    /// Snapshot of `status` taken on the last SaveTick. Used to
    /// detect "the status changed since the last tick" without
    /// touching every call site that writes to `status` — the
    /// auto-clear timer reads `status` against `status_seen` and
    /// stamps `status_set_at` on a fresh string.
    pub status_seen: String,
    /// When the current `status` was first observed by the auto-
    /// clear logic. Reset every time `status_seen != status`.
    /// Cleared on tick once the message is older than
    /// [`STATUS_LIFETIME`].
    pub status_set_at: Option<std::time::Instant>,
    /// `Some(when)` while the user has clicked "Reset all to
    /// defaults" once and hasn't yet confirmed. Second click
    /// within [`RESET_CONFIRM_WINDOW`] actually performs the
    /// reset; click after the window times out arms again instead
    /// of acting. `None` = no reset pending.
    pub reset_armed_at: Option<std::time::Instant>,

    /// Weather-location geocoder (Settings tab): live query text,
    /// the last search's results, and an in-flight flag.
    pub weather_query: String,
    pub weather_results: Vec<geocode::GeoHit>,
    pub weather_searching: bool,
    /// Currently-selected slot in the radial preview, if any.
    /// Drives the per-slice editor in the Buttons-tab right column.
    pub selected_slice: Option<usize>,
    /// Latest battery reading. Sourced from the daemon when
    /// available (HID++ — instant + accurate charging state)
    /// with UPower as a fallback. The daemon is authoritative;
    /// UPower only fills in when the daemon snapshot is stale
    /// (older than `BATTERY_DAEMON_FRESH_SECS`).
    pub battery: Option<battery::BatteryStatus>,
    /// Wall-clock instant of the most recent daemon-sourced
    /// battery update. UPower writes are gated on this so a
    /// subsequent UPower poll doesn't clobber the daemon's
    /// charging=true state with `charging=false` (UPower's
    /// reporting on Bluetooth/HID is often wrong, ours via
    /// HID++ is canonical).
    pub battery_daemon_at: Option<std::time::Instant>,
    /// Cached list of macros in `~/.config/oxidemx/macros/`.
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
    /// Open inline colour picker in the AI-window-effects card:
    /// `(status index, palette slot)`.
    pub ai_fx_editing: Option<(usize, usize)>,
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
    /// and a Cancel button while waiting; the deferred Task::perform
    /// races independently. `None` = no detect in flight.
    pub detect_in_flight: Option<DetectInFlight>,
    /// Visual icon-picker dialog state. `Some` while the picker
    /// is open against a slice or sub-item; `None` when closed.
    /// Picker `target` is set when opening and consumed when an
    /// icon is clicked.
    pub icon_picker: Option<icon_picker::IconPickerState>,
    /// Most-recently-used icon names. Front of the list is the
    /// last-picked icon. Persisted to
    /// `~/.config/oxidemx/recent-icons.json` after each pick;
    /// surfaces as a row at the top of the icon picker so common
    /// choices are one click away.
    pub recent_icons: Vec<String>,
    /// Snapshot of installed `.desktop` apps + their declared
    /// `Icon=` fields. Used by the picker's "Apps" source so the
    /// user can pick an icon by app rather than by symbolic name.
    /// Loaded once at startup; static enough to skip live
    /// reloading.
    pub installed_apps: Vec<oxidemx_shared::DesktopEntry>,
    /// In-flight app picker for the slice command field. `Some`
    /// while the inline list is open; `None` when closed. Picker
    /// fills the slice's command + icon + label-if-empty + flips
    /// to "Full colour" mode in one click.
    pub app_command_picker: Option<app_picker::AppCommandPickerState>,
    /// Custom-animation editor full-panel state, if open. Set
    /// by Message::OpenAnimationEditor (clicked from a
    /// "Customize" button on the Animation tab); cleared by
    /// Message::CloseAnimationEditor (Back arrow). When `Some`,
    /// the main view swaps to the editor panel and hides the
    /// normal tab content.
    pub animation_editor: Option<animation_editor::AnimationEditorState>,
    /// Active shortcut-capture target, if the user clicked
    /// "Capture" on a Shortcut-kind slice/sub-item editor. Drives
    /// the keyboard subscription gate and the Capture button's
    /// label flip ("Capture" → "Press a chord… (Esc to cancel)").
    pub capturing_shortcut: Option<ShortcutCaptureTarget>,
    /// Slug of the saved user theme currently being renamed, plus
    /// the in-flight draft string. `None` = no rename open.
    pub renaming_theme: Option<RenameThemeDraft>,
    /// Searchable font-family picker state. Holds the deduplicated
    /// list of system families discovered via `fc-list` once at
    /// boot; the combo_box widget filters in place as the user
    /// types.
    pub font_picker: iced::widget::combo_box::State<FontChoice>,
    /// Shared rasterised icon cache (XDG resolver + tinting). Lives
    /// at State level so it persists across re-renders and across
    /// theme changes (colours change → new cache entries; old
    /// entries stay for free).
    pub icons: std::rc::Rc<oxidemx_icons::IconCache>,
    /// Iced-Handle cache layered on top — saves the GPU-upload step
    /// every render, keyed by (source, size, colour).
    pub iced_handles: std::rc::Rc<
        std::cell::RefCell<
            std::collections::HashMap<radial_preview::IconKey, iced::widget::image::Handle>,
        >,
    >,
    /// Latest gamepad-rumble → haptic diagnostic report, shown in
    /// the Gaming tab after the user clicks "Diagnose". `None` until
    /// the first diagnose; refreshed on each subsequent click.
    pub haptic_diagnosis: Option<String>,
    /// In-flight Gemini API key text (Settings tab → AI Assistant).
    /// Write-only: the stored key is never loaded back into the
    /// field, only a "configured" indicator is shown.
    pub ai_key_draft: String,
    /// AI tab allowlist add-form draft (not persisted).
    pub ai_allowlist_draft: String,
    /// Whether `~/.config/oxidemx/gemini.key` exists. Checked at
    /// boot and updated on save/remove.
    pub ai_key_present: bool,
    /// Settings-side mirror of the installed-widget registry
    /// (`~/.config/oxidemx/widgets`). Scanned once at startup +
    /// re-scanned on `Message::RescanWidgets` after store actions.
    pub widget_registry: Vec<tabs::buttons::picker::WidgetSummaryLite>,
    /// Slice index whose behavior picker panel is expanded, if any.
    pub picker_open: Option<usize>,
    /// Live query of the picker's search field.
    pub picker_search: String,
    /// Undo-by-reselect snapshot: the slice as it was before the
    /// picker session started. Re-picking the tile matching this
    /// snapshot's behavior restores it wholesale (so a widget's
    /// instance settings survive a round-trip). Kept across the
    /// pick-applies-and-collapses step; GC'd on explicit Cancel,
    /// slice deselect/reselect, and slice list mutations.
    pub picker_undo: Option<(usize, oxidemx_shared::Slice)>,
    /// Full manifests per installed widget id — the options card
    /// renders from `manifest.options`, which `WidgetSummaryLite`
    /// intentionally omits. Refreshed together with
    /// `widget_registry` (same scan).
    pub widget_manifests: std::collections::HashMap<String, oxidemx_widget_proto::WidgetManifest>,
    /// Which (slice idx, option key) owns the location-option
    /// geocoder state below. One search at a time — the card only
    /// shows the query/results on the control that claimed it.
    pub widget_loc_target: Option<(usize, String)>,
    /// Live query text of the active location option's search field.
    pub widget_loc_query: String,
    /// Results of the last location-option search.
    pub widget_loc_results: Vec<geocode::GeoHit>,
    /// In-flight flag for the location-option geocoder.
    pub widget_loc_searching: bool,
    /// Widget store / downloader dialog (spec §11). `Some` while
    /// open — takes over the content area via the same full-panel
    /// chrome as the icon picker; dropped wholesale on close.
    pub widget_store: Option<widget_store::WidgetStoreState>,
    /// Live wedge preview worker for the open widget options card
    /// (Plan 3 Task 5). Reconciled by `widget_preview::sync` after
    /// every update; `None` whenever the card isn't showing.
    pub widget_preview: Option<widget_preview::PreviewHandle>,
    /// Last known window width (logical px), fed by the resize
    /// subscription. Drives the picker grid's 3-up / 4-up split —
    /// iced's `responsive` wrapper doesn't measure correctly inside
    /// the page scrollable (infinite height), so we track the
    /// window instead.
    pub window_width: f32,
    /// Agents tab data — flows (with validation), roster agents, and
    /// MCP servers. Loaded at boot + on refresh (not in Default, which
    /// must stay IO-free).
    pub agents: tabs::agents::AgentsData,
    /// Agents tab: the "new flow" name field + last create result.
    pub agents_new_flow_draft: String,
    pub agents_new_flow_status: String,
    /// Agents tab: the open in-GUI flow.md editor, if any.
    pub agents_flow_editor: Option<tabs::agents::FlowEditor>,
}

/// Where the AI Assistant's Gemini API key lives. Mirrors the
/// lookup in `overlay-rs/src/ai_client.rs::load_api_key` (which
/// also honours `GEMINI_API_KEY` and the legacy juhradial path —
/// this app only manages the canonical file).
/// Key file for a provider (`~/.config/oxidemx/<stem>.key`), or
/// `None` for keyless providers (Ollama, Claude Code).
pub fn ai_key_path_for(provider: oxidemx_shared::config::AiProvider) -> Option<std::path::PathBuf> {
    let stem = provider.key_file_stem()?;
    let home = std::env::var("HOME").unwrap_or_default();
    Some(std::path::Path::new(&home).join(format!(".config/oxidemx/{stem}.key")))
}

/// Whether the given provider has a key on disk (or no key needed).
pub fn ai_key_present_for(provider: oxidemx_shared::config::AiProvider) -> bool {
    match ai_key_path_for(provider) {
        Some(p) => p.exists(),
        None => true, // keyless providers are always "ready"
    }
}

impl Default for State {
    fn default() -> Self {
        let path = oxidemx_shared::config::default_config_path();
        let mut config = path
            .as_ref()
            .and_then(|p| AppConfig::load_from(p).ok())
            .unwrap_or_default();
        // `unwrap_or_default()` skips the loader's normalize step,
        // so make sure the multi-page invariant holds (>=1 page)
        // before any slice-editor message can mutate state.
        config.radial_menu.normalize_pages();
        let ai_key_present = ai_key_present_for(config.overlay.ai.provider);
        let pal = palette::Palette::resolve(&config.theme);
        // Restore the last-visited tab from disk if the user has
        // one saved. Falls back to Buttons (the home tab) when
        // there's no record or it points at an unknown tag.
        // `OXIDEMX_SETTINGS_TAB` (set by the overlay's chat action icons)
        // forces a starting tab; otherwise restore the last-used one.
        let initial_tab = std::env::var("OXIDEMX_SETTINGS_TAB")
            .ok()
            .and_then(|t| Tab::from_tag(&t))
            .or_else(|| ui_state::load_last_tab().as_deref().and_then(Tab::from_tag))
            .unwrap_or(Tab::MouseButtons);
        let (widget_registry, widget_manifests) = tabs::buttons::picker::scan_registry_full();
        State {
            config,
            palette: pal,
            tab: initial_tab,
            config_path: path,
            last_edit: None,
            saved_pending: false,
            status: String::new(),
            status_seen: String::new(),
            status_set_at: None,
            reset_armed_at: None,
            weather_query: String::new(),
            weather_results: Vec::new(),
            weather_searching: false,
            selected_slice: None,
            icons: std::rc::Rc::new(oxidemx_icons::IconCache::new()),
            iced_handles: std::rc::Rc::new(std::cell::RefCell::new(
                std::collections::HashMap::new(),
            )),
            battery: None,
            battery_daemon_at: None,
            macros: tabs::macros::list(),
            daemon: daemon::DaemonSnapshot::default(),
            recording: RecordingState::Idle,
            theme_editor: None,
            ai_fx_editing: None,
            macro_edit: None,
            app_binding_draft: AppBindingDraft::default(),
            active_page: 0,
            app_classes_drafts: std::collections::BTreeMap::new(),
            detect_in_flight: None,
            icon_picker: None,
            recent_icons: recents::load(),
            installed_apps: oxidemx_shared::enumerate_applications(),
            app_command_picker: None,
            animation_editor: None,
            capturing_shortcut: None,
            renaming_theme: None,
            font_picker: iced::widget::combo_box::State::new(
                std::iter::once(FontChoice::default())
                    .chain(
                        fonts::system_families()
                            .iter()
                            .cloned()
                            .map(FontChoice::Family),
                    )
                    .collect(),
            ),
            haptic_diagnosis: None,
            ai_key_draft: String::new(),
            ai_allowlist_draft: String::new(),
            ai_key_present,
            widget_registry,
            picker_open: None,
            picker_search: String::new(),
            picker_undo: None,
            widget_manifests,
            widget_loc_target: None,
            widget_loc_query: String::new(),
            widget_loc_results: Vec::new(),
            widget_loc_searching: false,
            widget_store: None,
            widget_preview: None,
            window_width: INITIAL_WINDOW_SIZE.width,
            agents: tabs::agents::AgentsData::default(),
            agents_new_flow_draft: String::new(),
            agents_new_flow_status: String::new(),
            agents_flow_editor: None,
        }
    }
}

/// Startup window size — shared between the `iced::window::Settings`
/// in `main()` and the `State::window_width` seed so the picker grid
/// renders at the right density before the first resize event.
const INITIAL_WINDOW_SIZE: iced::Size = iced::Size::new(1280.0, 820.0);

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

/// Single RGB channel selector for the inline colour picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChannel {
    Red,
    Green,
    Blue,
}

/// Where a captured key chord should be written when the user
/// finishes a shortcut-capture session. Mirrors the shape of the
/// icon-picker's target enum so the same dispatch pattern works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutCaptureTarget {
    Slice(usize),
    SubItem { parent: usize, idx: usize },
}

/// In-flight rename of a saved user theme. `original` is the slug
/// the user clicked Rename on; `draft` is the new slug they're
/// typing. Committing saves the working theme under `draft`,
/// deletes the file at `original`, and (if the renamed theme
/// was active) updates `state.config.theme`.
#[derive(Debug, Clone)]
pub struct RenameThemeDraft {
    pub original: String,
    pub draft: String,
}

/// One entry in the font-family picker. `Default` sentinel covers
/// the "let the system decide" option (config field stored as ""),
/// `Family(name)` is one installed family. Display impl drives the
/// combo_box's filtering + on-screen text, and PartialEq is used by
/// iced to look up the currently-selected entry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FontChoice {
    #[default]
    Default,
    Family(String),
}

impl FontChoice {
    /// Resolve to the on-disk config representation — empty string
    /// for the system default, family name otherwise.
    pub fn as_config_value(&self) -> String {
        match self {
            FontChoice::Default => String::new(),
            FontChoice::Family(s) => s.clone(),
        }
    }

    /// Inverse of `as_config_value` — picks the matching enum
    /// variant from a stored config string.
    pub fn from_config_value(s: &str) -> Self {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            FontChoice::Default
        } else {
            FontChoice::Family(trimmed.to_string())
        }
    }
}

impl std::fmt::Display for FontChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontChoice::Default => f.write_str("(System default)"),
            FontChoice::Family(s) => f.write_str(s),
        }
    }
}

/// In-flight state of the custom-theme editor.
#[derive(Debug, Clone)]
pub struct ThemeEditor {
    /// Working palette — starts as a clone of the active theme's
    /// colours and accumulates the user's edits. Saved on click.
    pub working: oxidemx_shared::theme::ThemeColors,
    /// Pristine snapshot taken when the editor was opened. Used by
    /// the Revert button to restore the user's edits to where they
    /// started without having to close and re-open the customiser
    /// (which would also drop their typed slug and any other UI
    /// state). Never mutated.
    pub original: oxidemx_shared::theme::ThemeColors,
    /// is_dark flag for the working theme. Mirrors the theme this
    /// was forked from.
    pub is_dark: bool,
    /// Slug typed into the "Save as" input. Becomes both the
    /// filename and the picker entry on save.
    pub slug: String,
    /// Which palette field's color picker is currently expanded,
    /// or `None` when no row is being edited. Click a swatch in
    /// the editor → set this; click the swatch again or press
    /// Done → clear it.
    pub editing_field: Option<String>,
}

/// Apply a `#rrggbb` (or any string the user typed) to the named
/// field on a `ThemeColors`. Unknown field name → no-op. Used by
/// the custom-palette editor to thread one Message back into the
/// working struct.
fn set_theme_color_field(c: &mut oxidemx_shared::theme::ThemeColors, field: &str, value: String) {
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

/// Read a named palette field as its current hex string. Returns
/// the empty string for unknown fields. Used by the inline colour
/// picker to seed its R/G/B sliders from the existing value.
fn theme_field_value(c: &oxidemx_shared::theme::ThemeColors, field: &str) -> String {
    match field {
        "crust" => c.crust.clone(),
        "mantle" => c.mantle.clone(),
        "base" => c.base.clone(),
        "surface0" => c.surface0.clone(),
        "surface1" => c.surface1.clone(),
        "surface2" => c.surface2.clone(),
        "overlay0" => c.overlay0.clone(),
        "overlay1" => c.overlay1.clone(),
        "text" => c.text.clone(),
        "subtext1" => c.subtext1.clone(),
        "subtext0" => c.subtext0.clone(),
        "accent" => c.accent.clone(),
        "accent2" => c.accent2.clone(),
        "accent_dim" => c.accent_dim.clone(),
        "green" => c.green.clone(),
        "yellow" => c.yellow.clone(),
        "red" => c.red.clone(),
        "blue" => c.blue.clone(),
        "mauve" => c.mauve.clone(),
        "pink" => c.pink.clone(),
        "peach" => c.peach.clone(),
        "teal" => c.teal.clone(),
        "sapphire" => c.sapphire.clone(),
        "lavender" => c.lavender.clone(),
        _ => String::new(),
    }
}

/// Apply an HSV update to the editing field. `transform` receives
/// the field's current `(h, s, v)` (decoded from the stored hex)
/// and returns the new `(h, s, v)`. Used by both the SV-square
/// drag and the hue-strip drag so they go through one place.
fn apply_hsv_change<F>(state: &mut State, field: &str, transform: F)
where
    F: FnOnce(f32, f32, f32) -> (f32, f32, f32),
{
    let editor = match state.theme_editor.as_mut() {
        Some(e) => e,
        None => return,
    };
    let current = theme_field_value(&editor.working, field);
    let (r, g, b) = parse_hex_channels(&current);
    let (h, s, v) = color_canvas::rgb_to_hsv(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let (nh, ns, nv) = transform(h, s, v);
    let (nr, ng, nb) = color_canvas::hsv_to_rgb(nh, ns, nv);
    let hex = format!(
        "#{:02x}{:02x}{:02x}",
        (nr * 255.0).round().clamp(0.0, 255.0) as u8,
        (ng * 255.0).round().clamp(0.0, 255.0) as u8,
        (nb * 255.0).round().clamp(0.0, 255.0) as u8,
    );
    set_theme_color_field(&mut editor.working, field, hex);
    let preview = oxidemx_shared::theme::Theme {
        name: "(custom)".into(),
        description: String::new(),
        is_dark: editor.is_dark,
        radial_image: None,
        radial_params: None,
        colors: editor.working.clone(),
    };
    state.palette = palette::Palette::from_theme(&preview);
    state.touch();
}

/// Decode a `#rrggbb` (or `rrggbb`) hex into its three byte
/// channels. Unknown / malformed input returns black so the picker
/// always has a sane base to work from.
/// The active theme's `[accent, accent2, accent_dim]` hexes — the
/// fallback palette every AI-window effect starts from.
pub fn ai_fx_theme_hexes(state: &State) -> [String; 3] {
    oxidemx_shared::theme::Theme::load(&state.config.theme)
        .map(|t| {
            [
                t.colors.accent.clone(),
                t.colors.accent2.clone(),
                t.colors.accent_dim.clone(),
            ]
        })
        .unwrap_or_else(|| {
            [
                "#00d4ff".to_string(),
                "#b794ff".to_string(),
                "#1a6a80".to_string(),
            ]
        })
}

pub fn ai_fx_status_mut(state: &mut State, idx: usize) -> &mut oxidemx_shared::config::AiStatusFx {
    let fx = &mut state.config.radial_menu.visuals.ai_fx;
    match idx {
        0 => &mut fx.thinking,
        1 => &mut fx.awaiting,
        _ => &mut fx.idle,
    }
}

/// Effective hex for (status, slot): the custom override when set,
/// else the theme fallback.
pub fn ai_fx_effective_hex(state: &State, idx: usize, slot: usize) -> String {
    let fx = &state.config.radial_menu.visuals.ai_fx;
    let status = match idx {
        0 => &fx.thinking,
        1 => &fx.awaiting,
        _ => &fx.idle,
    };
    status
        .colors
        .as_ref()
        .map(|c| c[slot].clone())
        .filter(|c| oxidemx_shared::theme::parse_hex_rgba(c).is_some())
        .unwrap_or_else(|| ai_fx_theme_hexes(state)[slot].clone())
}

/// Read-modify-write one AI-FX palette colour through HSV space —
/// the SV-square / hue-strip handlers both funnel through here.
fn apply_ai_fx_hsv(
    state: &mut State,
    idx: usize,
    slot: usize,
    f: impl FnOnce(f32, f32, f32) -> (f32, f32, f32),
) {
    let current = ai_fx_effective_hex(state, idx, slot);
    let (r, g, b) = parse_hex_channels(&current);
    let (h, s, v) = color_canvas::rgb_to_hsv(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let (h, s, v) = f(h, s, v);
    let (nr, ng, nb) = color_canvas::hsv_to_rgb(h, s, v);
    let hex = format!(
        "#{:02x}{:02x}{:02x}",
        (nr * 255.0).round() as u8,
        (ng * 255.0).round() as u8,
        (nb * 255.0).round() as u8
    );
    let theme_hexes = ai_fx_theme_hexes(state);
    let fx = ai_fx_status_mut(state, idx);
    let mut colors = fx.colors.clone().unwrap_or(theme_hexes);
    colors[slot] = hex;
    fx.colors = Some(colors);
    state.touch();
}

pub fn parse_hex_channels(s: &str) -> (u8, u8, u8) {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return (0, 0, 0);
    }
    let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(0);
    (r, g, b)
}

/// Spawn a slice's Exec command via `sh -c` so the user can
/// validate it from the editor without going through the radial
/// menu. Mirrors the daemon's exec convention (see
/// `daemon/src/actions.rs::execute_command`) so what tests here
/// is what the daemon will run later. Status messages surface
/// failures; the spawn itself is non-blocking.
fn run_test_action(state: &mut State, slice: Option<&oxidemx_shared::Slice>) {
    use oxidemx_shared::ActionKind;
    let slice = match slice {
        Some(s) => s,
        None => {
            state.status = "Couldn't find slice to test.".into();
            return;
        }
    };
    let cmd = slice.command.trim();
    match slice.kind {
        ActionKind::Exec | ActionKind::Emoji => {
            if cmd.is_empty() {
                state.status = "Cannot test: command is empty.".into();
                return;
            }
            match std::process::Command::new("sh").args(["-c", cmd]).spawn() {
                Ok(child) => {
                    state.status = format!("Spawned (PID {}): {}", child.id(), cmd);
                }
                Err(e) => {
                    state.status = format!("Failed to spawn: {e}");
                }
            }
        }
        ActionKind::Settings => {
            // Test would just re-launch this very window. Skip the
            // spawn but tell the user what would happen.
            state.status = "Settings slice opens this window; already here.".into();
        }
        ActionKind::Macro => {
            if cmd.is_empty() {
                state.status = "Cannot test: macro id is empty.".into();
                return;
            }
            // Fire-and-forget through the existing daemon trigger
            // path. Status reflects the dispatch attempt; the
            // daemon's own logs cover playback success/failure.
            let id = cmd.to_string();
            tokio::spawn(daemon::trigger_macro(id.clone()));
            state.status = format!("Triggering macro \"{id}\" via daemon");
        }
        ActionKind::Shortcut => {
            if cmd.is_empty() {
                state.status = "Cannot test: key chord is empty.".into();
                return;
            }
            let keys = cmd.to_string();
            tokio::spawn(daemon::trigger_shortcut(keys.clone()));
            state.status = format!("Sending shortcut \"{keys}\" via daemon");
        }
        ActionKind::EasySwitch => match cmd.parse::<u8>() {
            Ok(idx) if (1..=3).contains(&idx) => {
                tokio::spawn(daemon::trigger_set_host(idx));
                state.status = format!("Switching to host {idx} via daemon");
            }
            _ => {
                state.status = format!("Easy-Switch host must be 1, 2, or 3 (got {cmd:?})");
            }
        },
        ActionKind::Submenu => {
            state.status = "Submenu slices have no action — test sub-items individually.".into();
        }
        ActionKind::Widget => {
            state.status =
                "Widget slices render live data in the overlay — nothing to test here.".into();
        }
        ActionKind::Dial => {
            state.status =
                "Dial slices adjust on scroll/drag in the overlay — nothing to test here.".into();
        }
        ActionKind::Power => {
            // Never actually fire a power action from the editor's
            // Test button — locking/suspending the session mid-edit
            // is hostile. Describe what it would do instead.
            state.status = format!(
                "Power slice would run \"{}\" — not fired from the editor.",
                if cmd.is_empty() { "(unset)" } else { cmd }
            );
        }
        ActionKind::NightLight => {
            state.status = "Night-light slice toggles GNOME night light in the overlay.".into();
        }
        ActionKind::MouseSetting => {
            state.status = format!(
                "Mouse-setting slice applies \"{}\" via the daemon — test from the overlay.",
                if cmd.is_empty() { "(unset)" } else { cmd }
            );
        }
        ActionKind::None => {
            state.status = "This slice has no action configured.".into();
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

/// Compute the effective wheel mode the device should be in,
/// given the current scroll config. The settings UI splits
/// "wheel mode picker" from "smartshift toggle"; the daemon
/// expects a single 3-state mode string. This helper is the
/// merge point.
fn effective_wheel_mode(scroll: &oxidemx_shared::ScrollConfig) -> &'static str {
    match scroll.mode.as_str() {
        // "free" and "freespin" both → freespin (Python used
        // "freespin", early Rust wrote "free"; daemon accepts
        // both).
        "freespin" | "free" => "freespin",
        "ratchet" => "ratchet",
        // The "smartshift" picker entry — only if the SmartShift
        // toggle is also on. With the toggle off, treat as plain
        // ratchet so the user gets the always-clicky behaviour
        // they asked for.
        _ => {
            if scroll.smartshift {
                "smartshift"
            } else {
                "ratchet"
            }
        }
    }
}

/// Fire the daemon's `set_wheel_mode` D-Bus call with the
/// effective mode + current threshold. Wired to every scroll-
/// mode picker / smartshift toggle / threshold slider change so
/// the device updates instantly without waiting for the debounced
/// config save + ReloadConfig round trip.
fn fire_wheel_mode_apply(state: &State) -> Task<Message> {
    let scroll = &state.config.scroll;
    let mode = effective_wheel_mode(scroll).to_string();
    let threshold = scroll.smartshift_threshold.min(100) as u8;
    Task::perform(daemon::set_wheel_mode(mode, threshold), |_| Message::Noop)
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
            radial_preview::Action::SwapSlices { from, to } => Message::SwapSlices { from, to },
        }
    }
}

/// How long a status message stays in the footer before being
/// auto-cleared. Long enough for the user to read a save-success
/// toast or a "Spawned (PID …)" line; short enough that stale
/// messages don't linger across unrelated edits. The last
/// `STATUS_FADE_TAIL` of this window is a smooth fade-out instead
/// of a hard cut, so the toast feels less abrupt.
const STATUS_LIFETIME: Duration = Duration::from_secs(5);
/// Tail of `STATUS_LIFETIME` over which the status text fades
/// from full alpha down to zero. Renderer reads `status_set_at`
/// and computes a per-frame alpha — the auto-clear logic still
/// drops the text once we're past `STATUS_LIFETIME`, but the
/// final stretch reads as a fade rather than a snap.
pub const STATUS_FADE_TAIL: Duration = Duration::from_millis(800);

/// Window the user has to confirm a destructive "Reset all to
/// defaults" click. First click arms; second click within this
/// window performs the reset; outside the window the button just
/// rearms instead of acting (treating a stale arming as a fresh
/// intent).
const RESET_CONFIRM_WINDOW: Duration = Duration::from_secs(3);

impl State {
    fn touch(&mut self) {
        self.last_edit = Some(Instant::now());
        self.saved_pending = true;
    }

    /// Collapse the behavior picker panel and drop the
    /// undo-by-reselect snapshot. Called on explicit Cancel, slice
    /// deselect/reselect, and any slice-list mutation that would
    /// leave the snapshot pointing at a stale index.
    fn reset_picker(&mut self) {
        self.picker_open = None;
        self.picker_search.clear();
        self.picker_undo = None;
        // The options card's location search is slice-addressed too —
        // a stale target would render under the wrong control after a
        // deselect/reorder, so it resets with the picker state.
        self.widget_loc_target = None;
        self.widget_loc_query.clear();
        self.widget_loc_results.clear();
        self.widget_loc_searching = false;
    }

    /// Editing context of the custom widget on slice `idx`:
    /// `(widget id, current scope, effective instance key)`.
    ///
    /// When the slice has no `instance_key` yet (hand-edited
    /// config), the canonical `<page-slug>.slot<N>` key is derived
    /// AND written back into the slice so this first edit and every
    /// later one land under a stable address. `None` for built-in
    /// widget sources and non-widget slices.
    fn widget_edit_ctx(
        &mut self,
        idx: usize,
    ) -> Option<(String, oxidemx_shared::WidgetScope, String)> {
        let page_name = self
            .config
            .radial_menu
            .pages
            .get(self.active_page)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let slice = self.active_slices_mut().get_mut(idx)?;
        let w = slice.widget.as_mut()?;
        let oxidemx_shared::WidgetSource::Custom(id) = &w.source else {
            return None;
        };
        let id = id.clone();
        let (ikey, derived) = match w.instance_key.as_deref() {
            Some(k) if !k.is_empty() => (k.to_string(), false),
            _ => {
                let k = oxidemx_shared::widgets::instance_key(&page_name, idx);
                w.instance_key = Some(k.clone());
                (k, true)
            }
        };
        let scope = w.scope;
        if derived {
            // The write-back is a config mutation in its own right —
            // persist it even if the calling handler bails out.
            self.touch();
        }
        Some((id, scope, ikey))
    }

    /// Tick the status auto-clear timer. Picks up new strings on
    /// the same tick they're written without requiring every call
    /// site to stamp a timestamp; clears strings older than
    /// [`STATUS_LIFETIME`]. Idempotent + cheap (string compare +
    /// elapsed check).
    fn maybe_clear_status(&mut self) {
        if self.status != self.status_seen {
            self.status_seen = self.status.clone();
            self.status_set_at = if self.status.is_empty() {
                None
            } else {
                Some(Instant::now())
            };
            return;
        }
        if self.status.is_empty() {
            return;
        }
        if let Some(set_at) = self.status_set_at {
            if set_at.elapsed() >= STATUS_LIFETIME {
                self.status.clear();
                self.status_seen.clear();
                self.status_set_at = None;
            }
        }
    }

    /// Mutable access to the slice list of the currently-active
    /// page. Guarantees `pages` is non-empty + clamps
    /// `active_page` to a valid index — defensive against state
    /// arriving from a partially-migrated config.
    fn active_slices_mut(&mut self) -> &mut Vec<oxidemx_shared::Slice> {
        if self.config.radial_menu.pages.is_empty() {
            self.config
                .radial_menu
                .pages
                .push(oxidemx_shared::RadialPage::default());
        }
        let max = self.config.radial_menu.pages.len() - 1;
        if self.active_page > max {
            self.active_page = max;
        }
        &mut self.config.radial_menu.pages[self.active_page].slices
    }

    /// Read-only counterpart for view code. Returns an empty slice
    /// rather than panicking when the index is stale.
    fn active_slices(&self) -> &[oxidemx_shared::Slice] {
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

// `State::default()` does config IO; it can't be a literal, so the
// post-construction agents scan isn't a field-reassign smell.
#[allow(clippy::field_reassign_with_default)]
fn boot() -> (State, Task<Message>) {
    // Kick off both probes immediately so the indicators aren't
    // blank for the full poll interval after launch.
    let mut state = State::default();
    // Scan flows/roster/MCP once at startup (IO — kept out of Default).
    state.agents = tabs::agents::AgentsData::load();
    (
        state,
        Task::batch([
            Task::perform(battery::poll(), Message::BatteryUpdate),
            Task::perform(daemon::poll(), Message::DaemonSnapshotReceived),
        ]),
    )
}

/// Re-scan `~/.config/oxidemx/widgets` into the settings-side
/// caches (lite summaries + full manifests). Shared by the
/// `RescanWidgets` message and the store's install/uninstall
/// follow-ups.
fn rescan_widgets(state: &mut State) {
    let (registry, manifests) = tabs::buttons::picker::scan_registry_full();
    state.widget_registry = registry;
    state.widget_manifests = manifests;
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    let task = update_inner(state, message);
    // Reconcile the options-card live preview against whatever the
    // message just changed (selection, option edits, tab switches,
    // uninstalls…). Cheap when no custom-widget slice is selected.
    let preview = widget_preview::sync(state);
    Task::batch([task, preview])
}

fn update_inner(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::SwitchTab(t) => {
            state.tab = t;
            // Refresh tab-specific caches on entry. Cheap when the
            // tab doesn't need it.
            if t == Tab::Macros {
                state.macros = tabs::macros::list();
            }
            // Re-scan flows/roster/MCP each time the Agents tab opens
            // so externally-edited .md files show up without restart.
            if t == Tab::Agents {
                state.agents = tabs::agents::AgentsData::load();
            }
            // Persist the new tab fire-and-forget so re-opening
            // the settings window lands on the same surface.
            Task::perform(ui_state::save_last_tab(t.tag().to_string()), |_| {
                Message::LastTabPersisted
            })
        }
        Message::AgentsRefresh => {
            state.agents = tabs::agents::AgentsData::load();
            Task::none()
        }
        Message::AgentsRunFlow(id) => {
            tabs::agents::launch_mission_control(&id);
            Task::none()
        }
        Message::AgentsNewFlowDraft(s) => {
            state.agents_new_flow_draft = s;
            Task::none()
        }
        Message::AgentsCreateFlow => {
            match tabs::agents::scaffold_flow(&state.agents_new_flow_draft) {
                Ok(id) => {
                    state.agents_new_flow_draft.clear();
                    state.agents_new_flow_status =
                        format!("created `{id}` — edit its flow.md to refine");
                    state.agents = tabs::agents::AgentsData::load();
                    // Open the fresh flow in the editor right away.
                    state.agents_flow_editor = tabs::agents::FlowEditor::open(&id);
                }
                Err(e) => state.agents_new_flow_status = format!("✗ {e}"),
            }
            Task::none()
        }
        Message::AgentsEditFlow(id) => {
            state.agents_flow_editor = tabs::agents::FlowEditor::open(&id);
            Task::none()
        }
        Message::AgentsEditorAction(action) => {
            if let Some(ed) = &mut state.agents_flow_editor {
                let revalidate = action.is_edit();
                ed.content.perform(action);
                if revalidate {
                    ed.revalidate();
                }
            }
            Task::none()
        }
        Message::AgentsSaveFlow => {
            if let Some(ed) = &state.agents_flow_editor {
                match ed.save() {
                    Ok(()) => {
                        state.agents_new_flow_status = format!("saved `{}/flow.md`", ed.id);
                        state.agents = tabs::agents::AgentsData::load();
                    }
                    Err(e) => state.agents_new_flow_status = format!("✗ save failed: {e}"),
                }
            }
            Task::none()
        }
        Message::AgentsCloseEditor => {
            state.agents_flow_editor = None;
            Task::none()
        }
        Message::SetAiFxEffect(idx, slug) => {
            let fx = &mut state.config.radial_menu.visuals.ai_fx;
            let target = match idx {
                0 => &mut fx.thinking,
                1 => &mut fx.awaiting,
                _ => &mut fx.idle,
            };
            target.effect = slug;
            state.touch();
            Task::none()
        }
        Message::SetAiFxIntensity(idx, v) => {
            let fx = &mut state.config.radial_menu.visuals.ai_fx;
            let target = match idx {
                0 => &mut fx.thinking,
                1 => &mut fx.awaiting,
                _ => &mut fx.idle,
            };
            target.intensity = v.clamp(0.0, 1.0);
            state.touch();
            Task::none()
        }
        Message::SetAiFxSpeed(idx, v) => {
            let fx = &mut state.config.radial_menu.visuals.ai_fx;
            let target = match idx {
                0 => &mut fx.thinking,
                1 => &mut fx.awaiting,
                _ => &mut fx.idle,
            };
            target.speed = v.clamp(0.25, 3.0);
            state.touch();
            Task::none()
        }
        Message::ToggleAiFxColorPicker(idx, slot) => {
            state.ai_fx_editing = match state.ai_fx_editing {
                Some(cur) if cur == (idx, slot) => None,
                _ => Some((idx, slot)),
            };
            Task::none()
        }
        Message::SetAiFxColorSv(idx, slot, sat, val) => {
            apply_ai_fx_hsv(state, idx, slot, |h, _, _| (h, sat, val));
            Task::none()
        }
        Message::SetAiFxColorHue(idx, slot, h) => {
            apply_ai_fx_hsv(state, idx, slot, |_, s, v| {
                // Greyscale rescue — same trick as the theme
                // editor's hue strip: dragging hue on a black/white
                // colour would otherwise feel inert.
                let s = if s < 0.001 { 1.0 } else { s };
                let v = if v < 0.001 { 1.0 } else { v };
                (h, s, v)
            });
            Task::none()
        }
        Message::SetAiFxColorHex(idx, slot, hex) => {
            let theme_hexes = ai_fx_theme_hexes(state);
            let fx = ai_fx_status_mut(state, idx);
            let mut colors = fx.colors.clone().unwrap_or(theme_hexes);
            if let Some(c) = colors.get_mut(slot) {
                *c = hex;
            }
            fx.colors = Some(colors);
            state.touch();
            Task::none()
        }
        Message::ResetAiFxColors(idx) => {
            ai_fx_status_mut(state, idx).colors = None;
            state.ai_fx_editing = None;
            state.touch();
            Task::none()
        }
        Message::SetVisual(field, v) => {
            match field {
                VisualField::MenuBackgroundOpacity => {
                    state.config.radial_menu.visuals.menu_background_opacity = v.clamp(0.0, 1.0);
                }
                VisualField::SliceHighlightOpacity => {
                    state.config.radial_menu.visuals.slice_highlight_opacity = v.clamp(0.0, 1.0);
                }
                VisualField::CenterLabelSize => {
                    // 0 = disabled (the renderer skips drawing); cap
                    // at 32 so an accidental drag doesn't spawn
                    // ridiculous text.
                    state.config.radial_menu.visuals.center_label_size = v.clamp(0.0, 32.0);
                }
                VisualField::TooltipFontSize => {
                    state.config.radial_menu.visuals.tooltip_font_size = v.clamp(0.0, 24.0);
                }
                VisualField::AuroraIntensity => {
                    state.config.radial_menu.visuals.aurora_intensity = v.clamp(0.0, 1.0);
                }
                VisualField::RippleIntensity => {
                    state.config.radial_menu.visuals.ripple_intensity = v.clamp(0.0, 1.0);
                }
                VisualField::HoverGlowIntensity => {
                    state.config.radial_menu.visuals.hover_glow_intensity = v.clamp(0.0, 1.0);
                }
                VisualField::DispatchBurstIntensity => {
                    state.config.radial_menu.visuals.dispatch_burst_intensity = v.clamp(0.0, 1.0);
                }
                VisualField::SdfRingIntensity => {
                    state.config.radial_menu.visuals.sdf_ring_intensity = v.clamp(0.0, 1.0);
                }
                VisualField::HoverTiltIntensity => {
                    state.config.radial_menu.visuals.hover_tilt_intensity = v.clamp(0.0, 1.0);
                }
                VisualField::HoverTiltShadow => {
                    state.config.radial_menu.visuals.hover_tilt_shadow = v.clamp(0.0, 1.0);
                }
                VisualField::HoverTiltSharpness => {
                    state.config.radial_menu.visuals.hover_tilt_sharpness = v.clamp(0.0, 1.0);
                }
                VisualField::DiscBevelIntensity => {
                    state.config.radial_menu.visuals.disc_bevel_intensity = v.clamp(0.0, 1.0);
                }
                VisualField::CenterDomeIntensity => {
                    state.config.radial_menu.visuals.center_dome_intensity = v.clamp(0.0, 1.0);
                }
                VisualField::SliceBevelIntensity => {
                    state.config.radial_menu.visuals.slice_bevel_intensity = v.clamp(0.0, 1.0);
                }
                VisualField::DropShadowIntensity => {
                    state.config.radial_menu.visuals.drop_shadow_intensity = v.clamp(0.0, 1.0);
                }
                VisualField::LightAngleRad => {
                    // Wrap to (-π, π] so the value stays in a
                    // sane numeric range as the user spins the
                    // slider past one full revolution.
                    use std::f32::consts::PI;
                    let mut a = v % (2.0 * PI);
                    if a > PI {
                        a -= 2.0 * PI;
                    } else if a <= -PI {
                        a += 2.0 * PI;
                    }
                    state.config.radial_menu.visuals.light_angle_rad = a;
                }
                VisualField::SpecularSweepIntensity => {
                    state.config.radial_menu.visuals.specular_sweep_intensity = v.clamp(0.0, 1.0);
                }
                VisualField::SpecularSweepPeriod => {
                    // Clamp to a sane range — too fast looks
                    // like a strobe; too slow looks frozen.
                    state.config.radial_menu.visuals.specular_sweep_period_s = v.clamp(1.0, 30.0);
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
        Message::SetTooltipDelay(ms) => {
            // Cap at 5s — anything longer is effectively "off" and
            // the user should set the font size to 0 instead.
            state.config.radial_menu.visuals.tooltip_delay_ms = ms.min(5000);
            state.touch();
            Task::none()
        }
        Message::SetPageNameShow(on) => {
            state.config.radial_menu.visuals.page_name_show = on;
            state.touch();
            Task::none()
        }
        Message::SetPageNameVisibleMs(ms) => {
            // Cap at 4s — beyond that the flash starts feeling
            // less like an announcement and more like a static
            // label.
            state.config.radial_menu.visuals.page_name_visible_ms = ms.min(4000);
            state.touch();
            Task::none()
        }
        Message::SetPageNameTransitionMs(ms) => {
            // Floor at 50 ms (the slide animation needs at least
            // that to feel intentional) and cap at 1 s.
            state.config.radial_menu.visuals.page_name_transition_ms = ms.clamp(50, 1000);
            state.touch();
            Task::none()
        }
        Message::SetPageNameSlideDistance(px) => {
            state.config.radial_menu.visuals.page_name_slide_distance_px = px.clamp(0.0, 200.0);
            state.touch();
            Task::none()
        }
        Message::SetPageNameArced(v) => {
            state.config.radial_menu.visuals.page_name_arced = v;
            state.touch();
            Task::none()
        }
        Message::SetPageNameUseMonospace(v) => {
            state.config.radial_menu.visuals.page_name_use_monospace = v;
            state.touch();
            Task::none()
        }
        Message::SetPageNameFontFamily(s) => {
            state.config.radial_menu.visuals.page_name_font_family = s;
            state.touch();
            Task::none()
        }
        Message::SetTooltipUseMonospace(v) => {
            state.config.radial_menu.visuals.tooltip_use_monospace = v;
            state.touch();
            Task::none()
        }
        Message::SetTooltipFontFamily(s) => {
            state.config.radial_menu.visuals.tooltip_font_family = s;
            state.touch();
            Task::none()
        }
        Message::SetTooltipBgColor(s) => {
            state.config.radial_menu.visuals.tooltip_bg_color = s;
            state.touch();
            Task::none()
        }
        Message::SetTooltipBgAlpha(a) => {
            state.config.radial_menu.visuals.tooltip_bg_alpha = a.clamp(0.0, 1.0);
            state.touch();
            Task::none()
        }
        Message::SetTooltipTextColor(s) => {
            state.config.radial_menu.visuals.tooltip_text_color = s;
            state.touch();
            Task::none()
        }
        Message::ResetTooltipStyle => {
            let defaults = oxidemx_shared::VisualSettings::default();
            let v = &mut state.config.radial_menu.visuals;
            v.tooltip_use_monospace = defaults.tooltip_use_monospace;
            v.tooltip_font_family = defaults.tooltip_font_family;
            v.tooltip_bg_color = defaults.tooltip_bg_color;
            v.tooltip_bg_alpha = defaults.tooltip_bg_alpha;
            v.tooltip_text_color = defaults.tooltip_text_color;
            state.status = "Tooltip styling reset to defaults".into();
            state.touch();
            Task::none()
        }
        Message::TestHapticEvent(event) => {
            Task::perform(daemon::trigger_haptic_event(event), |_| {
                Message::HapticTestFired
            })
        }
        Message::HapticTestFired => Task::none(),
        Message::SetTransition(elem, dir, cfg) => {
            let anim = elem.get_mut(&mut state.config.radial_menu.animation);
            *dir.pick_mut(anim) = cfg;
            state.touch();
            Task::none()
        }
        Message::SetChainStagger(elem, ms) => {
            let anim = elem.get_mut(&mut state.config.radial_menu.animation);
            anim.chain = Some(oxidemx_shared::ChainConfig { stagger_ms: ms });
            state.touch();
            Task::none()
        }
        Message::ResetElementAnimation(elem) => {
            let anim = elem.get_mut(&mut state.config.radial_menu.animation);
            *anim = elem.default_for();
            state.touch();
            Task::none()
        }
        Message::SetPageTransition(cfg) => {
            state.config.radial_menu.animation.page_transition = cfg;
            state.touch();
            Task::none()
        }
        Message::SetDispatchBurstStyle(s) => {
            state.config.radial_menu.visuals.dispatch_burst_style = s;
            state.touch();
            Task::none()
        }
        Message::ResetPageTransition => {
            state.config.radial_menu.animation.page_transition =
                oxidemx_shared::PageTransitionConfig::default();
            state.touch();
            Task::none()
        }
        Message::BeginShortcutCapture(target) => {
            // Toggle behaviour: clicking Capture twice cancels.
            if state.capturing_shortcut == Some(target) {
                state.capturing_shortcut = None;
                state.status = "Shortcut capture cancelled".into();
            } else {
                state.capturing_shortcut = Some(target);
                state.status = "Press a key chord… (Esc to cancel)".into();
            }
            Task::none()
        }
        Message::CancelShortcutCapture => {
            state.capturing_shortcut = None;
            state.status = "Shortcut capture cancelled".into();
            Task::none()
        }
        Message::ShortcutCaptured(chord) => {
            if let Some(target) = state.capturing_shortcut {
                match target {
                    ShortcutCaptureTarget::Slice(idx) => {
                        if let Some(slice) = state.active_slices_mut().get_mut(idx) {
                            slice.command = chord.clone();
                            state.touch();
                        }
                    }
                    ShortcutCaptureTarget::SubItem { parent, idx } => {
                        if let Some(item) = state
                            .active_slices_mut()
                            .get_mut(parent)
                            .and_then(|p| p.submenu.get_mut(idx))
                        {
                            item.command = chord.clone();
                            state.touch();
                        }
                    }
                }
                state.capturing_shortcut = None;
                state.status = format!("Captured: {chord}");
            }
            Task::none()
        }
        Message::AiProviderChanged(p) => {
            state.config.overlay.ai.provider = p;
            // Reset the model to the new provider's default so the
            // stored model never points at the wrong provider.
            state.config.overlay.ai.model = p.default_model().to_string();
            // Refresh the key indicator for the now-selected provider.
            state.ai_key_present = ai_key_present_for(p);
            state.ai_key_draft.clear();
            state.touch();
            Task::none()
        }
        Message::AiModelChanged(m) => {
            state.config.overlay.ai.model = m;
            state.touch();
            Task::none()
        }
        Message::AiLocalEndpointChanged(url) => {
            state.config.overlay.ai.local_endpoint = url;
            state.touch();
            Task::none()
        }
        Message::AiRoutingToggled(on) => {
            state.config.overlay.ai.routing_enabled = on;
            state.touch();
            Task::none()
        }
        Message::AiFastProviderChanged(p) => {
            state.config.overlay.ai.fast_provider = p;
            // Keep the fast model valid for the new fast provider.
            state.config.overlay.ai.fast_model = p.default_model().to_string();
            state.touch();
            Task::none()
        }
        Message::AiFastModelChanged(m) => {
            state.config.overlay.ai.fast_model = m;
            state.touch();
            Task::none()
        }
        Message::AiAllowlistDraftChanged(v) => {
            state.ai_allowlist_draft = v;
            Task::none()
        }
        Message::AiAllowlistAdd => {
            let entry = state.ai_allowlist_draft.trim().to_string();
            if entry.is_empty() || entry == "*" {
                state.status = "Allowlist entries must name a command (bare * is refused)".into();
                return Task::none();
            }
            let list = &mut state.config.overlay.ai.command_allowlist;
            if list.iter().any(|e| e == &entry) {
                state.status = format!("\"{entry}\" is already allowlisted");
                return Task::none();
            }
            list.push(entry);
            state.ai_allowlist_draft.clear();
            state.touch();
            Task::none()
        }
        Message::AiAllowlistRemove(i) => {
            let list = &mut state.config.overlay.ai.command_allowlist;
            if i < list.len() {
                let removed = list.remove(i);
                state.status = format!("Removed \"{removed}\" from the allowlist");
                state.touch();
            }
            Task::none()
        }
        Message::AiModelDirChanged(s) => {
            state.config.overlay.local_models.download_dir = std::path::PathBuf::from(s);
            state.touch();
            Task::none()
        }
        Message::AiModelDirPick => Task::perform(
            async {
                let chosen = rfd::AsyncFileDialog::new()
                    .set_title("Choose model download folder")
                    .pick_folder()
                    .await;
                chosen
                    .map(|h| h.path().display().to_string())
                    .unwrap_or_default()
            },
            |path| {
                if path.is_empty() {
                    Message::Noop
                } else {
                    Message::AiModelDirChanged(path)
                }
            },
        ),
        Message::AiIdleTimeoutChanged(s) => {
            if let Ok(secs) = s.parse::<u64>() {
                state.config.overlay.local_models.idle_timeout_secs = secs;
                state.touch();
            }
            Task::none()
        }
        Message::AiKeyDraftChanged(v) => {
            state.ai_key_draft = v;
            Task::none()
        }
        Message::AiKeySave => {
            let key = state.ai_key_draft.trim().to_string();
            if key.is_empty() {
                state.status = "AI key field is empty — nothing saved".into();
                return Task::none();
            }
            let Some(path) = ai_key_path_for(state.config.overlay.ai.provider) else {
                state.status = "This provider needs no API key".into();
                return Task::none();
            };
            let result = (|| -> std::io::Result<()> {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&path, &key)?;
                // Secret on disk — owner read/write only.
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&path)?.permissions();
                perms.set_mode(0o600);
                std::fs::set_permissions(&path, perms)?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    state.ai_key_draft.clear();
                    state.ai_key_present = true;
                    state.status =
                        "AI API key saved — the assistant uses it from the next message".into();
                }
                Err(e) => state.status = format!("AI key save failed: {e}"),
            }
            Task::none()
        }
        Message::AiKeyRemove => {
            let Some(path) = ai_key_path_for(state.config.overlay.ai.provider) else {
                return Task::none();
            };
            match std::fs::remove_file(path) {
                Ok(()) => {
                    state.ai_key_present = false;
                    state.status = "AI API key removed".into();
                }
                Err(e) => state.status = format!("AI key remove failed: {e}"),
            }
            Task::none()
        }
        Message::ExportConfig => {
            // Capture the full bundle (config + macros + user
            // themes) on the iced thread so file I/O happens
            // before we hand control to the dialog future. Bundle
            // lookups read the same dirs as the daemon, so an
            // export captures everything a fresh install would
            // need to re-create the user's setup.
            let bundle = bundle::ConfigBundle::capture(state.config.clone());
            Task::perform(
                async move {
                    let json = serde_json::to_string_pretty(&bundle)
                        .map_err(|e| format!("serialise: {e}"))?;
                    let chosen = rfd::AsyncFileDialog::new()
                        .set_title("Export config")
                        .set_file_name("oxidemx-bundle.json")
                        .add_filter("JSON", &["json"])
                        .save_file()
                        .await;
                    let handle = chosen.ok_or_else(|| "cancelled".to_string())?;
                    let path = handle.path().to_path_buf();
                    std::fs::write(&path, json).map_err(|e| format!("write: {e}"))?;
                    Ok::<String, String>(path.display().to_string())
                },
                Message::ConfigExported,
            )
        }
        Message::ConfigExported(Ok(path)) => {
            state.status = format!("Config exported to {path}");
            Task::none()
        }
        Message::ConfigExported(Err(e)) => {
            if e == "cancelled" {
                state.status = "Export cancelled".into();
            } else {
                state.status = format!("Export failed: {e}");
            }
            Task::none()
        }
        Message::ImportConfig => Task::perform(
            async move {
                let chosen = rfd::AsyncFileDialog::new()
                    .set_title("Import config")
                    .add_filter("JSON", &["json"])
                    .pick_file()
                    .await;
                let handle = chosen.ok_or_else(|| "cancelled".to_string())?;
                let bytes = std::fs::read(handle.path()).map_err(|e| format!("read: {e}"))?;
                // parse_and_install handles both the new bundle
                // format and the older bare-AppConfig export,
                // unpacking macros + themes to disk as a side
                // effect of the bundle path.
                let (mut cfg, errors) = bundle::parse_and_install(&bytes)?;
                cfg.radial_menu.normalize_pages();
                Ok::<(Box<oxidemx_shared::AppConfig>, Vec<String>), String>((Box::new(cfg), errors))
            },
            Message::ConfigImported,
        ),
        Message::ConfigImported(Ok((cfg, errors))) => {
            state.config = *cfg;
            state.palette = palette::Palette::resolve(&state.config.theme);
            state.theme_editor = None;
            // Re-read macros from disk so the editor sees newly
            // installed bundle entries; the daemon also reloads
            // them on its next use.
            state.macros = tabs::macros::list();
            state.touch();
            state.status = if errors.is_empty() {
                "Config imported (incl. macros + themes) — saving…".into()
            } else {
                format!(
                    "Config imported with {} non-fatal issue(s): {}",
                    errors.len(),
                    errors.first().cloned().unwrap_or_default()
                )
            };
            Task::none()
        }
        Message::ConfigImported(Err(e)) => {
            if e == "cancelled" {
                state.status = "Import cancelled".into();
            } else {
                state.status = format!("Import failed: {e}");
            }
            Task::none()
        }
        Message::StatusFadeTick => {
            // Pure redraw trigger — also runs the auto-clear so
            // the string drops the moment LIFETIME elapses
            // instead of waiting for the next slow SaveTick.
            state.maybe_clear_status();
            Task::none()
        }
        Message::LastTabPersisted => Task::none(),
        Message::ExportTheme(slug) => {
            // Load fresh from disk so any unsaved customiser edits
            // don't leak in. If the theme can't load (file went
            // missing under us) surface the error rather than
            // exporting bundled defaults silently.
            let theme = match oxidemx_shared::theme::Theme::load(
                &oxidemx_shared::theme::ThemeName::from(slug.as_str()),
            ) {
                Some(t) => t,
                None => {
                    state.status = format!("Could not load theme \"{slug}\" for export");
                    return Task::none();
                }
            };
            let suggested = format!("{slug}.json");
            Task::perform(
                async move {
                    let json = serde_json::to_string_pretty(&theme)
                        .map_err(|e| format!("serialise: {e}"))?;
                    let chosen = rfd::AsyncFileDialog::new()
                        .set_title("Export theme")
                        .set_file_name(&suggested)
                        .add_filter("Theme JSON", &["json"])
                        .save_file()
                        .await;
                    let handle = chosen.ok_or_else(|| "cancelled".to_string())?;
                    std::fs::write(handle.path(), json).map_err(|e| format!("write: {e}"))?;
                    Ok::<String, String>(handle.path().display().to_string())
                },
                Message::ThemeExported,
            )
        }
        Message::ThemeExported(Ok(path)) => {
            state.status = format!("Theme exported to {path}");
            Task::none()
        }
        Message::ThemeExported(Err(e)) => {
            if e == "cancelled" {
                state.status = "Theme export cancelled".into();
            } else {
                state.status = format!("Theme export failed: {e}");
            }
            Task::none()
        }
        Message::BeginRenameTheme(slug) => {
            state.renaming_theme = Some(RenameThemeDraft {
                original: slug.clone(),
                draft: slug,
            });
            Task::none()
        }
        Message::SetRenameThemeDraft(s) => {
            if let Some(r) = state.renaming_theme.as_mut() {
                r.draft = s;
            }
            Task::none()
        }
        Message::CancelRenameTheme => {
            state.renaming_theme = None;
            Task::none()
        }
        Message::CommitRenameTheme => {
            let rename = match state.renaming_theme.take() {
                Some(r) => r,
                None => return Task::none(),
            };
            let new_slug = sanitize_slug(&rename.draft);
            if new_slug.is_empty() {
                state.status = "Rename failed: name is empty".into();
                state.renaming_theme = Some(rename);
                return Task::none();
            }
            if new_slug == rename.original {
                // No change — silently close.
                return Task::none();
            }
            let theme = match oxidemx_shared::theme::Theme::load(
                &oxidemx_shared::theme::ThemeName::from(rename.original.as_str()),
            ) {
                Some(t) => t,
                None => {
                    state.status = format!("Rename failed: could not load \"{}\"", rename.original);
                    return Task::none();
                }
            };
            let original = rename.original.clone();
            let was_active = state.config.theme.as_str() == rename.original;
            // Save under new slug, then delete the old file. Doing
            // it in this order means a partial failure leaves both
            // copies on disk rather than losing the theme.
            let _ = oxidemx_shared::theme::save_user_theme(&new_slug, &theme).map_err(|e| {
                state.status = format!("Rename save failed: {e}");
            });
            let _ = oxidemx_shared::theme::delete_user_theme(&original).map_err(|e| {
                state.status = format!("Rename cleanup failed: {e}");
            });
            if was_active {
                state.config.theme = oxidemx_shared::theme::ThemeName::from(new_slug.as_str());
                state.palette = palette::Palette::resolve(&state.config.theme);
                state.touch();
            }
            state.status = format!("Renamed \"{original}\" → \"{new_slug}\"");
            Task::none()
        }
        Message::ThemeRenamed(Ok((_old, _new))) => Task::none(),
        Message::ThemeRenamed(Err(e)) => {
            state.status = format!("Rename failed: {e}");
            Task::none()
        }
        Message::ImportTheme => Task::perform(
            async move {
                let chosen = rfd::AsyncFileDialog::new()
                    .set_title("Import theme")
                    .add_filter("Theme JSON", &["json"])
                    .pick_file()
                    .await;
                let handle = chosen.ok_or_else(|| "cancelled".to_string())?;
                let stem = handle
                    .path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| "filename has no stem".to_string())?;
                let slug = sanitize_slug(&stem);
                if slug.is_empty() {
                    return Err("filename gives empty slug".to_string());
                }
                let bytes = std::fs::read(handle.path()).map_err(|e| format!("read: {e}"))?;
                let theme: oxidemx_shared::theme::Theme =
                    serde_json::from_slice(&bytes).map_err(|e| format!("parse: {e}"))?;
                oxidemx_shared::theme::save_user_theme(&slug, &theme)
                    .map_err(|e| format!("save: {e}"))?;
                Ok::<String, String>(slug)
            },
            Message::ThemeImported,
        ),
        Message::ThemeImported(Ok(slug)) => {
            state.config.theme = oxidemx_shared::theme::ThemeName::from(slug.as_str());
            state.palette = palette::Palette::resolve(&state.config.theme);
            state.touch();
            state.status = format!("Imported theme \"{slug}\" and switched to it");
            Task::none()
        }
        Message::ThemeImported(Err(e)) => {
            if e == "cancelled" {
                state.status = "Theme import cancelled".into();
            } else {
                state.status = format!("Theme import failed: {e}");
            }
            Task::none()
        }
        Message::OpenConfigFolder => {
            let path = oxidemx_shared::config::default_config_path()
                .and_then(|p| p.parent().map(|q| q.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            match std::process::Command::new("xdg-open").arg(&path).spawn() {
                Ok(_) => {
                    state.status = format!("Opening {}", path.display());
                }
                Err(e) => {
                    state.status = format!("Could not open folder ({e})");
                }
            }
            Task::none()
        }
        Message::OpenOverlayForPreview => {
            // Coords go to the daemon → overlay positioner. We
            // don't have a cheap way to query the actual cursor
            // here from inside iced, so use a sensible screen-
            // centre default; the GNOME extension's overlay
            // positioner will keep the menu on-screen even on
            // smaller displays. The user can always click to
            // dismiss and re-trigger via their bound mouse button
            // for a real cursor-anchored test.
            state.status =
                "Opening radial menu… scroll wheel over centre puck to test transitions".into();
            Task::perform(daemon::show_radial_at(960, 540), |_| {
                Message::OverlayPreviewFired
            })
        }
        Message::OverlayPreviewFired => Task::none(),
        Message::ResetAll => {
            // Two-press confirmation. First click arms the reset
            // and shows a warning in the footer; second click
            // within RESET_CONFIRM_WINDOW actually resets. After
            // the window times out the next click rearms instead
            // of acting — so a stale arming can't accidentally
            // wipe state when the user comes back to the window.
            //
            // The reset is "all visual + interaction defaults":
            // animation block (incl. page transition), visuals
            // (opacity + label sizes + font), and haptics
            // (per-event patterns + debounce). User-defined slice
            // bindings, pages, app-context bindings, and macros
            // are *not* touched — those are real user data, not
            // settings.
            let now = Instant::now();
            let armed = state
                .reset_armed_at
                .map(|t| now.duration_since(t) <= RESET_CONFIRM_WINDOW)
                .unwrap_or(false);
            if armed {
                state.config.radial_menu.animation = AnimationConfig::default();
                state.config.radial_menu.visuals = VisualSettings::default();
                state.config.haptics = oxidemx_shared::haptics::HapticsConfig::default();
                state.reset_armed_at = None;
                state.status =
                    "Reset complete — animation + visuals + haptics back to defaults".into();
                state.touch();
            } else {
                state.reset_armed_at = Some(now);
                state.status = format!(
                    "Click \"Reset\" again within {}s to confirm — this wipes \
                     animation + visuals + haptics back to defaults. Slices, \
                     pages, and macros are kept.",
                    RESET_CONFIRM_WINDOW.as_secs()
                );
            }
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
        Message::SaveTick => {
            state.maybe_clear_status();
            state.maybe_save().unwrap_or_else(Task::none)
        }
        Message::Saved(Ok(())) => {
            info!("config saved");
            state.status = "Saved".to_string();
            // Tell the daemon to re-read the file we just wrote.
            // Without this, button reassignments + scroll /
            // pointer / haptic edits persist to disk but never
            // reach the device — daemon keeps running with the
            // config it loaded at startup. Fire-and-forget;
            // silent no-op when the daemon isn't running.
            Task::perform(daemon::request_reload(), |_| Message::Noop)
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
            state.config.theme = oxidemx_shared::theme::ThemeName::from(name.as_str());
            state.palette = palette::Palette::resolve(&state.config.theme);
            state.touch();
            Task::none()
        }
        // --- Slices editor handlers (operate on the active page) ---
        Message::AddSlice => {
            let slices = state.active_slices_mut();
            slices.push(oxidemx_shared::Slice {
                action_id: None,
                label: "New slice".into(),
                kind: oxidemx_shared::ActionKind::Exec,
                command: String::new(),
                color: "accent".into(),
                icon: String::new(),
                submenu: Vec::new(),
                visible_if: None,
                icon_untinted: false,
                description: String::new(),
                widget: None,
                dial: None,
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
            // Indices shifted — a stale picker/undo snapshot could
            // restore onto the wrong slot.
            state.reset_picker();
            Task::none()
        }
        Message::MoveSliceUp(i) => {
            let slices = state.active_slices_mut();
            if i > 0 && i < slices.len() {
                slices.swap(i, i - 1);
                state.touch();
            }
            state.reset_picker();
            Task::none()
        }
        Message::MoveSliceDown(i) => {
            let slices = state.active_slices_mut();
            if i + 1 < slices.len() {
                slices.swap(i, i + 1);
                state.touch();
            }
            state.reset_picker();
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
        Message::SetSliceWidgetSource(i, source) => {
            if let Some(slice) = state.active_slices_mut().get_mut(i) {
                slice.widget = Some(oxidemx_shared::WidgetConfig {
                    source,
                    format: None,
                    scope: oxidemx_shared::WidgetScope::Instance,
                    instance_key: None,
                });
                state.touch();
            }
            Task::none()
        }

        // --- Behavior chip + picker panel (spec §10b/c) ---
        Message::OpenPicker(idx) => {
            // Snapshot for undo-by-reselect — but only when no
            // snapshot exists for this slot yet, so a pick →
            // reopen → re-pick round-trip can still restore the
            // original slice (incl. a widget's instance config).
            if state.picker_undo.as_ref().map(|(i, _)| *i) != Some(idx) {
                state.picker_undo = state.active_slices().get(idx).cloned().map(|s| (idx, s));
            }
            state.picker_open = Some(idx);
            state.picker_search.clear();
            Task::none()
        }
        Message::ClosePicker => {
            state.reset_picker();
            Task::none()
        }
        Message::PickerSearch(q) => {
            state.picker_search = q;
            Task::none()
        }
        Message::PickAction(i, kind) => {
            use tabs::buttons::picker::{pick_matches_slice, PickChoice};
            let restored = match state.picker_undo.as_ref() {
                Some((ui, stored))
                    if *ui == i && pick_matches_slice(stored, &PickChoice::Action(kind)) =>
                {
                    Some(stored.clone())
                }
                _ => None,
            };
            if let Some(slice) = state.active_slices_mut().get_mut(i) {
                match restored {
                    // Undo-by-reselect: bring the whole snapshot back.
                    Some(stored) => *slice = stored,
                    // Fresh pick — same mutation as the legacy
                    // SetSliceKind (command/color/etc. survive).
                    None => slice.kind = kind,
                }
                state.touch();
            }
            // Single click applies + collapses; the snapshot stays
            // for a potential reselect (cleared on Cancel/deselect).
            state.picker_open = None;
            state.picker_search.clear();
            Task::none()
        }
        Message::PickWidget(i, source) => {
            use tabs::buttons::picker::{apply_widget_pick, pick_matches_slice, PickChoice};
            let restored = match state.picker_undo.as_ref() {
                Some((ui, stored))
                    if *ui == i
                        && pick_matches_slice(stored, &PickChoice::Widget(source.clone())) =>
                {
                    Some(stored.clone())
                }
                _ => None,
            };
            let page_name = state
                .config
                .radial_menu
                .pages
                .get(state.active_page)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            // Cloned so the registry can be read while the slice is
            // borrowed mutably; the list is tiny (installed widgets).
            let registry = state.widget_registry.clone();
            if let Some(slice) = state.active_slices_mut().get_mut(i) {
                match restored {
                    Some(stored) => *slice = stored,
                    None => apply_widget_pick(slice, source, &page_name, i, &registry),
                }
                state.touch();
            }
            state.picker_open = None;
            state.picker_search.clear();
            Task::none()
        }
        Message::OpenWidgetStore => {
            state.widget_store = Some(widget_store::WidgetStoreState::default());
            Task::none()
        }
        Message::RescanWidgets => {
            rescan_widgets(state);
            Task::none()
        }
        Message::ConvertSliceToPlugin(i) => {
            use tabs::buttons::picker::{convert_slice_to_plugin, convertible_plugin_id};
            let page_name = state
                .config
                .radial_menu
                .pages
                .get(state.active_page)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let overlay = state.config.overlay.clone();
            let registry = state.widget_registry.clone();
            // The store is taken out so the conversion fn can write the
            // weather lift while the slice is borrowed mutably from the
            // same `state.config`.
            let mut store = std::mem::take(&mut state.config.widgets);
            let converted = state.active_slices_mut().get_mut(i).and_then(|slice| {
                // Re-check the gate (installed + Ready) — the message
                // only comes from the hint button, but the registry may
                // have changed between render and click.
                convertible_plugin_id(slice, &registry)?;
                convert_slice_to_plugin(slice, &overlay, &mut store, &page_name, i)
            });
            state.config.widgets = store;
            if let Some(id) = converted {
                info!("converted slice {i} to bundled plugin widget {id:?}");
                state.touch();
            }
            Task::none()
        }

        // --- Widget store / downloader dialog (spec §11) ---
        Message::CloseWidgetStore => {
            state.widget_store = None;
            Task::none()
        }
        Message::StoreSearch(s) => {
            if let Some(store) = state.widget_store.as_mut() {
                store.search = s;
            }
            Task::none()
        }
        Message::StoreUrlInput(s) => {
            if let Some(store) = state.widget_store.as_mut() {
                store.url_input = s;
            }
            Task::none()
        }
        Message::StoreInstallFromFile => {
            if let Some(store) = state.widget_store.as_mut() {
                store.busy = true;
            }
            Task::perform(
                widget_store::pick_and_install(),
                Message::StoreInstallResult,
            )
        }
        Message::StoreInstallFromUrl => {
            let url = state
                .widget_store
                .as_ref()
                .map(|s| s.url_input.trim().to_string())
                .unwrap_or_default();
            if url.is_empty() {
                state.status = "Enter a bundle URL first".into();
                return Task::none();
            }
            if let Some(store) = state.widget_store.as_mut() {
                store.busy = true;
            }
            Task::perform(
                widget_store::download_and_install(url),
                Message::StoreInstallResult,
            )
        }
        Message::StoreInstallResult(outcome) => {
            if let Some(store) = state.widget_store.as_mut() {
                store.busy = false;
            }
            match outcome {
                widget_store::StoreInstallOutcome::Installed { id } => {
                    rescan_widgets(state);
                    if let Some(store) = state.widget_store.as_mut() {
                        store.consent = None;
                    }
                    state.status = format!("Widget \"{id}\" installed");
                }
                widget_store::StoreInstallOutcome::NeedsConsent(prompt) => {
                    match state.widget_store.as_mut() {
                        Some(store) => store.consent = Some(prompt),
                        // Dialog closed while the install ran —
                        // don't install behind the user's back.
                        None => {
                            state.status =
                                "Install needs confirmation — reopen the widget store".into()
                        }
                    }
                }
                widget_store::StoreInstallOutcome::Cancelled => {
                    state.status = "Install cancelled".into();
                }
                widget_store::StoreInstallOutcome::Failed(e) => {
                    state.status = format!("Widget install failed: {e}");
                }
            }
            Task::none()
        }
        Message::StoreConsentAccept => {
            let prompt = state.widget_store.as_mut().and_then(|s| s.consent.take());
            match prompt {
                Some(p) => {
                    if let Some(store) = state.widget_store.as_mut() {
                        store.busy = true;
                    }
                    Task::perform(
                        widget_store::install_bundle(p.bundle, true),
                        Message::StoreInstallResult,
                    )
                }
                None => Task::none(),
            }
        }
        Message::StoreConsentCancel => {
            if let Some(store) = state.widget_store.as_mut() {
                store.consent = None;
            }
            state.status = "Install cancelled".into();
            Task::none()
        }
        Message::StoreUninstall(id) => {
            let Some(store) = state.widget_store.as_mut() else {
                return Task::none();
            };
            if store.pending_uninstall.as_deref() == Some(id.as_str()) {
                store.pending_uninstall = None;
                match widget_store::uninstall(&id) {
                    Ok(()) => {
                        rescan_widgets(state);
                        // Settings bags (config.widgets.*) are
                        // deliberately KEPT (spec §9) — a reinstall
                        // picks the old values straight back up.
                        state.status =
                            format!("Widget \"{id}\" uninstalled — its settings are kept");
                    }
                    Err(e) => state.status = format!("Uninstall failed: {e}"),
                }
            } else {
                store.pending_uninstall = Some(id);
            }
            Task::none()
        }

        // --- Widget options card (spec §5/§6/§10d) ---
        Message::SetWidgetScope(idx, scope) => {
            if let Some((id, old_scope, ikey)) = state.widget_edit_ctx(idx) {
                if old_scope != scope {
                    if let Some(w) = state
                        .active_slices_mut()
                        .get_mut(idx)
                        .and_then(|s| s.widget.as_mut())
                    {
                        w.scope = scope;
                    }
                    // Global → slice: seed the instance bag as a copy
                    // of the current resolved values so it diverges
                    // from there (spec §6 table). The reverse toggle
                    // keeps the instance bag (ignored until toggled
                    // back).
                    if scope == oxidemx_shared::WidgetScope::Instance {
                        let defaults = state
                            .widget_manifests
                            .get(&id)
                            .map(|m| m.defaults())
                            .unwrap_or_default();
                        state.config.widgets.seed_instance(&id, &ikey, &defaults);
                    }
                    state.touch();
                }
            }
            Task::none()
        }
        Message::SetWidgetOption { slice, key, value } => {
            if let Some((id, scope, ikey)) = state.widget_edit_ctx(slice) {
                tabs::buttons::widget_options::write_option(
                    &mut state.config.widgets,
                    &id,
                    &ikey,
                    scope,
                    &key,
                    value,
                );
                state.touch();
            }
            Task::none()
        }
        Message::ResetWidgetOption { slice, key } => {
            if let Some((id, scope, ikey)) = state.widget_edit_ctx(slice) {
                tabs::buttons::widget_options::reset_option(
                    &mut state.config.widgets,
                    &id,
                    &ikey,
                    scope,
                    &key,
                );
                state.touch();
            }
            Task::none()
        }
        Message::WidgetLocQuery { slice, key, text } => {
            // Typing claims the shared search state for this control;
            // stale results from another control are dropped.
            let target = Some((slice, key));
            if state.widget_loc_target != target {
                state.widget_loc_results.clear();
            }
            state.widget_loc_target = target;
            state.widget_loc_query = text;
            Task::none()
        }
        Message::WidgetLocSearch { slice, key } => {
            let target = Some((slice, key));
            if state.widget_loc_target != target {
                // Search pressed on a control that never claimed the
                // query — claim it empty instead of searching another
                // control's text under this key.
                state.widget_loc_target = target;
                state.widget_loc_query.clear();
                state.widget_loc_results.clear();
                return Task::none();
            }
            let q = state.widget_loc_query.trim().to_string();
            if q.is_empty() || state.widget_loc_searching {
                return Task::none();
            }
            state.widget_loc_searching = true;
            state.widget_loc_results.clear();
            Task::perform(geocode::search(q), Message::WidgetLocResults)
        }
        Message::WidgetLocResults(res) => {
            state.widget_loc_searching = false;
            match res {
                Ok(hits) => state.widget_loc_results = hits,
                Err(e) => state.status = format!("Location lookup failed: {e}"),
            }
            Task::none()
        }
        Message::WidgetLocPick {
            slice,
            key,
            name,
            lat,
            lon,
        } => {
            if let Some((id, scope, ikey)) = state.widget_edit_ctx(slice) {
                let value = serde_json::json!({ "name": name, "lat": lat, "lon": lon });
                tabs::buttons::widget_options::write_option(
                    &mut state.config.widgets,
                    &id,
                    &ikey,
                    scope,
                    &key,
                    value,
                );
                state.widget_loc_target = None;
                state.widget_loc_query.clear();
                state.widget_loc_results.clear();
                state.status = format!("Location set: {name}");
                state.touch();
            }
            Task::none()
        }
        Message::WidgetPreviewEvent(event) => {
            widget_preview::on_event(state, event);
            Task::none()
        }
        Message::SetSliceDial(i, kind) => {
            if let Some(slice) = state.active_slices_mut().get_mut(i) {
                slice.dial = Some(kind);
                state.touch();
            }
            Task::none()
        }
        Message::SetWeatherQuery(q) => {
            state.weather_query = q;
            Task::none()
        }
        Message::WeatherSearch => {
            let q = state.weather_query.trim().to_string();
            if q.is_empty() || state.weather_searching {
                return Task::none();
            }
            state.weather_searching = true;
            state.weather_results.clear();
            Task::perform(geocode::search(q), Message::WeatherResults)
        }
        Message::WeatherResults(res) => {
            state.weather_searching = false;
            match res {
                Ok(places) => {
                    state.status = format!("{} places found", places.len());
                    state.weather_results = places;
                }
                Err(e) => state.status = format!("Weather lookup failed: {e}"),
            }
            Task::none()
        }
        Message::WeatherPick(idx) => {
            if let Some(place) = state.weather_results.get(idx).cloned() {
                state.config.overlay.weather_location = Some((place.lat, place.lon));
                state.config.overlay.weather_place = Some(place.name.clone());
                state.weather_results.clear();
                state.weather_query.clear();
                state.status = format!("Weather location set: {}", place.name);
                state.touch();
            }
            Task::none()
        }
        Message::WeatherClearLocation => {
            state.config.overlay.weather_location = None;
            state.config.overlay.weather_place = None;
            state.status = "Weather location cleared".to_string();
            state.touch();
            Task::none()
        }
        Message::SetWeatherCelsius(celsius) => {
            state.config.overlay.weather_celsius = celsius;
            state.status = format!("Weather units set to °{}", if celsius { "C" } else { "F" });
            state.touch();
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
                    Some(oxidemx_shared::Condition::Always) | None => None,
                    other => other,
                };
                state.touch();
            }
            Task::none()
        }
        Message::SetSubItemVisibility {
            parent,
            idx,
            condition,
        } => {
            if let Some(item) = state
                .active_slices_mut()
                .get_mut(parent)
                .and_then(|p| p.submenu.get_mut(idx))
            {
                item.visible_if = match condition {
                    Some(oxidemx_shared::Condition::Always) | None => None,
                    other => other,
                };
                state.touch();
            }
            Task::none()
        }

        Message::WindowResized(width) => {
            state.window_width = width;
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
            // Behavior picker + undo snapshot are slot-bound too —
            // moving to a different slot invalidates both.
            if state.selected_slice != Some(i) {
                state.reset_picker();
            }
            state.selected_slice = Some(i);
            // Empty-slot ergonomics (plan 5): the page's slice list
            // may be shorter than its slot count, so selecting an
            // empty slot (reorder row / radial-preview wedge) first
            // creates the backing slice — padded with inert
            // kind-None placeholders — and then opens the behavior
            // picker right away, making "click empty slot → choose
            // what it does" a single step.
            if tabs::buttons::rows::ensure_slot_exists(state.active_slices_mut(), i) {
                state.touch();
            }
            let placeholder = state
                .active_slices()
                .get(i)
                .is_some_and(tabs::buttons::rows::is_placeholder);
            if placeholder && state.picker_open != Some(i) {
                // Mirror Message::OpenPicker — snapshot once for
                // undo-by-reselect, open, clear the search.
                if state.picker_undo.as_ref().map(|(u, _)| *u) != Some(i) {
                    state.picker_undo = state.active_slices().get(i).cloned().map(|s| (i, s));
                }
                state.picker_open = Some(i);
                state.picker_search.clear();
            }
            Task::none()
        }
        Message::DismissSliceSelection => {
            state.selected_slice = None;
            // Close anything bound to the deselected slot.
            state.icon_picker = None;
            state.app_command_picker = None;
            state.reset_picker();
            Task::none()
        }
        Message::SwapSlices { from, to } => {
            let slices = state.active_slices_mut();
            // Pad to N_SLICES so the user can drop into an empty slot.
            while slices.len() < 8.max(from + 1).max(to + 1) {
                slices.push(oxidemx_shared::Slice {
                    action_id: None,
                    label: "(empty)".into(),
                    kind: oxidemx_shared::ActionKind::None,
                    command: String::new(),
                    color: "accent".into(),
                    icon: String::new(),
                    submenu: Vec::new(),
                    visible_if: None,
                    icon_untinted: false,
                    description: String::new(),
                    widget: None,
                    dial: None,
                });
            }
            if from < slices.len() && to < slices.len() {
                slices.swap(from, to);
                // Follow the moved slice — the user usually wants to
                // continue editing it.
                state.selected_slice = Some(to);
                state.touch();
            }
            // Slot indices changed under the picker/undo snapshot.
            state.reset_picker();
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
                HapticsEvent::SubmenuOpen => pe.submenu_open = pattern,
                HapticsEvent::SubmenuClose => pe.submenu_close = pattern,
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
        Message::SetScrollHorizontalInvert(on) => {
            state.config.scroll.horizontal_invert = on;
            state.touch();
            // Direct D-Bus apply via ThumbWheel — bypasses the
            // ReloadConfig path so the device flips instantly.
            // Best-effort: silently no-ops if the daemon or
            // device doesn't expose 0x2150.
            Task::perform(daemon::set_thumb_wheel_invert(on), |_| Message::Noop)
        }
        Message::SetScrollSmooth(on) => {
            state.config.scroll.smooth = on;
            state.touch();
            Task::none()
        }
        Message::SetScrollSmartshift(on) => {
            state.config.scroll.smartshift = on;
            state.touch();
            // Direct D-Bus apply — bypasses the ReloadConfig path
            // so the device updates instantly. The reload still
            // fires later via Saved → request_reload, but the
            // gate logic there will see no-op since the daemon's
            // already on the new state.
            fire_wheel_mode_apply(state)
        }
        Message::SetScrollSmartshiftThreshold(v) => {
            state.config.scroll.smartshift_threshold = v;
            state.touch();
            fire_wheel_mode_apply(state)
        }
        Message::SetScrollMode(s) => {
            state.config.scroll.mode = s;
            state.touch();
            fire_wheel_mode_apply(state)
        }
        Message::SetButtonAssignment(button, action) => {
            button.set(&mut state.config.buttons, action);
            state.touch();
            Task::none()
        }

        // --- Battery ---
        Message::BatteryTick => Task::perform(battery::poll(), Message::BatteryUpdate),
        Message::BatteryUpdate(s) => {
            // UPower fallback. Skip when the daemon has provided
            // a recent reading — its HID++ source is canonical
            // for charging state (UPower on Bluetooth/HID
            // routinely reports `Discharging` even when the
            // device is plugged in, which would otherwise erase
            // the daemon's correct charging=true).
            let daemon_fresh = state
                .battery_daemon_at
                .map(|t| t.elapsed() < std::time::Duration::from_secs(15))
                .unwrap_or(false);
            if !daemon_fresh {
                state.battery = s;
            }
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
        Message::ExportMacro(id) => {
            // Read the raw JSON off disk so we export the daemon-
            // authoritative shape verbatim rather than serialising
            // our reduced `MacroSummary`.
            let raw = match tabs::macros::read_raw(&id) {
                Ok(v) => v,
                Err(e) => {
                    state.status = format!("Could not read macro: {e}");
                    return Task::none();
                }
            };
            let suggested = format!("{id}.json");
            Task::perform(
                async move {
                    let json = serde_json::to_string_pretty(&raw)
                        .map_err(|e| format!("serialise: {e}"))?;
                    let chosen = rfd::AsyncFileDialog::new()
                        .set_title("Export macro")
                        .set_file_name(&suggested)
                        .add_filter("Macro JSON", &["json"])
                        .save_file()
                        .await;
                    let handle = chosen.ok_or_else(|| "cancelled".to_string())?;
                    std::fs::write(handle.path(), json).map_err(|e| format!("write: {e}"))?;
                    Ok::<String, String>(handle.path().display().to_string())
                },
                Message::MacroExported,
            )
        }
        Message::MacroExported(Ok(path)) => {
            state.status = format!("Macro exported to {path}");
            Task::none()
        }
        Message::MacroExported(Err(e)) => {
            if e == "cancelled" {
                state.status = "Macro export cancelled".into();
            } else {
                state.status = format!("Macro export failed: {e}");
            }
            Task::none()
        }
        Message::ImportMacro => Task::perform(
            async move {
                let chosen = rfd::AsyncFileDialog::new()
                    .set_title("Import macro")
                    .add_filter("Macro JSON", &["json"])
                    .pick_file()
                    .await;
                let handle = chosen.ok_or_else(|| "cancelled".to_string())?;
                let stem = handle
                    .path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| "filename has no stem".to_string())?;
                let id = sanitize_slug(&stem);
                if id.is_empty() {
                    return Err("filename gives empty id".to_string());
                }
                let bytes = std::fs::read(handle.path()).map_err(|e| format!("read: {e}"))?;
                let value: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(|e| format!("parse: {e}"))?;
                tabs::macros::write_raw(&id, &value).map_err(|e| format!("write: {e}"))?;
                Ok::<String, String>(id)
            },
            Message::MacroImported,
        ),
        Message::MacroImported(Ok(id)) => {
            state.macros = tabs::macros::list();
            state.status = format!("Imported macro \"{id}\"");
            Task::none()
        }
        Message::MacroImported(Err(e)) => {
            if e == "cancelled" {
                state.status = "Macro import cancelled".into();
            } else {
                state.status = format!("Macro import failed: {e}");
            }
            Task::none()
        }

        // --- Daemon snapshot ---
        Message::DaemonTick => Task::perform(daemon::poll(), Message::DaemonSnapshotReceived),
        Message::DaemonSnapshotReceived(snap) => {
            // Prefer the daemon's battery reading over UPower —
            // it's instant rather than UPower's ~30s lag AND
            // its HID++ charging-status byte is canonical (UPower
            // routinely shows Discharging on connected wireless
            // mice even when they're charging via USB-C). Stamp
            // the freshness instant so the UPower fallback path
            // doesn't overwrite us until the daemon goes quiet.
            if let Some((p, c)) = snap.battery {
                state.battery = Some(battery::BatteryStatus {
                    percent: p,
                    charging: c,
                });
                state.battery_daemon_at = Some(std::time::Instant::now());
            }

            // Reflect the device-side wheel mode + threshold back
            // into the local config so the picker matches whatever
            // the mouse currently has — including changes the user
            // made by pressing the SmartShift button on the device.
            // Only writes when there's a real difference so we
            // don't churn the autosave debouncer on every poll.
            if let Some((slug, threshold)) = &snap.wheel_mode {
                let cur_mode = if state.config.scroll.mode == "free" {
                    "freespin"
                } else {
                    state.config.scroll.mode.as_str()
                };
                if cur_mode != slug.as_str() {
                    state.config.scroll.mode = slug.clone();
                    state.touch();
                }
                // smartshift toggle mirrors the slug — UI semantic
                // is "smartshift = auto-disengage active".
                let want_smartshift = slug == "smartshift";
                if state.config.scroll.smartshift != want_smartshift {
                    state.config.scroll.smartshift = want_smartshift;
                    state.touch();
                }
                if want_smartshift
                    && *threshold > 0
                    && state.config.scroll.smartshift_threshold != *threshold as u32
                {
                    state.config.scroll.smartshift_threshold = *threshold as u32;
                    state.touch();
                }
            }

            // ThumbWheel invert reflects back into the toggle so
            // the user sees what the device actually has, not the
            // last value persisted to disk.
            if let Some((_divert, invert)) = snap.thumb_wheel {
                if state.config.scroll.horizontal_invert != invert {
                    state.config.scroll.horizontal_invert = invert;
                    state.touch();
                }
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
            state.status = "DPI updated".to_string();
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
            state.status = "Host switched".to_string();
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

        // --- Gaming → haptic redirect bridge (Phase 2: config-only) ---
        Message::SetHapticRedirectEnabled(on) => {
            state.config.gaming.haptic_redirect.enabled = on;
            state.touch();
            Task::none()
        }
        Message::SetHapticRedirectMode(m) => {
            state.config.gaming.haptic_redirect.mode = m;
            state.touch();
            Task::none()
        }
        Message::SetHapticRedirectCurve(c) => {
            state.config.gaming.haptic_redirect.curve = c;
            state.touch();
            Task::none()
        }
        Message::SetHapticRedirectEventMode(em) => {
            state.config.gaming.haptic_redirect.event_mode = em;
            state.touch();
            Task::none()
        }
        Message::SetHapticRedirectIntensityScale(v) => {
            state.config.gaming.haptic_redirect.intensity_scale = v;
            state.touch();
            Task::none()
        }
        Message::SetHapticRedirectMinIntensity(v) => {
            state.config.gaming.haptic_redirect.min_intensity = v;
            state.touch();
            Task::none()
        }
        Message::SetHapticRedirectStrongWeight(v) => {
            state.config.gaming.haptic_redirect.strong_weight = v;
            state.touch();
            Task::none()
        }
        Message::SetHapticRedirectWeakWeight(v) => {
            state.config.gaming.haptic_redirect.weak_weight = v;
            state.touch();
            Task::none()
        }
        Message::SetHapticRedirectThrottleMs(v) => {
            state.config.gaming.haptic_redirect.throttle_ms = v;
            state.touch();
            Task::none()
        }
        Message::SetHapticRedirectPassthroughToPad(on) => {
            state.config.gaming.haptic_redirect.passthrough_to_pad = on;
            state.touch();
            Task::none()
        }
        Message::SetHapticRedirectHardHide(on) => {
            state
                .config
                .gaming
                .haptic_redirect
                .hard_hide_real_controller = on;
            state.touch();
            Task::none()
        }
        Message::SetHapticRedirectKeepGamepadActive(secs) => {
            state.config.gaming.haptic_redirect.keep_gamepad_active_secs = secs;
            state.touch();
            Task::none()
        }
        Message::TestHapticRedirect => Task::perform(
            daemon::test_haptic_redirect(),
            Message::HapticRedirectTested,
        ),
        Message::HapticRedirectTested(Ok(())) => {
            state.status = "Haptic test pulse sent".into();
            Task::none()
        }
        Message::HapticRedirectTested(Err(e)) => {
            state.status = format!("Haptic test failed: {e}");
            Task::none()
        }
        Message::DiagnoseHapticRedirect => Task::perform(
            daemon::diagnose_haptic_redirect(),
            Message::HapticRedirectDiagnosed,
        ),
        Message::HapticRedirectDiagnosed(Ok(report)) => {
            state.haptic_diagnosis = Some(report);
            Task::none()
        }
        Message::HapticRedirectDiagnosed(Err(e)) => {
            state.status = format!("Diagnose failed: {e}");
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

        Message::DeleteUserTheme(slug) => {
            match oxidemx_shared::theme::delete_user_theme(&slug) {
                Ok(()) => {
                    state.status = format!("Deleted theme \"{slug}\"");
                    // If the deleted theme was active, fall back to
                    // the default. Otherwise just leave the picker
                    // alone — the catalogue will have one fewer
                    // entry on next render.
                    if state.config.theme.as_str() == slug {
                        state.config.theme = oxidemx_shared::theme::ThemeName::CatppuccinMocha;
                        state.palette = palette::Palette::resolve(&state.config.theme);
                        state.touch();
                    }
                }
                Err(e) => {
                    state.status = format!("Delete failed: {e}");
                }
            }
            Task::none()
        }
        // --- Custom theme editor ---
        Message::ToggleThemeCustomiser => {
            state.theme_editor = match state.theme_editor.take() {
                Some(_) => None,
                None => {
                    let active = oxidemx_shared::theme::Theme::load(&state.config.theme)
                        .unwrap_or_else(|| {
                            oxidemx_shared::theme::Theme::load(
                                &oxidemx_shared::theme::ThemeName::CatppuccinMocha,
                            )
                            .expect("bundled mocha")
                        });
                    Some(ThemeEditor {
                        working: active.colors.clone(),
                        original: active.colors.clone(),
                        is_dark: active.is_dark,
                        slug: String::new(),
                        editing_field: None,
                    })
                }
            };
            Task::none()
        }
        Message::RevertCustomTheme => {
            if let Some(editor) = state.theme_editor.as_mut() {
                editor.working = editor.original.clone();
                let preview = oxidemx_shared::theme::Theme {
                    name: "(custom)".into(),
                    description: String::new(),
                    is_dark: editor.is_dark,
                    radial_image: None,
                    radial_params: None,
                    colors: editor.working.clone(),
                };
                state.palette = palette::Palette::from_theme(&preview);
                state.touch();
            }
            Task::none()
        }
        Message::SetThemeColor { field, value } => {
            if let Some(editor) = state.theme_editor.as_mut() {
                set_theme_color_field(&mut editor.working, &field, value);
                // Live-preview the WIP palette on the running UI.
                let preview = oxidemx_shared::theme::Theme {
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
        Message::ToggleThemeColorPicker(field) => {
            if let Some(editor) = state.theme_editor.as_mut() {
                editor.editing_field = match editor.editing_field.take() {
                    Some(cur) if cur == field => None,
                    _ => Some(field),
                };
            }
            Task::none()
        }
        Message::SetThemeColorSv { field, s, v } => {
            apply_hsv_change(state, &field, |h, _, _| (h, s, v));
            Task::none()
        }
        Message::SetThemeColorHue { field, h } => {
            apply_hsv_change(state, &field, |_, s, v| {
                // Greyscale → bring saturation up so the picked hue
                // actually shows on screen. Otherwise dragging the
                // hue strip on a black/white field would feel inert.
                let new_s = if s < 0.001 { 1.0 } else { s };
                let new_v = if v < 0.001 { 1.0 } else { v };
                (h, new_s, new_v)
            });
            Task::none()
        }
        Message::SetThemeColorChannel {
            field,
            channel,
            value,
        } => {
            if let Some(editor) = state.theme_editor.as_mut() {
                let current = theme_field_value(&editor.working, &field);
                let (mut r, mut g, mut b) = parse_hex_channels(&current);
                match channel {
                    ColorChannel::Red => r = value,
                    ColorChannel::Green => g = value,
                    ColorChannel::Blue => b = value,
                }
                let hex = format!("#{r:02x}{g:02x}{b:02x}");
                set_theme_color_field(&mut editor.working, &field, hex);
                let preview = oxidemx_shared::theme::Theme {
                    name: "(custom)".into(),
                    description: String::new(),
                    is_dark: editor.is_dark,
                    radial_image: None,
                    radial_params: None,
                    colors: editor.working.clone(),
                };
                state.palette = palette::Palette::from_theme(&preview);
                state.touch();
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
                let theme = oxidemx_shared::theme::Theme {
                    name: editor.slug.trim().to_string(),
                    description: "User-customised theme".into(),
                    is_dark: editor.is_dark,
                    radial_image: None,
                    radial_params: None,
                    colors: editor.working.clone(),
                };
                let result = oxidemx_shared::theme::save_user_theme(&slug, &theme)
                    .map(|_| slug)
                    .map_err(|e| e.to_string());
                return Task::perform(async move { result }, Message::CustomThemeSaved);
            }
            Task::none()
        }
        Message::CustomThemeSaved(Ok(slug)) => {
            state.config.theme = oxidemx_shared::theme::ThemeName::from(slug.as_str());
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
                slice.submenu.push(oxidemx_shared::Slice {
                    action_id: None,
                    label: "New item".into(),
                    kind: oxidemx_shared::ActionKind::Exec,
                    command: String::new(),
                    color: "accent".into(),
                    icon: String::new(),
                    submenu: Vec::new(),
                    visible_if: None,
                    icon_untinted: false,
                    description: String::new(),
                    widget: None,
                    dial: None,
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
            state
                .config
                .radial_menu
                .pages
                .push(oxidemx_shared::RadialPage {
                    name: format!("Page {}", state.config.radial_menu.pages.len() + 1),
                    slices: Vec::new(),
                    app_classes: Vec::new(),
                    include_in_scroll: true,
                    slot_count: 8,
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
        Message::SetPageSlotCount { page, count } => {
            if let Some(p) = state.config.radial_menu.pages.get_mut(page) {
                p.slot_count = count.clamp(2, 8);
                // Slices beyond the new slot count just don't
                // render — keep them in the page so re-bumping the
                // count later doesn't lose work.
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
                let copy = oxidemx_shared::RadialPage {
                    name: format!("{} (copy)", src.name),
                    slices: src.slices,
                    // Clear app_classes on the copy: two pages
                    // matching the same class would create an
                    // ambiguous auto-select. The user can re-add
                    // app classes deliberately.
                    app_classes: Vec::new(),
                    include_in_scroll: src.include_in_scroll,
                    slot_count: src.slot_count,
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
            state.status =
                format!("Switch to your target app — sampling focused window in {delay_secs}s…");
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
                                let icon = oxidemx_icons::rasterize_icon_untinted(&name, size);
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
        Message::OpenAnimationEditor(el) => {
            state.animation_editor = Some(animation_editor::AnimationEditorState::new(el));
            Task::none()
        }
        Message::CloseAnimationEditor => {
            state.animation_editor = None;
            Task::none()
        }
        Message::AnimationEditorSelectTrack(direction, idx) => {
            if let Some(editor) = state.animation_editor.as_mut() {
                editor.selected = Some((direction, idx));
            }
            Task::none()
        }
        Message::AnimationEditorAddTrack(direction, kind_name) => {
            let element = match state.animation_editor.as_ref() {
                Some(e) => e.element,
                None => return Task::none(),
            };
            let new_track = oxidemx_shared::AnimationTrack {
                kind: oxidemx_shared::TrackKind::default_for(kind_name),
                ..Default::default()
            };
            let anim = animation_editor::element_animation_mut(state, element);
            let cfg = match direction {
                animation_editor::AnimEditorDirection::Enter => &mut anim.enter,
                animation_editor::AnimEditorDirection::Exit => &mut anim.exit,
            };
            cfg.custom_tracks.push(new_track);
            // Auto-select the new track so the parameter editor
            // appears immediately — saves the user a click.
            let new_idx = cfg.custom_tracks.len() - 1;
            if let Some(editor) = state.animation_editor.as_mut() {
                editor.selected = Some((direction, new_idx));
            }
            state.touch();
            Task::none()
        }
        Message::AnimationEditorDeleteTrack(direction, idx) => {
            let element = match state.animation_editor.as_ref() {
                Some(e) => e.element,
                None => return Task::none(),
            };
            let anim = animation_editor::element_animation_mut(state, element);
            let cfg = match direction {
                animation_editor::AnimEditorDirection::Enter => &mut anim.enter,
                animation_editor::AnimEditorDirection::Exit => &mut anim.exit,
            };
            if idx < cfg.custom_tracks.len() {
                cfg.custom_tracks.remove(idx);
            }
            // Clear / shift the selection so it doesn't dangle.
            if let Some(editor) = state.animation_editor.as_mut() {
                if let Some((sel_dir, sel_idx)) = editor.selected {
                    if sel_dir == direction {
                        if sel_idx == idx {
                            editor.selected = None;
                        } else if sel_idx > idx {
                            editor.selected = Some((sel_dir, sel_idx - 1));
                        }
                    }
                }
            }
            state.touch();
            Task::none()
        }
        Message::AnimationEditorChangeKind(direction, idx, kind_name) => {
            let element = match state.animation_editor.as_ref() {
                Some(e) => e.element,
                None => return Task::none(),
            };
            let anim = animation_editor::element_animation_mut(state, element);
            let cfg = match direction {
                animation_editor::AnimEditorDirection::Enter => &mut anim.enter,
                animation_editor::AnimEditorDirection::Exit => &mut anim.exit,
            };
            if let Some(track) = cfg.custom_tracks.get_mut(idx) {
                track.kind = oxidemx_shared::TrackKind::default_for(kind_name);
            }
            state.touch();
            Task::none()
        }
        Message::AnimationEditorSetParam(direction, idx, param) => {
            let element = match state.animation_editor.as_ref() {
                Some(e) => e.element,
                None => return Task::none(),
            };
            let anim = animation_editor::element_animation_mut(state, element);
            let cfg = match direction {
                animation_editor::AnimEditorDirection::Enter => &mut anim.enter,
                animation_editor::AnimEditorDirection::Exit => &mut anim.exit,
            };
            if let Some(track) = cfg.custom_tracks.get_mut(idx) {
                animation_editor::apply_track_param(track, param);
            }
            state.touch();
            Task::none()
        }
        Message::AnimationEditorSetEasingKind(direction, idx, opt) => {
            let element = match state.animation_editor.as_ref() {
                Some(e) => e.element,
                None => return Task::none(),
            };
            let anim = animation_editor::element_animation_mut(state, element);
            let cfg = match direction {
                animation_editor::AnimEditorDirection::Enter => &mut anim.enter,
                animation_editor::AnimEditorDirection::Exit => &mut anim.exit,
            };
            if let Some(track) = cfg.custom_tracks.get_mut(idx) {
                track.easing = match opt {
                    animation_editor::EasingPickOption::Linear => oxidemx_shared::Easing::Linear,
                    animation_editor::EasingPickOption::EaseIn => oxidemx_shared::Easing::EaseIn,
                    animation_editor::EasingPickOption::EaseOut => oxidemx_shared::Easing::EaseOut,
                    animation_editor::EasingPickOption::EaseInOut => {
                        oxidemx_shared::Easing::EaseInOut
                    }
                    animation_editor::EasingPickOption::Spring => {
                        // Preserve old stiffness/damping if already
                        // a spring; otherwise use Motion.dev "gentle"
                        // defaults.
                        if let oxidemx_shared::Easing::Spring { .. } = track.easing {
                            track.easing
                        } else {
                            oxidemx_shared::Easing::Spring {
                                stiffness: 180.0,
                                damping: 14.0,
                            }
                        }
                    }
                };
            }
            state.touch();
            Task::none()
        }
        Message::AnimationEditorReset => {
            let element = match state.animation_editor.as_ref() {
                Some(e) => e.element,
                None => return Task::none(),
            };
            let anim = animation_editor::element_animation_mut(state, element);
            anim.enter.custom_tracks.clear();
            anim.exit.custom_tracks.clear();
            if let Some(editor) = state.animation_editor.as_mut() {
                editor.selected = None;
            }
            state.touch();
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
        Message::PickAppForCommand {
            command,
            icon,
            label,
        } => {
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
                        slice.kind = oxidemx_shared::ActionKind::Exec;
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
                        item.kind = oxidemx_shared::ActionKind::Exec;
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
                    let mut candidates: Vec<&str> = icon_picker::COMMON_ICONS.to_vec();
                    for r in state.recent_icons.iter() {
                        if !candidates.contains(&r.as_str()) {
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
                                        let icon =
                                            oxidemx_icons::rasterize_icon(&name, size, color);
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
                                        let icon =
                                            oxidemx_icons::rasterize_icon_untinted(&name, size);
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
        Message::DetectedFocusedClass {
            page,
            generation,
            class,
        } => {
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

        // ====================================================================
        // Indicator Popup handlers
        // ====================================================================
        Message::SetPopupMode(mode) => {
            state.config.popup.mode = mode;
            state.touch();
            Task::none()
        }
        Message::SetPopupShowHostButtons(on) => {
            state.config.popup.show_host_buttons = on;
            state.touch();
            Task::none()
        }
        Message::SetPopupHostLabelStyle(style) => {
            state.config.popup.host_label_style = style;
            state.touch();
            Task::none()
        }
        Message::PopupToggleMoveUp(id) => {
            let list = if state.config.popup.mode == oxidemx_shared::PopupMode::Simple {
                &mut state.config.popup.simple_toggles
            } else {
                &mut state.config.popup.power_toggles
            };
            oxidemx_shared::PopupConfig::move_up(list, &id);
            state.touch();
            Task::none()
        }
        Message::PopupToggleMoveDown(id) => {
            let list = if state.config.popup.mode == oxidemx_shared::PopupMode::Simple {
                &mut state.config.popup.simple_toggles
            } else {
                &mut state.config.popup.power_toggles
            };
            oxidemx_shared::PopupConfig::move_down(list, &id);
            state.touch();
            Task::none()
        }
        Message::PopupToggleRemove(id) => {
            let list = if state.config.popup.mode == oxidemx_shared::PopupMode::Simple {
                &mut state.config.popup.simple_toggles
            } else {
                &mut state.config.popup.power_toggles
            };
            list.retain(|x| x != &id);
            state.touch();
            Task::none()
        }
        Message::PopupToggleAdd(id) => {
            if !oxidemx_shared::QUICK_TOGGLE_CATALOG
                .iter()
                .any(|q| q.id == id)
            {
                warn!("PopupToggleAdd: unknown id {id:?} — not in QUICK_TOGGLE_CATALOG");
                return Task::none();
            }
            let list = if state.config.popup.mode == oxidemx_shared::PopupMode::Simple {
                &mut state.config.popup.simple_toggles
            } else {
                &mut state.config.popup.power_toggles
            };
            if !list.iter().any(|x| x == &id) {
                list.push(id);
                state.touch();
            }
            Task::none()
        }
        Message::PopupSliderMoveUp(id) => {
            oxidemx_shared::PopupConfig::move_up(&mut state.config.popup.power_sliders, &id);
            state.touch();
            Task::none()
        }
        Message::PopupSliderMoveDown(id) => {
            oxidemx_shared::PopupConfig::move_down(&mut state.config.popup.power_sliders, &id);
            state.touch();
            Task::none()
        }
        Message::PopupSliderRemove(id) => {
            state.config.popup.power_sliders.retain(|x| x != &id);
            state.touch();
            Task::none()
        }
        Message::PopupSliderAdd(id) => {
            if !oxidemx_shared::QUICK_SLIDER_CATALOG
                .iter()
                .any(|q| q.id == id)
            {
                warn!("PopupSliderAdd: unknown id {id:?} — not in QUICK_SLIDER_CATALOG");
                return Task::none();
            }
            if !state.config.popup.power_sliders.iter().any(|x| x == &id) {
                state.config.popup.power_sliders.push(id);
                state.touch();
            }
            Task::none()
        }
        Message::SetPopupVolumeOnScroll(on) => {
            state.config.popup.volume_on_scroll = on;
            state.touch();
            Task::none()
        }
        Message::SetPopupCloseOnAction(on) => {
            state.config.popup.close_on_action = on;
            state.touch();
            Task::none()
        }
        Message::SetPopupAnimations(on) => {
            state.config.popup.animations = on;
            state.touch();
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
    let back_btn = iced::widget::button(
        row![icon("arrow-left", 12.0, pal.text), text("Back").size(12)]
            .spacing(5)
            .align_y(iced::Alignment::Center),
    )
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
    let body: Element<Message> = if let Some(store) = state.widget_store.as_ref() {
        // The store wins over the other panels — it can be opened
        // from inside the slice picker ("Get more widgets…") and
        // from the missing-widget chip's Reinstall button.
        full_panel(
            state,
            "Get more widgets",
            widget_store::view(state, store),
            Message::CloseWidgetStore,
        )
    } else if let Some(p) = state.app_command_picker.as_ref() {
        full_panel(
            state,
            "Pick app for command",
            app_picker::view(state, p),
            Message::CloseAppCommandPicker,
        )
    } else if let Some(p) = state.icon_picker.as_ref() {
        full_panel(
            state,
            "Pick an icon",
            icon_picker::view(state, p),
            Message::CloseIconPicker,
        )
    } else if let Some(editor) = state.theme_editor.as_ref() {
        full_panel(
            state,
            "Customise theme",
            theme_customiser::view(state, editor),
            Message::ToggleThemeCustomiser,
        )
    } else if let Some(editor) = state.animation_editor.as_ref() {
        full_panel(
            state,
            "Custom animation editor",
            animation_editor::view(state, editor),
            Message::CloseAnimationEditor,
        )
    } else {
        match state.tab {
            Tab::MouseButtons => tabs::mouse_buttons::view(state),
            Tab::Menu => tabs::buttons::view(state),
            Tab::Ai => tabs::ai::view(state),
            Tab::Agents => tabs::agents::view(state),
            Tab::Settings => tabs::settings_page::view(state),
            Tab::PointScroll => tabs::scroll::view(state),
            Tab::IndicatorPopup => tabs::indicator_popup::view(state),
            Tab::Haptic => tabs::haptics::view(state),
            Tab::Devices => tabs::devices::view(state),
            Tab::EasySwitch => tabs::easyswitch::view(state),
            Tab::Flow => tabs::placeholder::view(
                state,
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
            scrollable(container(body).padding(20)).style(style::scrollable_style(&state.palette))
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
            text("OxideMX").size(20),
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
    // Tint the symbolic icon with the active accent when the row
    // is highlighted, otherwise the regular text colour. Same
    // alpha-mask rasterisation as the slice icons → the icon
    // colour follows whichever theme the user picked.
    let tint = if active { pal.accent } else { pal.text };
    let icon_size: f32 = 16.0;
    let icon_handle = crate::radial_preview::resolve_icon_handle(
        &state.icons,
        &state.iced_handles,
        tab.icon_name(),
        icon_size as u32,
        tint,
    );
    let icon_widget: Element<Message> = match icon_handle {
        Some(h) => iced::widget::image(h)
            .width(Length::Fixed(icon_size))
            .height(Length::Fixed(icon_size))
            .into(),
        // Theme doesn't ship the symbolic — fall back to the
        // single-letter mnemonic so the sidebar always renders
        // something at the same column width.
        None => container(text(tab.glyph()).size(13))
            .center_x(Length::Fixed(icon_size))
            .center_y(Length::Fixed(icon_size))
            .into(),
    };

    let mut inner = row![icon_widget, text(tab.label()).size(13)]
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
        // Compute the fade-out alpha. The toast is fully opaque
        // for `STATUS_LIFETIME - STATUS_FADE_TAIL`, then eases
        // smoothly to 0 over the tail. Once past `STATUS_LIFETIME`
        // the auto-clear has already wiped the string so this
        // branch isn't reached.
        let alpha = match state.status_set_at {
            Some(t) => {
                let elapsed = t.elapsed();
                if elapsed + STATUS_FADE_TAIL >= STATUS_LIFETIME {
                    let into_tail = elapsed
                        .saturating_sub(STATUS_LIFETIME - STATUS_FADE_TAIL)
                        .as_secs_f32();
                    let normalised = (into_tail / STATUS_FADE_TAIL.as_secs_f32()).clamp(0.0, 1.0);
                    // 1 - t^3: linger near full alpha for most of
                    // the tail (~88 % visible at the halfway mark),
                    // then accelerate the decay. Reads as a soft
                    // fade rather than a long blur.
                    1.0 - normalised * normalised * normalised
                } else {
                    1.0
                }
            }
            None => 1.0,
        };
        let accent = pal.accent;
        text(state.status.as_str())
            .size(11)
            .style(move |_| iced::widget::text::Style {
                color: Some(iced::Color {
                    a: accent.a * alpha,
                    ..accent
                }),
            })
            .into()
    } else {
        text("Idle.").size(11).style(style::text_faint(pal)).into()
    };

    container(
        row![
            text("OxideMX · Free & open source software · original concept by JuhLabs")
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

fn subscription(state: &State) -> Subscription<Message> {
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
        // Window width feeds the picker grid's 3-up / 4-up split.
        iced::window::resize_events().map(|(_id, size)| Message::WindowResized(size.width)),
    ];
    // Shortcut-capture subscription — only active while the user
    // has armed a Capture button. Listens for keyboard events,
    // formats the chord, and emits ShortcutCaptured. Esc cancels.
    if state.capturing_shortcut.is_some() {
        subs.push(iced::event::listen_with(shortcut_capture_filter));
    }
    // Faster tick during the status auto-fade tail so the alpha
    // ramp renders smoothly (~30 fps) instead of stepping along
    // the 200 ms SaveTick. Gated so we don't pay the 50 ms wakeup
    // cost when no status is on screen.
    let in_fade = state
        .status_set_at
        .map(|t| {
            let e = t.elapsed();
            !state.status.is_empty()
                && e + STATUS_FADE_TAIL >= STATUS_LIFETIME
                && e <= STATUS_LIFETIME
        })
        .unwrap_or(false);
    if in_fade {
        subs.push(iced::time::every(Duration::from_millis(50)).map(|_| Message::StatusFadeTick));
    }
    if FOCUS_RX.get().is_some() {
        // The singleton handshake gave us a receiver — wire it in
        // so subsequent `oxidemx-settings` invocations call
        // `Focus` on us and the running window pops to the front.
        // iced::Subscription::run takes a fn() pointer (no
        // captures), so the stream builder reads the receiver out
        // of the OnceLock.
        subs.push(Subscription::run(focus_subscription_builder).map(|_| Message::Focus));
    }
    Subscription::batch(subs)
}

/// Filter every runtime event for keyboard presses while the user
/// is mid-capture. Returns `Some(ShortcutCaptured(chord))` when a
/// non-modifier key arrives (so the user releasing only Shift
/// won't accidentally save "+shift"), `Some(CancelShortcutCapture)`
/// on Esc, and `None` otherwise. Modifier-only presses are
/// ignored so the user can hold Ctrl+Shift before pressing the
/// final key.
fn shortcut_capture_filter(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::{key, Event as KbdEvent, Key};
    let (key, modifiers) = match event {
        iced::Event::Keyboard(KbdEvent::KeyPressed { key, modifiers, .. }) => (key, modifiers),
        _ => return None,
    };
    // Esc cancels.
    if matches!(key, Key::Named(key::Named::Escape)) {
        return Some(Message::CancelShortcutCapture);
    }
    // Ignore bare modifier presses — wait for a real key.
    if matches!(
        key,
        Key::Named(key::Named::Control)
            | Key::Named(key::Named::Shift)
            | Key::Named(key::Named::Alt)
            | Key::Named(key::Named::Super)
            | Key::Named(key::Named::Meta)
    ) {
        return None;
    }
    let key_label = match &key {
        Key::Character(c) => c.to_lowercase(),
        Key::Named(n) => named_key_label(*n)?,
        Key::Unidentified => return None,
    };
    let mut parts: Vec<&str> = Vec::with_capacity(5);
    if modifiers.control() {
        parts.push("ctrl");
    }
    if modifiers.alt() {
        parts.push("alt");
    }
    if modifiers.shift() {
        parts.push("shift");
    }
    if modifiers.logo() {
        parts.push("super");
    }
    parts.push(&key_label);
    Some(Message::ShortcutCaptured(parts.join("+")))
}

/// Map iced's `Named` key enum to the xdotool/ydotool name the
/// daemon expects. Returns None for keys that don't have a
/// sensible mapping (e.g. dead keys, lock keys) — those are
/// silently dropped so the capture session keeps waiting for a
/// usable chord.
fn named_key_label(n: iced::keyboard::key::Named) -> Option<String> {
    use iced::keyboard::key::Named::*;
    let s = match n {
        Enter => "Return",
        Tab => "Tab",
        Space => "space",
        Backspace => "BackSpace",
        Delete => "Delete",
        Insert => "Insert",
        Home => "Home",
        End => "End",
        PageUp => "Prior",
        PageDown => "Next",
        ArrowUp => "Up",
        ArrowDown => "Down",
        ArrowLeft => "Left",
        ArrowRight => "Right",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        _ => return None,
    };
    Some(s.to_string())
}

fn focus_subscription_builder() -> impl futures_util::stream::Stream<Item = ()> {
    let rx = FOCUS_RX
        .get()
        .cloned()
        .expect("FOCUS_RX present; checked in subscription()");
    singleton::focus_stream(rx)
}

/// Headless config sanity check (`--check-config`): load the same
/// config + widget registry the GUI would, print a short summary,
/// exit 0/1. Used by `scripts/settings-widget-demo.sh` to validate
/// a seeded XDG_CONFIG_HOME without launching a window.
fn check_config() -> ! {
    let Some(path) = oxidemx_shared::config::default_config_path() else {
        eprintln!("check-config: cannot resolve a config path (no HOME?)");
        std::process::exit(1);
    };
    let cfg = match AppConfig::load_from(&path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("check-config: {} failed to load: {e}", path.display());
            std::process::exit(1);
        }
    };
    println!("config: {}", path.display());
    for (pi, page) in cfg.radial_menu.pages.iter().enumerate() {
        println!("page {pi} {:?}: {} slices", page.name, page.slices.len());
        for (si, slice) in page.slices.iter().enumerate() {
            if slice.kind == oxidemx_shared::ActionKind::Widget {
                let source = slice
                    .widget
                    .as_ref()
                    .map(|w| format!("{:?}", w.source))
                    .unwrap_or_else(|| "<none>".into());
                let ikey = slice
                    .widget
                    .as_ref()
                    .and_then(|w| w.instance_key.as_deref())
                    .unwrap_or("-");
                println!("  slot {si}: widget {source} instance_key={ikey}");
            }
        }
    }
    println!(
        "widget bags: {} global, {} instance",
        cfg.widgets.global.len(),
        cfg.widgets.instances.len()
    );
    let (registry, _) = tabs::buttons::picker::scan_registry_full();
    for w in &registry {
        println!(
            "installed: {} v{} by {} ({})",
            w.id,
            w.version,
            w.author,
            if w.ready { "ready" } else { "incompatible" }
        );
    }
    std::process::exit(0);
}

fn main() -> iced::Result {
    // Seed bundled built-in widgets BEFORE the startup registry scan
    // (`State::default` → `scan_registry_full`) so the picker is
    // populated even if the overlay never ran (spec §16). Runs ahead
    // of --check-config too — the demo script validates the seeded
    // registry through that path. Logged with eprintln: tracing isn't
    // initialised yet and check-config prints to stdio anyway.
    match oxidemx_widget_cli::seed_builtin_widgets() {
        Ok(outcomes) => {
            for o in &outcomes {
                eprintln!("builtin widget seed: {o}");
            }
        }
        Err(e) => eprintln!("builtin widget seeding failed: {e}"),
    }

    // Headless config check for scripts — no window, no singleton.
    if std::env::args().any(|a| a == "--check-config") {
        check_config();
    }

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
            FOCUS_RX.set(rx).map_err(|_| ()).expect("FOCUS_RX set once");
        }
        singleton::Acquisition::SecondaryFocused => {
            info!("Existing settings instance focused; exiting.");
            return Ok(());
        }
        singleton::Acquisition::BusUnavailable => {
            warn!("Session bus unavailable; running without singleton enforcement.");
        }
    }

    let mut window = iced::window::Settings {
        size: INITIAL_WINDOW_SIZE,
        ..Default::default()
    };
    window.platform_specific.application_id = "org.oxidemx.settings".into();

    iced::application(boot, update, view)
        .title("OxideMX Settings")
        .window(window)
        .theme(|state: &State| {
            // Build an iced custom theme from our app palette so
            // built-in widgets (sliders, togglers, default-styled
            // buttons, pick_list highlights) automatically follow
            // the active accent + surface colours instead of
            // falling back to iced's stock blue.
            //
            // Only the six core slots (background, text, primary,
            // success, warning, danger) flow through; widgets that
            // need finer control still go through our `style::*`
            // helpers, but those that don't will at least pick up
            // the accent and surface colour from this theme.
            let pal = &state.palette;
            iced::Theme::custom(
                if pal.is_dark {
                    "OxideMX Dark"
                } else {
                    "OxideMX Light"
                },
                iced::theme::Palette {
                    background: pal.base,
                    text: pal.text,
                    primary: pal.accent,
                    success: pal.success,
                    warning: pal.warning,
                    danger: pal.danger,
                },
            )
        })
        .subscription(subscription)
        .run()
}

// ============================================================================
// Tests — end-to-end widget message flow (Plan 3 Task 4)
// ============================================================================

/// Drives `update()` through the real picker → options-card message
/// sequence against a temp `XDG_CONFIG_HOME` holding an installed
/// (dummy-wasm) weather widget, then persists through the real save
/// path and re-loads. The GUI walk this replaces is documented in
/// `scripts/settings-widget-demo.sh`.
#[cfg(test)]
mod widget_flow_tests {
    use super::*;
    use oxidemx_shared::{ActionKind, WidgetScope, WidgetSource};
    use serde_json::json;

    /// Minimal but valid weather manifest (mirrors
    /// `widgets/builtin/weather/widget.json` where it matters:
    /// id, options incl. location/enum/select with defaults).
    const WEATHER_MANIFEST: &str = r#"{
      "id": "weather",
      "name": "Weather",
      "version": "1.4.0",
      "author": "JuhLabs",
      "api_version": 1,
      "entry": "widget.wasm",
      "icon": "icon.svg",
      "permissions": ["net:api.open-meteo.com"],
      "slice": { "refresh_ms": 900000, "fallback_icon": "weather-clear-symbolic" },
      "options": [
        { "key": "location", "type": "location", "label": "Location", "required": true },
        { "key": "units",    "type": "enum",   "label": "Units",
          "values": ["c", "f"], "default": "c" },
        { "key": "refresh",  "type": "select", "label": "Refresh",
          "values": [300, 900, 1800, 3600], "default": 900, "unit": "s" }
      ]
    }"#;

    fn plain_slice(label: &str) -> oxidemx_shared::Slice {
        oxidemx_shared::Slice {
            action_id: None,
            label: label.to_string(),
            kind: ActionKind::Exec,
            command: "true".into(),
            color: "accent".into(),
            icon: String::new(),
            submenu: Vec::new(),
            visible_if: None,
            icon_untinted: false,
            description: String::new(),
            widget: None,
            dial: None,
        }
    }

    /// One temp config home with the dummy weather widget installed.
    /// Returned guard removes the tree on drop.
    struct TempConfigHome(std::path::PathBuf);
    impl Drop for TempConfigHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn install_temp_home() -> TempConfigHome {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "oxidemx-widget-flow-{}-{stamp}",
            std::process::id()
        ));
        let widget_dir = root.join("oxidemx/widgets/weather");
        std::fs::create_dir_all(&widget_dir).expect("mkdir widget dir");
        std::fs::write(widget_dir.join("widget.json"), WEATHER_MANIFEST).unwrap();
        std::fs::write(
            widget_dir.join("icon.svg"),
            "<svg xmlns='http://www.w3.org/2000/svg'/>",
        )
        .unwrap();
        // Registry scan only checks the entry file *exists* — wasm is
        // never loaded by the settings app, so a stub byte suffices.
        std::fs::write(widget_dir.join("widget.wasm"), b"\0asm").unwrap();
        // The whole flow (config path + registry scan) keys off
        // XDG_CONFIG_HOME, which `State::default()` reads at build
        // time below.
        std::env::set_var("XDG_CONFIG_HOME", &root);
        TempConfigHome(root)
    }

    /// The full spec §10 walk, headless: OpenPicker → PickWidget →
    /// option edits (incl. the geocoder's WidgetLocPick path) →
    /// scope flip → reset → persist → reload → resolution.
    #[tokio::test(flavor = "current_thread")]
    async fn widget_flow_end_to_end() {
        let home = install_temp_home();

        let mut state = State::default();
        assert!(
            state
                .widget_registry
                .iter()
                .any(|w| w.id == "weather" && w.ready),
            "temp-home weather widget must scan as Ready (got {:?})",
            state.widget_registry
        );
        assert!(state.widget_manifests.contains_key("weather"));

        // Ensure slot 4 exists on the active page.
        while state.active_slices_mut().len() < 5 {
            let n = state.active_slices_mut().len();
            let s = plain_slice(&format!("S{n}"));
            state.active_slices_mut().push(s);
        }
        // Slot 4 starts as a plain exec slice with an auto-labelable
        // (empty) label so PickWidget's relabel rule applies.
        state.active_slices_mut()[4] = plain_slice("");
        let page_name = state.config.radial_menu.pages[state.active_page]
            .name
            .clone();
        let expected_ikey = oxidemx_shared::widgets::instance_key(&page_name, 4);

        // --- picker: open + pick the installed weather widget ---
        let _ = update(&mut state, Message::OpenPicker(4));
        assert_eq!(state.picker_open, Some(4));
        assert!(state.picker_undo.is_some(), "undo snapshot taken on open");

        let _ = update(
            &mut state,
            Message::PickWidget(4, WidgetSource::Custom("weather".into())),
        );
        assert_eq!(state.picker_open, None, "pick applies + collapses");
        {
            let slice = &state.active_slices()[4];
            assert_eq!(slice.kind, ActionKind::Widget);
            let w = slice.widget.as_ref().expect("widget config set");
            assert_eq!(w.source, WidgetSource::Custom("weather".into()));
            assert_eq!(w.scope, WidgetScope::Instance);
            assert_eq!(w.instance_key.as_deref(), Some(expected_ikey.as_str()));
            assert_eq!(slice.label, "Weather", "auto-label from the registry name");
        }

        // --- options card edits (Instance scope) ---
        // Location lands through the geocoder pick message.
        let _ = update(
            &mut state,
            Message::WidgetLocPick {
                slice: 4,
                key: "location".into(),
                name: "Oslo".into(),
                lat: 59.91,
                lon: 10.75,
            },
        );
        let _ = update(
            &mut state,
            Message::SetWidgetOption {
                slice: 4,
                key: "units".into(),
                value: json!("f"),
            },
        );
        let inst_bag = &state.config.widgets.instances[&expected_ikey]["weather"];
        assert_eq!(
            inst_bag["location"],
            json!({ "name": "Oslo", "lat": 59.91, "lon": 10.75 })
        );
        assert_eq!(inst_bag["units"], json!("f"));
        assert!(state.config.widgets.global.is_empty());

        // --- scope flip → Global: pointer flips, instance bag KEPT ---
        let _ = update(&mut state, Message::SetWidgetScope(4, WidgetScope::Global));
        assert_eq!(
            state.active_slices()[4].widget.as_ref().unwrap().scope,
            WidgetScope::Global
        );
        assert_eq!(
            state.config.widgets.instances[&expected_ikey]["weather"]["units"],
            json!("f"),
            "instance bag kept (ignored) on the Global flip"
        );

        // --- global edit lands in the global bag only ---
        let _ = update(
            &mut state,
            Message::SetWidgetOption {
                slice: 4,
                key: "units".into(),
                value: json!("c"),
            },
        );
        assert_eq!(state.config.widgets.global["weather"]["units"], json!("c"));
        assert_eq!(
            state.config.widgets.instances[&expected_ikey]["weather"]["units"],
            json!("f"),
            "instance bag untouched by a global write"
        );

        // --- resolution through the two-bag merge ---
        let defaults = state.widget_manifests["weather"].defaults();
        let global_view = state.config.widgets.resolve(
            "weather",
            Some(&expected_ikey),
            WidgetScope::Global,
            &defaults,
        );
        assert_eq!(global_view["units"], json!("c"));
        assert_eq!(
            global_view["refresh"],
            json!(900),
            "manifest default survives"
        );
        assert!(
            !global_view.contains_key("location"),
            "instance-bag location is ignored under Global scope"
        );
        let instance_view = state.config.widgets.resolve(
            "weather",
            Some(&expected_ikey),
            WidgetScope::Instance,
            &defaults,
        );
        assert_eq!(instance_view["units"], json!("f"), "instance beats global");
        assert_eq!(instance_view["location"]["name"], json!("Oslo"));

        // --- per-option reset under Global scope removes the global key ---
        let _ = update(
            &mut state,
            Message::ResetWidgetOption {
                slice: 4,
                key: "units".into(),
            },
        );
        assert!(
            state.config.widgets.global.is_empty(),
            "empty global bag is pruned after the reset"
        );

        // --- config JSON shape (what the overlay's watcher will read) ---
        let cfg_json = serde_json::to_value(&state.config).expect("config serialises");
        let slice_json = &cfg_json["radial_menu"]["pages"][state.active_page]["slices"][4];
        assert_eq!(slice_json["type"], json!("widget"));
        assert_eq!(slice_json["widget"]["source"]["custom"], json!("weather"));
        assert_eq!(slice_json["widget"]["instance_key"], json!(expected_ikey));
        assert_eq!(
            cfg_json["widgets"]["instances"][&expected_ikey]["weather"]["units"],
            json!("f")
        );

        // --- persist through the real save path + reload ---
        let path = state.config_path.clone().expect("temp config path");
        assert!(
            path.starts_with(&home.0),
            "state must point at the temp home, not the user's real config"
        );
        persist::save(path.clone(), state.config.clone())
            .await
            .expect("save");
        let reloaded = AppConfig::load_from(&path).expect("reload");
        let r = reloaded.widgets.resolve(
            "weather",
            Some(&expected_ikey),
            WidgetScope::Instance,
            &defaults,
        );
        assert_eq!(r["units"], json!("f"));
        assert_eq!(r["location"]["name"], json!("Oslo"));
        assert_eq!(r["refresh"], json!(900));
        let slice = &reloaded.radial_menu.pages[state.active_page].slices[4];
        assert_eq!(slice.kind, ActionKind::Widget);
        assert_eq!(
            slice.widget.as_ref().unwrap().instance_key.as_deref(),
            Some(expected_ikey.as_str())
        );
        assert_eq!(slice.widget.as_ref().unwrap().scope, WidgetScope::Global);

        // --- live-preview lifecycle (Task 5): the post-update sync
        // spawns a preview handle when the options card becomes
        // visible (Menu tab + custom widget slice selected) and
        // drops it on deselect. The worker itself runs inside the
        // returned Task's stream — not polled here, so no wasm
        // executes; the lifecycle is what's under test.
        let _ = update(&mut state, Message::SwitchTab(Tab::Menu));
        let _ = update(&mut state, Message::SelectSlice(4));
        {
            let p = state
                .widget_preview
                .as_ref()
                .expect("preview handle spawned for the visible options card");
            assert_eq!(p.instance.widget_id, "weather");
            assert_eq!(p.instance.instance_key, expected_ikey);
        }
        // Selecting a non-widget slice tears the preview down.
        let _ = update(&mut state, Message::SelectSlice(0));
        assert!(state.widget_preview.is_none());
        // …and so does deselecting entirely.
        let _ = update(&mut state, Message::SelectSlice(4));
        assert!(state.widget_preview.is_some());
        let _ = update(&mut state, Message::DismissSliceSelection);
        assert!(state.widget_preview.is_none());
    }
}

// ============================================================================
// Tests — slice editor selection / empty-slot flow (Plan 5)
// ============================================================================

#[cfg(test)]
mod slice_editor_tests {
    use super::*;
    use oxidemx_shared::ActionKind;

    /// `State::default()` loads whatever config the environment
    /// points at — normalize the active page to a known two-slice
    /// baseline so assertions don't depend on the host machine.
    /// In-memory only: nothing here sends `SaveTick`, so the real
    /// config file is never written.
    fn test_state() -> State {
        let mut state = State::default();
        let slices = state.active_slices_mut();
        slices.clear();
        for label in ["A", "B"] {
            let mut s = tabs::buttons::rows::empty_slice();
            s.label = label.into();
            s.kind = ActionKind::Exec;
            s.command = "true".into();
            slices.push(s);
        }
        state.selected_slice = None;
        state.reset_picker();
        state
    }

    #[test]
    fn select_empty_slot_pads_and_opens_picker() {
        let mut state = test_state();
        let _ = update(&mut state, Message::SelectSlice(5));
        // Padded up to the clicked slot with inert placeholders…
        assert_eq!(state.active_slices().len(), 6);
        assert!(state.active_slices()[2..]
            .iter()
            .all(|s| s.kind == ActionKind::None));
        // …slot selected, behavior picker auto-opened, undo armed.
        assert_eq!(state.selected_slice, Some(5));
        assert_eq!(state.picker_open, Some(5));
        assert!(state.picker_undo.is_some());
    }

    #[test]
    fn select_existing_slice_does_not_open_picker() {
        let mut state = test_state();
        let _ = update(&mut state, Message::SelectSlice(0));
        assert_eq!(state.selected_slice, Some(0));
        assert_eq!(
            state.picker_open, None,
            "configured slices keep the chip collapsed"
        );
        assert_eq!(
            state.active_slices().len(),
            2,
            "no padding when the slot exists"
        );
    }

    #[test]
    fn select_existing_placeholder_opens_picker_without_padding() {
        let mut state = test_state();
        state
            .active_slices_mut()
            .push(tabs::buttons::rows::empty_slice());
        let _ = update(&mut state, Message::SelectSlice(2));
        assert_eq!(state.active_slices().len(), 3);
        assert_eq!(state.picker_open, Some(2));
    }

    #[test]
    fn switching_slots_moves_the_expansion_and_resets_the_picker() {
        let mut state = test_state();
        let _ = update(&mut state, Message::SelectSlice(4)); // empty → picker open
        assert_eq!(state.picker_open, Some(4));
        let _ = update(&mut state, Message::SelectSlice(0)); // configured slot
        assert_eq!(state.selected_slice, Some(0));
        assert_eq!(
            state.picker_open, None,
            "slot switch resets the stale picker"
        );
    }

    #[test]
    fn pick_on_a_padded_slot_lands_in_the_config() {
        let mut state = test_state();
        let _ = update(&mut state, Message::SelectSlice(3));
        let _ = update(&mut state, Message::PickAction(3, ActionKind::Settings));
        assert_eq!(state.active_slices()[3].kind, ActionKind::Settings);
        assert_eq!(state.picker_open, None, "pick applies + collapses");
    }
}
