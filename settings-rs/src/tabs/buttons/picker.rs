//! Behavior chip + inline action/widget picker panel (spec §10b/§10c).
//!
//! The chip is the collapsed answer to "what does this slice do?" —
//! 40 px icon tile, title, one-line summary, `Change…`. Clicking
//! `Change…` expands the picker panel below it: a search field and
//! two tile groups (built-in actions, widgets) where a single click
//! applies immediately and collapses back to the chip.
//!
//! Widget metadata comes from [`WidgetSummaryLite`] — a settings-side
//! mirror of the host registry scan, built once at startup (and again
//! on `Message::RescanWidgets` after store actions).

use std::path::PathBuf;

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Alignment, Background, Border, Element, Length};
use oxidemx_shared::{ActionKind, Slice, WidgetConfig, WidgetScope, WidgetSource};
use oxidemx_widgets::{palette::Palette, style};

use crate::{Message, State};

// ============================================================================
// Registry cache
// ============================================================================

/// Settings-side mirror of one installed widget — everything the
/// picker tile and (later) the options card need, detached from the
/// host registry's lifetime so it can live in `State`.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetSummaryLite {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    /// `WidgetState::Ready` — pickable. `false` renders the tile
    /// dimmed with `reason` shown verbatim (spec §9).
    pub ready: bool,
    pub reason: Option<String>,
    /// Whether the manifest declares `options[]` — drives the gear
    /// badge on the tile and the options card (Task 2).
    pub has_options: bool,
    /// Absolute path to the bundled icon, when present on disk.
    pub icon_path: Option<PathBuf>,
    /// Human-readable signing status: "registry", "key <fp>",
    /// or "unsigned". Informational only.
    pub signature: String,
}

/// Scan `~/.config/oxidemx/widgets` into the settings-side caches:
/// lite summaries for the picker tiles + full manifests per widget
/// id for the options card (which renders from `manifest.options`,
/// intentionally omitted from the lite summary). Called once at
/// startup and on `Message::RescanWidgets`.
pub fn scan_registry_full() -> (
    Vec<WidgetSummaryLite>,
    std::collections::HashMap<String, oxidemx_widget_proto::WidgetManifest>,
) {
    use oxidemx_widget_host::registry::{SignatureState, WidgetRegistry, WidgetState};
    let Some(dir) = WidgetRegistry::widgets_dir() else {
        return (Vec::new(), std::collections::HashMap::new());
    };
    let reg = WidgetRegistry::scan(&dir);
    let manifests = reg
        .iter()
        .map(|w| (w.manifest.id.clone(), w.manifest.clone()))
        .collect();
    let summaries = reg
        .iter()
        .map(|w| {
            let (ready, reason) = match &w.state {
                WidgetState::Ready => (true, None),
                WidgetState::Incompatible { reason } => (false, Some(reason.clone())),
            };
            let icon_path = {
                let p = w.dir.join(&w.manifest.icon);
                p.is_file().then_some(p)
            };
            WidgetSummaryLite {
                id: w.manifest.id.clone(),
                name: w.manifest.name.clone(),
                version: w.manifest.version.clone(),
                author: w.manifest.author.clone(),
                ready,
                reason,
                has_options: !w.manifest.options.is_empty(),
                icon_path,
                signature: match &w.signature_state {
                    SignatureState::Pinned => "registry".to_string(),
                    SignatureState::Unknown(fp) => format!("key {fp}"),
                    SignatureState::Unsigned => "unsigned".to_string(),
                },
            }
        })
        .collect();
    (summaries, manifests)
}

// ============================================================================
// Pure pick / undo / label helpers (unit-tested below)
// ============================================================================

/// What a picker tile applies when clicked. Used for both the
/// "current selection" ring and the undo-by-reselect match.
#[derive(Debug, Clone, PartialEq)]
pub enum PickChoice {
    Action(ActionKind),
    Widget(WidgetSource),
}

/// Does this tile describe the slice's current behavior? Drives the
/// accent ring + ✓ on the matching tile and the undo restore (a
/// re-pick of the snapshot's behavior restores the full snapshot).
pub fn pick_matches_slice(slice: &Slice, choice: &PickChoice) -> bool {
    match choice {
        PickChoice::Action(k) => slice.kind == *k,
        PickChoice::Widget(src) => {
            slice.kind == ActionKind::Widget
                && slice.widget.as_ref().map(|w| &w.source) == Some(src)
        }
    }
}

/// Case-insensitive substring match over any of `fields`.
/// Empty query matches everything.
pub fn matches_search(query: &str, fields: &[&str]) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    fields.iter().any(|f| f.to_lowercase().contains(&q))
}

/// Auto-label rule (spec §10c): replace the slice label with the
/// picked widget's name only when the label is empty or still equals
/// the auto-label from the previous pick — never clobber a label the
/// user typed themselves.
pub fn should_auto_label(current: &str, prev_auto: Option<&str>) -> bool {
    current.trim().is_empty() || prev_auto.is_some_and(|p| p == current)
}

/// Build the `WidgetConfig` for a fresh widget pick. Custom widgets
/// get an `instance_key` (`<page-slug>.slot<N>`) so their per-slice
/// settings bag has an address; built-ins keep `None`.
pub fn widget_config_for_pick(source: WidgetSource, page_name: &str, slot: usize) -> WidgetConfig {
    let instance_key = match &source {
        WidgetSource::Custom(_) => Some(oxidemx_shared::widgets::instance_key(page_name, slot)),
        _ => None,
    };
    WidgetConfig {
        source,
        format: None,
        scope: WidgetScope::Instance,
        instance_key,
    }
}

/// Bundled-plugin id for a native widget source (spec §16): the six
/// converted built-ins map to the `widgets/builtin/<id>` plugins
/// seeded at startup. `MouseBattery` stays native (needs daemon
/// battery data in the host — followups.md) and `Custom` is already
/// a plugin, so both return `None`.
pub fn builtin_plugin_id(source: &WidgetSource) -> Option<&'static str> {
    match source {
        WidgetSource::Weather => Some("weather"),
        WidgetSource::Cpu => Some("cpu"),
        WidgetSource::Memory => Some("memory"),
        WidgetSource::Network => Some("network"),
        WidgetSource::Disk => Some("disk"),
        WidgetSource::TasksDue => Some("tasks"),
        WidgetSource::MouseBattery | WidgetSource::Custom(_) => None,
    }
}

/// Is the plugin `id` installed AND `Ready`? (An incompatible install
/// must not hide the canned native tile or offer conversion — the
/// native render path still works, the plugin doesn't.)
pub fn plugin_ready(registry: &[WidgetSummaryLite], id: &str) -> bool {
    registry.iter().any(|w| w.id == id && w.ready)
}

/// The canned native tiles that should still show in the picker:
/// a tile is hidden when its mapped bundled plugin is installed and
/// ready (the registry tile covers it — new picks produce Custom
/// widgets, spec §16). MouseBattery has no mapping and always shows.
pub fn visible_builtin_tiles(registry: &[WidgetSummaryLite]) -> Vec<BuiltinTile> {
    builtin_tiles()
        .into_iter()
        .filter(|t| match builtin_plugin_id(&t.source) {
            Some(id) => !plugin_ready(registry, id),
            None => true,
        })
        .collect()
}

