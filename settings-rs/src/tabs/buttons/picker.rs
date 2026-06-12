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

/// Scan `~/.config/oxidemx/widgets` into the settings-side cache.
/// Called once at startup and on `Message::RescanWidgets`.
pub fn scan_registry() -> Vec<WidgetSummaryLite> {
    use oxidemx_widget_host::registry::{SignatureState, WidgetRegistry, WidgetState};
    let Some(dir) = WidgetRegistry::widgets_dir() else {
        return Vec::new();
    };
    let reg = WidgetRegistry::scan(&dir);
    reg.iter()
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
        .collect()
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
fn action_kind_name(kind: ActionKind) -> &'static str {
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
        ActionKind::Exec => ("Run once when the slice is clicked", "utilities-terminal-symbolic"),
        ActionKind::Submenu => ("Open a ring of sub-items", "view-app-grid-symbolic"),
        ActionKind::Macro => ("Replay a recorded input macro", "media-record-symbolic"),
        ActionKind::Shortcut => ("Send a keyboard chord", "input-keyboard-symbolic"),
        ActionKind::EasySwitch => ("Hop the mouse to another host", "computer-symbolic"),
        ActionKind::Settings => ("Open a settings surface", "preferences-system-symbolic"),
        ActionKind::Emoji => ("Open the emoji picker", "face-smile-symbolic"),
        ActionKind::Dial => ("Scroll to adjust volume / brightness", "multimedia-volume-control-symbolic"),
        ActionKind::Power => ("Lock, suspend, restart, shut down", "system-shutdown-symbolic"),
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
        BuiltinTile { source: WidgetSource::Weather, name: "Weather", value: "14°", sub: "Clear · Oslo" },
        BuiltinTile { source: WidgetSource::Cpu, name: "CPU usage", value: "23%", sub: "8 cores · 52°C" },
        BuiltinTile { source: WidgetSource::Memory, name: "Memory", value: "11.2", sub: "of 32 GB" },
        BuiltinTile { source: WidgetSource::Network, name: "Network rate", value: "84↓", sub: "12↑ Mb/s" },
        BuiltinTile { source: WidgetSource::Disk, name: "Disk free", value: "412", sub: "GB free" },
        BuiltinTile { source: WidgetSource::TasksDue, name: "Tasks due", value: "3", sub: "due today" },
        BuiltinTile { source: WidgetSource::MouseBattery, name: "Mouse battery", value: "78%", sub: "MX Master 4" },
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
const TILES_PER_ROW: usize = 3;

/// Chip + (when this slice's picker is open) the panel below it.
/// This is the slice editor's replacement for the old kind
/// pick_list — `buttons/mod.rs` renders it as its own row.
pub fn behavior_section<'a>(state: &'a State, idx: usize, slice: &'a Slice) -> Element<'a, Message> {
    let open = state.picker_open == Some(idx);
    let chip = behavior_chip(state, idx, slice, open);
    if open {
        column![chip, picker_panel(state, idx, slice)]
            .spacing(8)
            .into()
    } else {
        chip
    }
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

    let mut title_row = row![text(title).size(13)].spacing(8).align_y(Alignment::Center);
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

    let body = row![
        chip_icon_tile(state, slice),
        column![
            title_row,
            text(summary).size(11).style(style::text_dim(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
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
        if let Some(WidgetConfig { source: WidgetSource::Custom(id), .. }) = &slice.widget {
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

/// Kind-specific one-line summary on the chip.
fn chip_summary(state: &State, slice: &Slice) -> String {
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
                .map(|m| if m.name.trim().is_empty() { m.id.clone() } else { m.name.clone() });
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
    let builtins = filter_builtins(builtin_tiles(), query);
    let registry = filter_registry(&state.widget_registry, query);

    let mut panel = column![search].spacing(10);

    // Group 1 — built-in actions. Header hidden when search empties it.
    if !actions.is_empty() {
        panel = panel.push(group_header(pal, "Built-in actions".to_string()));
        let tiles: Vec<Element<Message>> = actions
            .into_iter()
            .map(|t| action_tile_view(state, idx, slice, t))
            .collect();
        panel = panel.push(tile_grid(tiles));
    }

    // Group 2 — widgets (built-in sources + installed registry).
    if !builtins.is_empty() || !registry.is_empty() {
        let installed = builtin_tiles().len() + state.widget_registry.len();
        panel = panel.push(group_header(pal, format!("Widgets · {installed} installed")));
        let mut tiles: Vec<Element<Message>> = builtins
            .into_iter()
            .map(|t| builtin_tile_view(state, idx, slice, t))
            .collect();
        tiles.extend(registry.into_iter().map(|w| registry_tile_view(state, idx, slice, w)));
        panel = panel.push(tile_grid(tiles));
    } else {
        // Both groups emptied by the search (builtins are static, so
        // this only happens with a non-matching query).
        panel = panel.push(
            text("No matches — clear the search to see everything.")
                .size(11)
                .style(style::text_faint(pal)),
        );
    }

    // Last tile is always the "Get more widgets…" stub (iced has no
    // dashed borders, so it renders as a quiet outline tile).
    panel = panel.push(get_more_tile(pal));

    container(panel)
        .padding(10)
        .width(Length::Fill)
        .style(style::card_quiet(pal))
        .into()
}

fn group_header(pal: &Palette, label: String) -> Element<'_, Message> {
    text(label).size(11).style(style::text_dim(pal)).into()
}

/// Pack tiles into fixed 3-up rows (plan allows fixed-width grid;
/// responsive 4-up at ≥920 px is a deferred nicety). Short rows are
/// padded with spacers so every tile keeps the same width.
fn tile_grid(tiles: Vec<Element<'_, Message>>) -> Element<'_, Message> {
    let mut grid = column![].spacing(8);
    let mut tiles = tiles.into_iter().peekable();
    while tiles.peek().is_some() {
        let mut r = row![].spacing(8);
        let mut n = 0;
        for tile in tiles.by_ref().take(TILES_PER_ROW) {
            r = r.push(tile);
            n += 1;
        }
        while n < TILES_PER_ROW {
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

    let mut name_row = row![text(tile.name).size(12)].spacing(4).align_y(Alignment::Center);
    if selected {
        name_row = name_row.push(text("✓").size(11).style(style::text_accent(pal)));
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

    let mut name_row = row![text(tile.name).size(11)].spacing(4).align_y(Alignment::Center);
    if selected {
        name_row = name_row.push(text("✓").size(10).style(style::text_accent(pal)));
    }

    let body = column![
        text(tile.value).size(20).style(style::text_accent(pal)),
        text(tile.sub).size(9).style(style::text_faint(pal)),
        Space::new().height(Length::Fixed(4.0)),
        name_row,
        text("OxideMX built-in").size(8).style(style::text_faint(pal)),
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
            text(w.name.chars().next().unwrap_or('?').to_uppercase().to_string())
                .size(14)
                .style(style::text_accent(pal))
                .into()
        });

    let mut name_row = row![text(w.name.as_str()).size(11)]
        .spacing(4)
        .align_y(Alignment::Center);
    if w.has_options {
        name_row = name_row.push(text("⚙").size(10).style(style::text_dim(pal)));
    }
    if selected {
        name_row = name_row.push(text("✓").size(10).style(style::text_accent(pal)));
    }

    let mut body = column![
        row![
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
        .spacing(8),
    ]
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

/// The always-last "Get more widgets…" stub tile → store dialog
/// (Task 3; the message exists now and reports a hint).
fn get_more_tile(pal: &Palette) -> Element<'_, Message> {
    let body = row![
        text("+").size(16).style(style::text_dim(pal)),
        column![
            text("Get more widgets…").size(12),
            text("Install community widgets")
                .size(9)
                .style(style::text_faint(pal)),
        ]
        .spacing(2),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    button(body)
        .padding(8)
        .width(Length::Fill)
        .style(style::btn_flat(pal))
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
    let border = if is_widget { pal.accent_40 } else { pal.hairline };
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
fn icon_tile_style(pal: &Palette) -> impl Fn(&iced::Theme) -> iced::widget::container::Style + 'static {
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

/// Picker tile button — surface card; the currently-applied tile
/// gets an accent ring + wash.
fn tile_style(
    pal: &Palette,
    selected: bool,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style + 'static {
    let bg = if selected { pal.accent_06 } else { pal.surface0 };
    let hover_bg = pal.row_hover;
    let border = if selected { pal.accent } else { pal.hairline };
    let border_hover = if selected { pal.accent } else { pal.hairline_strong };
    let text_color = pal.text;
    move |_, status| {
        let hovered = matches!(status, iced::widget::button::Status::Hovered);
        iced::widget::button::Style {
            background: Some(Background::Color(if hovered && !selected { hover_bg } else { bg })),
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
        apply_widget_pick(&mut s, WidgetSource::Custom("clock".into()), "Apps", 4, &reg);
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
        apply_widget_pick(&mut s, WidgetSource::Custom("clock".into()), "My Page", 2, &[]);
        assert_eq!(s.kind, ActionKind::Widget);
        let w = s.widget.expect("widget config set");
        assert_eq!(w.source, WidgetSource::Custom("clock".into()));
        assert_eq!(w.instance_key.as_deref(), Some("my-page.slot2"));
    }

    // --- undo-by-reselect matching ---

    #[test]
    fn pick_matches_stored_slice_behavior() {
        let exec = slice(ActionKind::Exec, "x");
        assert!(pick_matches_slice(&exec, &PickChoice::Action(ActionKind::Exec)));
        assert!(!pick_matches_slice(&exec, &PickChoice::Action(ActionKind::Macro)));
        assert!(!pick_matches_slice(&exec, &PickChoice::Widget(WidgetSource::Cpu)));

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
        assert!(pick_matches_slice(&w, &PickChoice::Action(ActionKind::Widget)));
    }

    // --- display names ---

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