/// `Some(plugin id)` when `slice` is a legacy NATIVE widget slice
/// whose bundled plugin replacement is installed and ready — drives
/// the "Convert" hint row under the behavior chip.
pub fn convertible_plugin_id(
    slice: &Slice,
    registry: &[WidgetSummaryLite],
) -> Option<&'static str> {
    if slice.kind != ActionKind::Widget {
        return None;
    }
    let id = builtin_plugin_id(&slice.widget.as_ref()?.source)?;
    plugin_ready(registry, id).then_some(id)
}

/// One-click legacy-slice conversion (spec §16): rewrite the native
/// `WidgetSource` to `Custom(<plugin id>)`, keeping the slice's label,
/// colour and icon untouched. Returns the plugin id, or `None` when
/// the slice isn't a convertible native widget.
///
/// * `scope` stays `Instance` and a fresh `instance_key` is assigned
///   so the per-slice settings bag has an address.
/// * `format` is dropped (set to `None`): native format strings have
///   no plugin equivalent — the plugins reproduce the default native
///   typography, which is what `format: None` rendered anyway.
/// * Weather lifts the legacy global overlay fields into the new
///   INSTANCE bag (mirroring `oxidemx_shared::migrate`'s shapes:
///   `location: {name, lat, lon}`, `units: "c"|"f"`) — but only when
///   `overlay.weather_location` is actually set, and never clobbering
///   values already present in the bag.
pub fn convert_slice_to_plugin(
    slice: &mut Slice,
    overlay: &oxidemx_shared::OverlayConfig,
    widgets: &mut oxidemx_shared::widgets::WidgetStore,
    page_name: &str,
    slot: usize,
) -> Option<String> {
    if slice.kind != ActionKind::Widget {
        return None;
    }
    let cfg = slice.widget.as_mut()?;
    let id = builtin_plugin_id(&cfg.source)?.to_string();
    let was_weather = matches!(cfg.source, WidgetSource::Weather);
    let ikey = oxidemx_shared::widgets::instance_key(page_name, slot);
    cfg.source = WidgetSource::Custom(id.clone());
    cfg.scope = WidgetScope::Instance;
    cfg.instance_key = Some(ikey.clone());
    cfg.format = None;

    if was_weather {
        if let Some((lat, lon)) = overlay.weather_location {
            let place = overlay.weather_place.clone().unwrap_or_default();
            let celsius = overlay.weather_celsius;
            let bag = widgets
                .instances
                .entry(ikey)
                .or_default()
                .entry(id.clone())
                .or_default();
            bag.entry("location".to_string()).or_insert_with(|| {
                serde_json::json!({
                    "name": place,
                    "lat": lat,
                    "lon": lon,
                })
            });
            bag.entry("units".to_string())
                .or_insert_with(|| serde_json::json!(if celsius { "c" } else { "f" }));
        }
    }
    Some(id)
}

/// Display name of a built-in widget source.
pub fn builtin_widget_name(source: &WidgetSource) -> &'static str {
    match source {
        WidgetSource::Weather => "Weather",
        WidgetSource::Cpu => "CPU usage",
        WidgetSource::Memory => "Memory",
        WidgetSource::Network => "Network rate",
        WidgetSource::Disk => "Disk free",
        WidgetSource::TasksDue => "Tasks due",
        WidgetSource::MouseBattery => "Mouse battery",
        WidgetSource::Custom(_) => "Custom widget",
    }
}

/// Display name for any source — registry name for Custom ids (the
/// raw id when uninstalled), canned names for built-ins.
pub fn widget_display_name(source: &WidgetSource, registry: &[WidgetSummaryLite]) -> String {
    match source {
        WidgetSource::Custom(id) => registry
            .iter()
            .find(|w| w.id == *id)
            .map(|w| w.name.clone())
            .unwrap_or_else(|| id.clone()),
        s => builtin_widget_name(s).to_string(),
    }
}

/// What the auto-label of the slice's *current* behavior would be —
/// `Some(widget name)` for widget slices, `None` otherwise. Compared
/// against the actual label to decide whether a new pick may relabel.
pub fn current_auto_label(slice: &Slice, registry: &[WidgetSummaryLite]) -> Option<String> {
    if slice.kind == ActionKind::Widget {
        slice
            .widget
            .as_ref()
            .map(|w| widget_display_name(&w.source, registry))
    } else {
        None
    }
}

/// Apply a (non-undo) widget pick to the slice: kind, fresh
/// `WidgetConfig` (instance_key for Custom), auto-label.
pub fn apply_widget_pick(
    slice: &mut Slice,
    source: WidgetSource,
    page_name: &str,
    slot: usize,
    registry: &[WidgetSummaryLite],
) {
    let prev_auto = current_auto_label(slice, registry);
    let new_name = widget_display_name(&source, registry);
    slice.kind = ActionKind::Widget;
    slice.widget = Some(widget_config_for_pick(source, page_name, slot));
    if should_auto_label(&slice.label, prev_auto.as_deref()) {
        slice.label = new_name;
    }
}

// ============================================================================
// Tile catalogues
// ============================================================================

/// One built-in-action tile: the kind it applies + display strings.
pub struct ActionTile {
    pub kind: ActionKind,
    pub name: &'static str,
    pub sub: &'static str,
    pub icon: &'static str,
}

/// Group 1 — every `ActionKind` from the legacy kind picker EXCEPT
/// Widget (widgets get their own group with richer tiles).
pub fn action_tiles() -> Vec<ActionTile> {
    super::KIND_OPTIONS
        .iter()
        .filter(|k| k.0 != ActionKind::Widget)
        .map(|k| {
            let (sub, icon) = action_tile_meta(k.0);
            ActionTile {
                kind: k.0,
                name: action_kind_name(k.0),
                sub,
                icon,
            }
        })
        .collect()
}

/// Display name for a kind — same strings as the legacy pick_list
/// (`KindOption`'s Display impl) so nothing renames behind the
/// user's back.
pub fn action_kind_name(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Exec => "Run command",
        ActionKind::Submenu => "Submenu",
        ActionKind::Macro => "Macro",
        ActionKind::EasySwitch => "Easy-Switch",
        ActionKind::Settings => "Open Settings",
        ActionKind::Emoji => "Emoji picker",
        ActionKind::Shortcut => "Keyboard shortcut",
        ActionKind::Widget => "Live widget",
        ActionKind::Dial => "Dial (scroll-adjust)",
        ActionKind::Power => "Power action",
        ActionKind::NightLight => "Night light toggle",
        ActionKind::MouseSetting => "Mouse quick setting",
        ActionKind::None => "Do nothing",
    }
}

/// (one-line sub, freedesktop icon name) per action kind.
fn action_tile_meta(kind: ActionKind) -> (&'static str, &'static str) {
    match kind {
        ActionKind::Exec => (
            "Run once when the slice is clicked",
            "utilities-terminal-symbolic",
        ),
        ActionKind::Submenu => ("Open a ring of sub-items", "view-app-grid-symbolic"),
        ActionKind::Macro => ("Replay a recorded input macro", "media-record-symbolic"),
        ActionKind::Shortcut => ("Send a keyboard chord", "input-keyboard-symbolic"),
        ActionKind::EasySwitch => ("Hop the mouse to another host", "computer-symbolic"),
        ActionKind::Settings => ("Open a settings surface", "preferences-system-symbolic"),
        ActionKind::Emoji => ("Open the emoji picker", "face-smile-symbolic"),
        ActionKind::Dial => (
            "Scroll to adjust volume / brightness",
            "multimedia-volume-control-symbolic",
        ),
        ActionKind::Power => (
            "Lock, suspend, restart, shut down",
            "system-shutdown-symbolic",
        ),
        ActionKind::NightLight => ("Toggle GNOME night light", "weather-clear-night-symbolic"),
        ActionKind::MouseSetting => ("DPI / SmartShift quick toggle", "input-mouse-symbolic"),
        ActionKind::Widget => ("Live data drawn inside the slice", "view-grid-symbolic"),
        ActionKind::None => ("Placeholder — does nothing", "action-unavailable-symbolic"),
    }
}

/// One built-in widget tile with its canned design-mockup preview.
pub struct BuiltinTile {
    pub source: WidgetSource,
    pub name: &'static str,
    /// Big preview value, e.g. "14°".
    pub value: &'static str,
    /// Preview sublabel, e.g. "Clear · Oslo".
    pub sub: &'static str,
}

/// The 7 built-in widget sources with the canned value/sublabel
/// pairs from the design mockup. Static previews in v1 — live mini
/// previews are an explicitly-deferred follow-up.
pub fn builtin_tiles() -> Vec<BuiltinTile> {
    vec![
        BuiltinTile {
            source: WidgetSource::Weather,
            name: "Weather",
            value: "14°",
            sub: "Clear · Oslo",
        },
        BuiltinTile {
            source: WidgetSource::Cpu,
            name: "CPU usage",
            value: "23%",
            sub: "8 cores · 52°C",
        },
        BuiltinTile {
            source: WidgetSource::Memory,
            name: "Memory",
            value: "11.2",
            sub: "of 32 GB",
        },
        BuiltinTile {
            source: WidgetSource::Network,
            name: "Network rate",
            value: "84↓",
            sub: "12↑ Mb/s",
        },
        BuiltinTile {
            source: WidgetSource::Disk,
            name: "Disk free",
            value: "412",
            sub: "GB free",
        },
        BuiltinTile {
            source: WidgetSource::TasksDue,
            name: "Tasks due",
            value: "3",
            sub: "due in 24h",
        },
        BuiltinTile {
            source: WidgetSource::MouseBattery,
            name: "Mouse battery",
            value: "78%",
            sub: "MX Master 4",
        },
    ]
}

/// Search filter for group 1 — matches name + sub line.
pub fn filter_actions(tiles: Vec<ActionTile>, query: &str) -> Vec<ActionTile> {
    tiles
        .into_iter()
        .filter(|t| matches_search(query, &[t.name, t.sub]))
        .collect()
}

/// Search filter for built-in widget tiles — name + preview sublabel.
pub fn filter_builtins(tiles: Vec<BuiltinTile>, query: &str) -> Vec<BuiltinTile> {
    tiles
        .into_iter()
        .filter(|t| matches_search(query, &[t.name, t.sub]))
        .collect()
}

/// Search filter for registry widget tiles — name / author / id.
pub fn filter_registry<'a>(
    widgets: &'a [WidgetSummaryLite],
    query: &str,
) -> Vec<&'a WidgetSummaryLite> {
    widgets
        .iter()
        .filter(|w| matches_search(query, &[&w.name, &w.author, &w.id]))
        .collect()
}

// ============================================================================
// Chip view
// ============================================================================

const CHIP_ICON_TILE: f32 = 40.0;
const CHIP_ICON_PX: u32 = 24;

/// Window width at which the picker grid switches from 3-up to
/// 4-up. The design's breakpoint is a ~920 px *content* column;
/// add the sidebar (~200 px) and page padding and that's a ~1160 px
/// window. State tracks the window (not the column — see
/// `State::window_width`), so the threshold bakes the chrome in.
const WIDE_GRID_MIN_WINDOW: f32 = 1160.0;

/// 3-up normally, 4-up when the window is wide enough that four
/// tiles still get a readable width each.
pub fn tiles_per_row(window_width: f32) -> usize {
    if window_width >= WIDE_GRID_MIN_WINDOW {
        4
    } else {
        3
    }
}

/// Chip + (when this slice's picker is open) the panel below it.
/// This is the slice editor's replacement for the old kind
/// pick_list — `buttons/mod.rs` renders it as its own row.
pub fn behavior_section<'a>(
    state: &'a State,
    idx: usize,
    slice: &'a Slice,
) -> Element<'a, Message> {
    let open = state.picker_open == Some(idx);
    let chip = behavior_chip(state, idx, slice, open);
    let convertible = convertible_plugin_id(slice, &state.widget_registry).is_some();
    if !open && !convertible {
        return chip;
    }
    let mut col = column![chip].spacing(8);
    if convertible {
        col = col.push(convert_hint_row(state, idx));
    }
    if open {
        col = col.push(picker_panel(state, idx, slice));
    }
    col.into()
}

/// One-line hint under the chip of a legacy native widget slice whose
/// bundled plugin replacement is installed (spec §16): explains the
/// situation + a single "Convert" button. Conversion keeps label and
/// colour; the slice simply starts rendering through the plugin.
fn convert_hint_row(state: &State, idx: usize) -> Element<'_, Message> {
    let pal = &state.palette;
    container(
        row![
            text("A plugin version of this widget is installed — converting keeps your label and colour.")
                .size(10)
                .style(style::text_dim(pal)),
            Space::new().width(Length::Fill),
            button(text("Convert").size(11))
                .style(style::btn_primary(pal))
                .on_press(Message::ConvertSliceToPlugin(idx)),
        ]
        .align_y(Alignment::Center)
        .spacing(8),
    )
    .padding([6, 8])
    .width(Length::Fill)
    .style(style::card_quiet(pal))
    .into()
}

/// Collapsed behavior chip (spec §10b): icon tile · title · summary
/// · `Change…`. Widget slices get an accent wash + WIDGET tag.
fn behavior_chip<'a>(
    state: &'a State,
    idx: usize,
    slice: &'a Slice,
    picker_open: bool,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let is_widget = slice.kind == ActionKind::Widget;

    let title = if slice.label.trim().is_empty() {
        action_kind_name(slice.kind).to_string()
    } else {
        slice.label.clone()
    };
    let summary = chip_summary(state, slice);

    let mut title_row = row![text(title).size(13)]
        .spacing(8)
        .align_y(Alignment::Center);
    if is_widget {
        title_row = title_row.push(
            container(text("WIDGET").size(8).style(style::text_accent(pal)))
                .padding([2, 6])
                .style(style::chip(pal)),
        );
    }

    let change_btn = if picker_open {
        button(text("Cancel").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::ClosePicker)
    } else {
        button(text("Change…").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::OpenPicker(idx))
    };

    // Orphaned widget slice (custom id not installed, spec §10e):
    // a Reinstall button next to Change… opens the downloader.
    let reinstall_btn: Element<Message> = if missing_widget_id(state, slice).is_some() {
        button(text("Reinstall").size(11))
            .style(style::btn_primary(pal))
            .on_press(Message::OpenWidgetStore)
            .into()
    } else {
        Space::new().width(Length::Shrink).into()
    };

    let body = row![
        chip_icon_tile(state, slice),
        column![
            title_row,
            text(summary).size(11).style(style::text_dim(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        reinstall_btn,
        change_btn,
    ]
    .align_y(Alignment::Center)
    .spacing(10);

    container(body)
        .padding(8)
        .width(Length::Fill)
        .style(chip_container(pal, is_widget))
        .into()
}

/// 40 px icon tile on the chip's left. Resolves the slice's icon
/// through the shared raster cache; widget slices without an icon
/// fall back to the registry widget's bundled SVG; final fallback
/// is the title's first letter on a tinted square.
fn chip_icon_tile<'a>(state: &'a State, slice: &'a Slice) -> Element<'a, Message> {
    let pal = &state.palette;

    // 1) The slice's own icon (XDG name or path), tinted like the ring.
    let mut handle = crate::radial_preview::resolve_icon_handle(
        &state.icons,
        &state.iced_handles,
        &slice.icon,
        CHIP_ICON_PX,
        pal.text,
    );
    // 2) Custom widget's bundled icon, untinted (brand colours).
    if handle.is_none() {
        if let Some(WidgetConfig {
            source: WidgetSource::Custom(id),
            ..
        }) = &slice.widget
        {
            if let Some(path) = state
                .widget_registry
                .iter()
                .find(|w| w.id == *id)
                .and_then(|w| w.icon_path.as_ref())
            {
                handle = crate::radial_preview::resolve_icon_handle(
                    &state.icons,
                    &state.iced_handles,
                    &path.to_string_lossy(),
                    CHIP_ICON_PX,
                    pal.text,
                );
            }
        }
    }

    let inner: Element<Message> = match handle {
        Some(h) => iced::widget::image(h)
            .width(Length::Fixed(CHIP_ICON_PX as f32))
            .height(Length::Fixed(CHIP_ICON_PX as f32))
            .into(),
        None => {
            let initial = slice
                .label
                .trim()
                .chars()
                .next()
                .or_else(|| action_kind_name(slice.kind).chars().next())
                .unwrap_or('?');
            text(initial.to_uppercase().to_string())
                .size(16)
                .style(style::text_accent(pal))
                .into()
        }
    };

    container(inner)
        .width(Length::Fixed(CHIP_ICON_TILE))
        .height(Length::Fixed(CHIP_ICON_TILE))
        .center_x(Length::Fixed(CHIP_ICON_TILE))
        .center_y(Length::Fixed(CHIP_ICON_TILE))
        .style(icon_tile_style(pal))
        .into()
}

/// `Some(id)` when the slice points at a custom widget whose id is
/// not in the installed registry (uninstalled / orphaned, spec §10e).
fn missing_widget_id<'a>(state: &State, slice: &'a Slice) -> Option<&'a str> {
    if slice.kind != ActionKind::Widget {
        return None;
    }
    match &slice.widget {
        Some(WidgetConfig {
            source: WidgetSource::Custom(id),
            ..
        }) if !state.widget_registry.iter().any(|w| w.id == *id) => Some(id),
        _ => None,
    }
}

/// Kind-specific one-line summary of what a slice does. Used on the
/// behavior chip AND on the collapsed reorder rows (rows.rs) so the
/// two surfaces never drift apart.
pub fn chip_summary(state: &State, slice: &Slice) -> String {
    let cmd = slice.command.trim();
    match slice.kind {
        ActionKind::Exec | ActionKind::Settings | ActionKind::Emoji => {
            if cmd.is_empty() {
                "No command set".to_string()
            } else {
                cmd.to_string()
            }
        }
        ActionKind::Macro => {
            let name = state
                .macros
                .iter()
                .find(|m| m.id == slice.command)
                .map(|m| {
                    if m.name.trim().is_empty() {
                        m.id.clone()
                    } else {
                        m.name.clone()
                    }
                });
            match name {
                Some(n) => format!("Macro · {n}"),
                None if cmd.is_empty() => "No macro picked".to_string(),
                None => format!("Macro · {cmd}"),
            }
        }
        ActionKind::Shortcut => {
            if cmd.is_empty() {
                "No chord set".to_string()
            } else {
                format!("Sends {cmd}")
            }
        }
        ActionKind::EasySwitch => {
            if cmd.is_empty() {
                "No host picked".to_string()
            } else {
                format!("Switch to host {cmd}")
            }
        }
        ActionKind::Submenu => {
            let n = slice.submenu.len();
            format!("{n} sub-item{}", if n == 1 { "" } else { "s" })
        }
        ActionKind::Widget => {
            // Uninstalled custom widget → "missing widget" summary
            // (spec §10e); the chip also grows a Reinstall button.
            if let Some(id) = missing_widget_id(state, slice) {
                return format!("Missing widget · \"{id}\" is not installed");
            }
            let name = slice
                .widget
                .as_ref()
                .map(|w| widget_display_name(&w.source, &state.widget_registry))
                .unwrap_or_else(|| "no source picked".to_string());
            format!("Live widget · {name}")
        }
        ActionKind::Dial => match slice.dial {
            Some(oxidemx_shared::DialKind::Brightness) => "Scroll-adjust brightness".to_string(),
            Some(oxidemx_shared::DialKind::Volume) => "Scroll-adjust volume".to_string(),
            None => "No dial target picked".to_string(),
        },
        ActionKind::Power => {
            if cmd.is_empty() {
                "No power action picked".to_string()
            } else {
                format!("Power · {cmd}")
            }
        }
        ActionKind::NightLight => "Toggles GNOME night light".to_string(),
        ActionKind::MouseSetting => {
            if cmd.is_empty() {
                "No setting string".to_string()
            } else {
                format!("Mouse · {cmd}")
            }
        }
        ActionKind::None => "Does nothing".to_string(),
    }
}

// ============================================================================
// Picker panel view
// ============================================================================

fn picker_panel<'a>(state: &'a State, idx: usize, slice: &'a Slice) -> Element<'a, Message> {
    let pal = &state.palette;
    let query = state.picker_search.as_str();

    let search = text_input("Search actions and widgets…", query)
        .on_input(Message::PickerSearch)
        .padding(6)
        .size(12);

    let actions = filter_actions(action_tiles(), query);
    // Canned native tiles whose bundled plugin is installed+ready are
    // hidden — the registry tile covers them (spec §16; new picks
    // produce Custom widgets).
    let visible_builtins = visible_builtin_tiles(&state.widget_registry);
    let visible_builtin_count = visible_builtins.len();
    let builtins = filter_builtins(visible_builtins, query);
    let registry = filter_registry(&state.widget_registry, query);

    let per_row = tiles_per_row(state.window_width);
    let mut panel = column![search].spacing(10);

    // Group 1 — built-in actions. Header hidden when search empties it.
    if !actions.is_empty() {
        panel = panel.push(group_header(pal, "Built-in actions".to_string()));
        let tiles: Vec<Element<Message>> = actions
            .into_iter()
            .map(|t| action_tile_view(state, idx, slice, t))
            .collect();
        panel = panel.push(tile_grid(tiles, per_row));
    }

    // Group 2 — widgets (built-in sources + installed registry).
    // Always rendered: the "Get more widgets…" stub is the grid's
    // last tile in the design's flow (iced has no dashed borders,
    // so it renders as a quiet outline tile), and it survives any
    // search query.
    let installed = visible_builtin_count + state.widget_registry.len();
    panel = panel.push(group_header(
        pal,
        format!("Widgets · {installed} installed"),
    ));
    if builtins.is_empty() && registry.is_empty() && !query.trim().is_empty() {
        panel = panel.push(
            text("No widgets match — clear the search to see everything.")
                .size(11)
                .style(style::text_faint(pal)),
        );
    }
    let mut tiles: Vec<Element<Message>> = builtins
        .into_iter()
        .map(|t| builtin_tile_view(state, idx, slice, t))
        .collect();
    tiles.extend(
        registry
            .into_iter()
            .map(|w| registry_tile_view(state, idx, slice, w)),
    );
    tiles.push(get_more_tile(pal));
    panel = panel.push(tile_grid(tiles, per_row));

    container(panel)
        .padding(10)
        .width(Length::Fill)
        .style(style::card_quiet(pal))
        .into()
}

fn group_header(pal: &Palette, label: String) -> Element<'_, Message> {
    text(label).size(11).style(style::text_dim(pal)).into()
}

/// Pack tiles into fixed `per_row`-up rows (3-up, 4-up on wide
/// windows — see [`tiles_per_row`]). Short rows are padded with
/// spacers so every tile keeps the same width.
fn tile_grid(tiles: Vec<Element<'_, Message>>, per_row: usize) -> Element<'_, Message> {
    let per_row = per_row.max(1);
    let mut grid = column![].spacing(8);
    let mut tiles = tiles.into_iter().peekable();
    while tiles.peek().is_some() {
        let mut r = row![].spacing(8);
        let mut n = 0;
        for tile in tiles.by_ref().take(per_row) {
            r = r.push(tile);
            n += 1;
        }
        while n < per_row {
            r = r.push(Space::new().width(Length::FillPortion(1)));
            n += 1;
        }
        grid = grid.push(r);
    }
    grid.into()
}

/// Compact action tile: icon + name + one-line sub. Click applies
/// the kind immediately (`Message::PickAction` closes the panel).
fn action_tile_view<'a>(
    state: &'a State,
    idx: usize,
    slice: &'a Slice,
    tile: ActionTile,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let selected = pick_matches_slice(slice, &PickChoice::Action(tile.kind));

    let icon: Element<Message> = match crate::radial_preview::resolve_icon_handle(
        &state.icons,
        &state.iced_handles,
        tile.icon,
        20,
        pal.text,
    ) {
        Some(h) => iced::widget::image(h)
            .width(Length::Fixed(20.0))
            .height(Length::Fixed(20.0))
            .into(),
        None => text(tile.name.chars().next().unwrap_or('?').to_string())
            .size(13)
            .style(style::text_accent(pal))
            .into(),
    };

    let mut name_row = row![text(tile.name).size(12)]
        .spacing(4)
        .align_y(Alignment::Center);
    if selected {
        name_row = name_row.push(oxidemx_widgets::icons::icon("check", 11.0, pal.accent));
    }

    let body = row![
        container(icon)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .center_x(Length::Fixed(28.0))
            .center_y(Length::Fixed(28.0)),
        column![
            name_row,
            text(tile.sub).size(9).style(style::text_faint(pal)),
        ]
        .spacing(2),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    button(body)
        .padding(8)
        .width(Length::FillPortion(1))
        .style(tile_style(pal, selected))
        .on_press(Message::PickAction(idx, tile.kind))
        .into()
}

/// Built-in widget tile: canned preview value + sublabel (static in
/// v1 — live minis are deferred) + the source name.
fn builtin_tile_view<'a>(
    state: &'a State,
    idx: usize,
    slice: &'a Slice,
    tile: BuiltinTile,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let selected = pick_matches_slice(slice, &PickChoice::Widget(tile.source.clone()));

    let mut name_row = row![text(tile.name).size(11)]
        .spacing(4)
        .align_y(Alignment::Center);
    if selected {
        name_row = name_row.push(oxidemx_widgets::icons::icon("check", 10.0, pal.accent));
    }

    let body = column![
        text(tile.value).size(20).style(style::text_accent(pal)),
        text(tile.sub).size(9).style(style::text_faint(pal)),
        Space::new().height(Length::Fixed(4.0)),
        name_row,
        text("OxideMX built-in")
            .size(8)
            .style(style::text_faint(pal)),
    ]
    .spacing(2);

    button(body)
        .padding(10)
        .width(Length::FillPortion(1))
        .style(tile_style(pal, selected))
        .on_press(Message::PickWidget(idx, tile.source))
        .into()
}

/// Installed (registry) widget tile: bundled icon, name, author ·
/// version, gear badge when it declares options. Incompatible
/// widgets render dimmed + unclickable with the reason inline.
fn registry_tile_view<'a>(
    state: &'a State,
    idx: usize,
    slice: &'a Slice,
    w: &'a WidgetSummaryLite,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let source = WidgetSource::Custom(w.id.clone());
    let selected = pick_matches_slice(slice, &PickChoice::Widget(source.clone()));

    let icon: Element<Message> = w
        .icon_path
        .as_ref()
        .and_then(|p| {
            crate::radial_preview::resolve_icon_handle(
                &state.icons,
                &state.iced_handles,
                &p.to_string_lossy(),
                24,
                pal.text,
            )
        })
        .map(|h| -> Element<Message> {
            iced::widget::image(h)
                .width(Length::Fixed(24.0))
                .height(Length::Fixed(24.0))
                .into()
        })
        .unwrap_or_else(|| {
            text(
                w.name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string(),
            )
            .size(14)
            .style(style::text_accent(pal))
            .into()
        });

    let mut name_row = row![text(w.name.as_str()).size(11)]
        .spacing(4)
        .align_y(Alignment::Center);
    if w.has_options {
        name_row = name_row.push(oxidemx_widgets::icons::icon("gear", 10.0, pal.subtext0));
    }
    if selected {
        name_row = name_row.push(oxidemx_widgets::icons::icon("check", 10.0, pal.accent));
    }

    let mut body = column![row![
        container(icon)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .center_x(Length::Fixed(28.0))
            .center_y(Length::Fixed(28.0)),
        column![
            name_row,
            text(format!("{} · v{}", w.author, w.version))
                .size(8)
                .style(style::text_faint(pal)),
        ]
        .spacing(2),
    ]
    .align_y(Alignment::Center)
    .spacing(8),]
    .spacing(4);

    if !w.ready {
        // Reason rendered inline (no tooltip widget in this app yet);
        // shown verbatim per spec §9.
        let reason = w.reason.as_deref().unwrap_or("incompatible");
        body = body.push(
            text(format!("incompatible — {reason}"))
                .size(8)
                .style(style::text_faint(pal)),
        );
    }

    let mut btn = button(body)
        .padding(10)
        .width(Length::FillPortion(1))
        .style(tile_style(pal, selected));
    if w.ready {
        btn = btn.on_press(Message::PickWidget(idx, source));
    }
    btn.into()
}

/// The always-last "Get more widgets…" tile in the widgets grid →
/// store dialog. Design's `MoreWidgetsTile` is a dashed-outline
/// tile; iced has no dashed borders, so it renders as a quiet
/// outline tile in the same grid flow.
fn get_more_tile(pal: &Palette) -> Element<'_, Message> {
    let body = column![
        text("+").size(18).style(style::text_dim(pal)),
        text("Get more widgets…").size(11),
        text("Browse the community registry")
            .size(9)
            .style(style::text_faint(pal)),
    ]
    .align_x(Alignment::Center)
    .spacing(2)
    .width(Length::Fill);

    button(body)
        .padding(10)
        .width(Length::FillPortion(1))
        .style(more_tile_style(pal))
        .on_press(Message::OpenWidgetStore)
        .into()
}

// ============================================================================
// Local styles
// ============================================================================

/// Chip container — quiet card normally, accent wash + accent
/// border for widget slices (spec §10b scannability).
fn chip_container(
    pal: &Palette,
    is_widget: bool,
) -> impl Fn(&iced::Theme) -> iced::widget::container::Style + 'static {
    let bg = if is_widget { pal.accent_06 } else { pal.crust };
    let border = if is_widget {
        pal.accent_40
    } else {
        pal.hairline
    };
    let text_color = pal.text;
    move |_| iced::widget::container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Some(text_color),
        ..Default::default()
    }
}

/// The chip's 40 px icon square.
fn icon_tile_style(
    pal: &Palette,
) -> impl Fn(&iced::Theme) -> iced::widget::container::Style + 'static {
    let bg = pal.surface0;
    let border = pal.hairline_faint;
    move |_| iced::widget::container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

/// Quiet outline tile for "Get more widgets…" — transparent body,
/// hairline border (the closest iced gets to the design's dashed
/// outline), text brightens on hover.
fn more_tile_style(
    pal: &Palette,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style + 'static {
    let hover_bg = pal.row_hover;
    let border = pal.hairline;
    let border_hover = pal.hairline_strong;
    let text_color = pal.text;
    move |_, status| {
        let hovered = matches!(status, iced::widget::button::Status::Hovered);
        iced::widget::button::Style {
            background: hovered.then_some(Background::Color(hover_bg)),
            text_color,
            border: Border {
                color: if hovered { border_hover } else { border },
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        }
    }
}

/// Picker tile button — surface card; the currently-applied tile
/// gets an accent ring + wash. Tiles without an `on_press`
/// (incompatible registry widgets, spec §9) reach this closure with
/// `Status::Disabled` and render properly dimmed: washed-out
/// background, faint border, muted text.
fn tile_style(
    pal: &Palette,
    selected: bool,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style + 'static {
    let bg = if selected {
        pal.accent_06
    } else {
        pal.surface0
    };
    let hover_bg = pal.row_hover;
    let border = if selected { pal.accent } else { pal.hairline };
    let border_hover = if selected {
        pal.accent
    } else {
        pal.hairline_strong
    };
    let text_color = pal.text;
    let disabled_bg = pal.crust;
    let disabled_border = pal.hairline_faint;
    let disabled_text = pal.overlay0;
    move |_, status| {
        if matches!(status, iced::widget::button::Status::Disabled) {
            return iced::widget::button::Style {
                background: Some(Background::Color(disabled_bg)),
                text_color: disabled_text,
                border: Border {
                    color: disabled_border,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            };
        }
        let hovered = matches!(status, iced::widget::button::Status::Hovered);
        iced::widget::button::Style {
            background: Some(Background::Color(if hovered && !selected {
                hover_bg
            } else {
                bg
            })),
            text_color,
            border: Border {
                color: if hovered { border_hover } else { border },
                width: if selected { 2.0 } else { 1.0 },
                radius: 8.0.into(),
            },
            ..Default::default()
        }
    }
}

// ============================================================================
// Tests — pure helpers only (view code is exercised by cargo check)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(kind: ActionKind, label: &str) -> Slice {
        Slice {
            action_id: None,
            label: label.to_string(),
            kind,
            command: String::new(),
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

    fn lite(id: &str, name: &str, author: &str, ready: bool) -> WidgetSummaryLite {
        WidgetSummaryLite {
            id: id.into(),
            name: name.into(),
            version: "1.0.0".into(),
            author: author.into(),
            ready,
            reason: (!ready).then(|| "nope".to_string()),
            has_options: false,
            icon_path: None,
            signature: "unsigned".into(),
        }
    }

    // --- grid density ---

    #[test]
    fn grid_is_three_up_normally_four_up_when_wide() {
        assert_eq!(tiles_per_row(800.0), 3);
        assert_eq!(tiles_per_row(WIDE_GRID_MIN_WINDOW - 1.0), 3);
        assert_eq!(tiles_per_row(WIDE_GRID_MIN_WINDOW), 4);
        assert_eq!(tiles_per_row(1920.0), 4);
    }

    // --- search filtering ---

    #[test]
    fn search_is_case_insensitive_substring() {
        assert!(matches_search("WEA", &["Weather", "JuhLabs"]));
        assert!(matches_search("labs", &["Weather", "JuhLabs"]));
        assert!(matches_search("", &["anything"]));
        assert!(!matches_search("zzz", &["Weather", "JuhLabs"]));
    }

    #[test]
    fn filter_actions_excludes_widget_and_matches_name_or_sub() {
        let all = action_tiles();
        assert!(all.iter().all(|t| t.kind != ActionKind::Widget));
        // matches by display name
        let hits = filter_actions(action_tiles(), "macro");
        assert!(hits.iter().any(|t| t.kind == ActionKind::Macro));
        // matches by sub line
        let hits = filter_actions(action_tiles(), "night light");
        assert!(hits.iter().any(|t| t.kind == ActionKind::NightLight));
        // empty query keeps everything
        assert_eq!(filter_actions(action_tiles(), "").len(), all.len());
    }

    #[test]
    fn filter_registry_matches_name_author_id() {
        let reg = vec![
            lite("weather2", "Weather Two", "JuhLabs", true),
            lite("clock", "Clock", "Someone Else", true),
        ];
        assert_eq!(filter_registry(&reg, "juhlabs").len(), 1);
        assert_eq!(filter_registry(&reg, "clock").len(), 1);
        assert_eq!(filter_registry(&reg, "weather2").len(), 1);
        assert_eq!(filter_registry(&reg, "").len(), 2);
        assert_eq!(filter_registry(&reg, "nope").len(), 0);
    }

    #[test]
    fn filter_builtins_matches_sublabel() {
        let hits = filter_builtins(builtin_tiles(), "oslo");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, WidgetSource::Weather);
    }

    // --- auto-label rule ---

    #[test]
    fn auto_label_when_empty_or_previous_auto() {
        assert!(should_auto_label("", None));
        assert!(should_auto_label("   ", None));
        assert!(should_auto_label("Weather", Some("Weather")));
        // user-typed label is never clobbered
        assert!(!should_auto_label("My label", Some("Weather")));
        assert!(!should_auto_label("My label", None));
    }

    #[test]
    fn apply_widget_pick_relabels_only_auto_labels() {
        let reg = vec![lite("clock", "Clock", "X", true)];

        // empty label → auto-label with the widget name
        let mut s = slice(ActionKind::Exec, "");
        apply_widget_pick(
            &mut s,
            WidgetSource::Custom("clock".into()),
            "Apps",
            4,
            &reg,
        );
        assert_eq!(s.label, "Clock");

        // label equals the previous pick's auto-label → replaced
        let mut s2 = slice(ActionKind::Widget, "Clock");
        s2.widget = Some(widget_config_for_pick(
            WidgetSource::Custom("clock".into()),
            "Apps",
            4,
        ));
        apply_widget_pick(&mut s2, WidgetSource::Weather, "Apps", 4, &reg);
        assert_eq!(s2.label, "Weather");

        // user-typed label survives
        let mut s3 = slice(ActionKind::Exec, "My thing");
        apply_widget_pick(&mut s3, WidgetSource::Weather, "Apps", 4, &reg);
        assert_eq!(s3.label, "My thing");
    }

    // --- instance_key assignment ---

    #[test]
    fn custom_pick_assigns_instance_key_builtin_does_not() {
        let cfg = widget_config_for_pick(WidgetSource::Custom("weather2".into()), "Apps", 4);
        assert_eq!(cfg.instance_key.as_deref(), Some("apps.slot4"));
        assert_eq!(cfg.scope, WidgetScope::Instance);
        assert_eq!(cfg.format, None);

        let cfg = widget_config_for_pick(WidgetSource::Cpu, "Apps", 4);
        assert_eq!(cfg.instance_key, None);
        assert_eq!(cfg.scope, WidgetScope::Instance);
    }

    #[test]
    fn apply_widget_pick_sets_kind_and_config() {
        let mut s = slice(ActionKind::Exec, "");
        apply_widget_pick(
            &mut s,
            WidgetSource::Custom("clock".into()),
            "My Page",
            2,
            &[],
        );
        assert_eq!(s.kind, ActionKind::Widget);
        let w = s.widget.expect("widget config set");
        assert_eq!(w.source, WidgetSource::Custom("clock".into()));
        assert_eq!(w.instance_key.as_deref(), Some("my-page.slot2"));
    }

    // --- undo-by-reselect matching ---

    #[test]
    fn pick_matches_stored_slice_behavior() {
        let exec = slice(ActionKind::Exec, "x");
        assert!(pick_matches_slice(
            &exec,
            &PickChoice::Action(ActionKind::Exec)
        ));
        assert!(!pick_matches_slice(
            &exec,
            &PickChoice::Action(ActionKind::Macro)
        ));
        assert!(!pick_matches_slice(
            &exec,
            &PickChoice::Widget(WidgetSource::Cpu)
        ));

        let mut w = slice(ActionKind::Widget, "Weather");
        w.widget = Some(widget_config_for_pick(
            WidgetSource::Custom("weather2".into()),
            "Apps",
            0,
        ));
        assert!(pick_matches_slice(
            &w,
            &PickChoice::Widget(WidgetSource::Custom("weather2".into()))
        ));
        // different widget id → no match
        assert!(!pick_matches_slice(
            &w,
            &PickChoice::Widget(WidgetSource::Custom("clock".into()))
        ));
        // widget kind never matches an action tile of kind Widget? —
        // action tiles exclude Widget, but the choice still compares
        // by kind for completeness.
        assert!(pick_matches_slice(
            &w,
            &PickChoice::Action(ActionKind::Widget)
        ));
    }

    // --- display names ---

    // --- builtin → bundled-plugin mapping (spec §16) ---

    #[test]
    fn builtin_plugin_id_maps_six_sources_battery_stays_native() {
        assert_eq!(builtin_plugin_id(&WidgetSource::Weather), Some("weather"));
        assert_eq!(builtin_plugin_id(&WidgetSource::Cpu), Some("cpu"));
        assert_eq!(builtin_plugin_id(&WidgetSource::Memory), Some("memory"));
        assert_eq!(builtin_plugin_id(&WidgetSource::Network), Some("network"));
        assert_eq!(builtin_plugin_id(&WidgetSource::Disk), Some("disk"));
        assert_eq!(builtin_plugin_id(&WidgetSource::TasksDue), Some("tasks"));
        assert_eq!(builtin_plugin_id(&WidgetSource::MouseBattery), None);
        assert_eq!(builtin_plugin_id(&WidgetSource::Custom("cpu".into())), None);
    }

    #[test]
    fn visible_builtin_tiles_hides_ready_plugins_only() {
        // nothing installed → all 7 canned tiles
        assert_eq!(visible_builtin_tiles(&[]).len(), 7);

        // cpu installed + ready → its canned tile is hidden
        let reg = vec![lite("cpu", "CPU usage", "OxideMX", true)];
        let visible = visible_builtin_tiles(&reg);
        assert_eq!(visible.len(), 6);
        assert!(!visible.iter().any(|t| t.source == WidgetSource::Cpu));

        // installed but NOT ready → the native tile stays
        let reg = vec![lite("cpu", "CPU usage", "OxideMX", false)];
        assert_eq!(visible_builtin_tiles(&reg).len(), 7);

        // MouseBattery never hides, even with a same-named plugin
        let reg = vec![lite("mouse-battery", "Mouse battery", "X", true)];
        let visible = visible_builtin_tiles(&reg);
        assert!(visible
            .iter()
            .any(|t| t.source == WidgetSource::MouseBattery));
    }

    // --- convert-to-plugin (spec §16 back-compat affordance) ---

    fn native_widget_slice(source: WidgetSource) -> Slice {
        let mut s = slice(ActionKind::Widget, "My label");
        s.color = "teal".into();
        s.widget = Some(WidgetConfig {
            source,
            format: Some("custom %s".into()),
            scope: WidgetScope::Instance,
            instance_key: None,
        });
        s
    }

    #[test]
    fn convertible_only_when_native_source_has_ready_plugin() {
        let ready = vec![lite("cpu", "CPU usage", "OxideMX", true)];
        let not_ready = vec![lite("cpu", "CPU usage", "OxideMX", false)];

        let s = native_widget_slice(WidgetSource::Cpu);
        assert_eq!(convertible_plugin_id(&s, &ready), Some("cpu"));
        assert_eq!(convertible_plugin_id(&s, &not_ready), None);
        assert_eq!(convertible_plugin_id(&s, &[]), None);

        // Custom slices are already plugins; battery has no mapping;
        // non-widget slices never convert.
        let c = native_widget_slice(WidgetSource::Custom("cpu".into()));
        assert_eq!(convertible_plugin_id(&c, &ready), None);
        let b = native_widget_slice(WidgetSource::MouseBattery);
        assert_eq!(convertible_plugin_id(&b, &ready), None);
        let e = slice(ActionKind::Exec, "x");
        assert_eq!(convertible_plugin_id(&e, &ready), None);
    }

    #[test]
    fn convert_cpu_rewrites_source_and_keeps_label_color() {
        let mut s = native_widget_slice(WidgetSource::Cpu);
        let overlay = oxidemx_shared::OverlayConfig::default();
        let mut store = oxidemx_shared::widgets::WidgetStore::default();

        let id = convert_slice_to_plugin(&mut s, &overlay, &mut store, "Apps", 2);
        assert_eq!(id.as_deref(), Some("cpu"));

        let w = s.widget.as_ref().unwrap();
        assert_eq!(w.source, WidgetSource::Custom("cpu".into()));
        assert_eq!(w.scope, WidgetScope::Instance);
        assert_eq!(w.instance_key.as_deref(), Some("apps.slot2"));
        // format has no plugin equivalent — dropped
        assert_eq!(w.format, None);
        // label/colour untouched
        assert_eq!(s.label, "My label");
        assert_eq!(s.color, "teal");
        // cpu has no legacy settings — no bags created
        assert!(store.is_empty());
    }

    #[test]
    fn convert_weather_lifts_overlay_settings_into_instance_bag() {
        let mut s = native_widget_slice(WidgetSource::Weather);
        let overlay = oxidemx_shared::OverlayConfig {
            weather_location: Some((59.91, 10.75)),
            weather_place: Some("Oslo, NO".into()),
            weather_celsius: true,
            ..Default::default()
        };
        let mut store = oxidemx_shared::widgets::WidgetStore::default();

        let id = convert_slice_to_plugin(&mut s, &overlay, &mut store, "Apps", 4);
        assert_eq!(id.as_deref(), Some("weather"));
        assert_eq!(
            s.widget.as_ref().unwrap().instance_key.as_deref(),
            Some("apps.slot4")
        );

        // Same shapes as oxidemx_shared::migrate's lift, but into the
        // INSTANCE bag (the converted slice's scope is Instance).
        let bag = &store.instances["apps.slot4"]["weather"];
        assert_eq!(bag["location"]["name"], serde_json::json!("Oslo, NO"));
        assert_eq!(bag["location"]["lat"], serde_json::json!(59.91));
        assert_eq!(bag["location"]["lon"], serde_json::json!(10.75));
        assert_eq!(bag["units"], serde_json::json!("c"));

        // celsius=false → "f"
        let mut s2 = native_widget_slice(WidgetSource::Weather);
        let overlay_f = oxidemx_shared::OverlayConfig {
            weather_location: Some((40.7, -74.0)),
            weather_place: None,
            weather_celsius: false,
            ..Default::default()
        };
        convert_slice_to_plugin(&mut s2, &overlay_f, &mut store, "Apps", 5);
        let bag = &store.instances["apps.slot5"]["weather"];
        assert_eq!(bag["units"], serde_json::json!("f"));
        assert_eq!(bag["location"]["name"], serde_json::json!(""));
    }

    #[test]
    fn convert_weather_without_overlay_fields_writes_no_bag() {
        let mut s = native_widget_slice(WidgetSource::Weather);
        let overlay = oxidemx_shared::OverlayConfig::default();
        let mut store = oxidemx_shared::widgets::WidgetStore::default();
        let id = convert_slice_to_plugin(&mut s, &overlay, &mut store, "Apps", 4);
        assert_eq!(id.as_deref(), Some("weather"));
        assert!(store.is_empty());
    }

    #[test]
    fn convert_weather_never_clobbers_existing_bag_values() {
        let mut s = native_widget_slice(WidgetSource::Weather);
        let overlay = oxidemx_shared::OverlayConfig {
            weather_location: Some((59.91, 10.75)),
            weather_place: Some("Oslo, NO".into()),
            weather_celsius: true,
            ..Default::default()
        };
        let mut store = oxidemx_shared::widgets::WidgetStore::default();
        store
            .instances
            .entry("apps.slot4".into())
            .or_default()
            .entry("weather".into())
            .or_default()
            .insert("units".into(), serde_json::json!("f"));

        convert_slice_to_plugin(&mut s, &overlay, &mut store, "Apps", 4);
        let bag = &store.instances["apps.slot4"]["weather"];
        // user's existing value wins; the missing key is still lifted
        assert_eq!(bag["units"], serde_json::json!("f"));
        assert_eq!(bag["location"]["name"], serde_json::json!("Oslo, NO"));
    }

    #[test]
    fn convert_refuses_non_convertible_slices() {
        let overlay = oxidemx_shared::OverlayConfig::default();
        let mut store = oxidemx_shared::widgets::WidgetStore::default();

        let mut battery = native_widget_slice(WidgetSource::MouseBattery);
        assert_eq!(
            convert_slice_to_plugin(&mut battery, &overlay, &mut store, "Apps", 1),
            None
        );
        assert_eq!(
            battery.widget.as_ref().unwrap().source,
            WidgetSource::MouseBattery
        );

        let mut custom = native_widget_slice(WidgetSource::Custom("cpu".into()));
        assert_eq!(
            convert_slice_to_plugin(&mut custom, &overlay, &mut store, "Apps", 1),
            None
        );

        let mut exec = slice(ActionKind::Exec, "x");
        assert_eq!(
            convert_slice_to_plugin(&mut exec, &overlay, &mut store, "Apps", 1),
            None
        );
    }

    #[test]
    fn widget_display_name_prefers_registry_name() {
        let reg = vec![lite("weather2", "Weather Two", "JuhLabs", true)];
        assert_eq!(
            widget_display_name(&WidgetSource::Custom("weather2".into()), &reg),
            "Weather Two"
        );
        // uninstalled id falls back to the raw id
        assert_eq!(
            widget_display_name(&WidgetSource::Custom("ghost".into()), &reg),
            "ghost"
        );
        assert_eq!(widget_display_name(&WidgetSource::Cpu, &reg), "CPU usage");
    }
}
