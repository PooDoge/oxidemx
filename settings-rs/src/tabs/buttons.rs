//! "Buttons" tab — centerpiece of the legacy juhradial UI:
//!   - mouse photo + button-target callouts
//!   - per-button assignments list
//!   - right column: live "Actions Ring" preview + Easy-Switch
//!     toggle + slices editor (add / remove / reorder / edit)

use crate::mouse_callouts::mouse_widget;
use crate::radial_preview::radial_preview_widget;
use crate::widgets::section_header;
use crate::{style, Message, State};
use iced::widget::{button, column, container, pick_list, row, rule, text, text_input, toggler, Space};
use iced::{Alignment, Element, Length};
use juhradial_shared::{ActionKind, Slice};
use std::path::PathBuf;

const MOUSE_IMAGE_PATH: &str = "assets/devices/logitechmouse.png";

// ============================================================================
// View entry
// ============================================================================

pub fn view(state: &State) -> Element<'_, Message> {
    row![
        center_panel(state),
        Space::new().width(Length::Fixed(20.0)),
        right_panel(state),
    ]
    .spacing(0)
    .into()
}

// ============================================================================
// Center: mouse photo + callout list
// ============================================================================

fn center_panel(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    let mut button_col = column![].spacing(4);
    for mb in juhradial_shared::MouseButton::all() {
        button_col = button_col.push(button_assignment_row(state, *mb));
    }

    let mouse_image = mouse_photo(state);

    container(
        column![
            section_header("MX Master 4"),
            text("Click any action below to remap that button.")
                .size(12)
                .style(style::text_dim(pal)),
            container(mouse_image)
                .padding(8)
                .center_x(Length::Fill),
            container(button_col)
                .padding(12)
                .style(style::card_quiet(pal))
                .width(Length::Fill),
        ]
        .spacing(14),
    )
    .width(Length::FillPortion(2))
    .into()
}

fn mouse_photo(state: &State) -> Element<'_, Message> {
    // Canvas-based mouse photo + overlaid callouts. Width and
    // height keep ~3:4 ratio (the asset's native aspect) and leave
    // generous space on the left for the L-shaped thumb labels +
    // top space for the wheel labels.
    let path = locate_mouse_image();
    mouse_widget(&state.palette, path, 540.0, 560.0)
}

fn locate_mouse_image() -> PathBuf {
    // Try sibling-of-binary first (deployed installs), then walk up
    // for a workspace-root assets/ dir (cargo build / dev runs).
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        for _ in 0..6 {
            let candidate = cur.join(MOUSE_IMAGE_PATH);
            if candidate.exists() {
                return candidate;
            }
            if !cur.pop() {
                break;
            }
        }
    }
    PathBuf::from(MOUSE_IMAGE_PATH)
}

fn button_assignment_row(state: &State, mb: juhradial_shared::MouseButton) -> Element<'_, Message> {
    let pal = &state.palette;
    let current = mb.get(&state.config.buttons);

    let options: Vec<ActionOption> = juhradial_shared::ButtonAction::all()
        .iter()
        .map(|a| ActionOption(*a))
        .collect();
    let selected = ActionOption(current);

    let picker = pick_list(options, Some(selected), move |opt: ActionOption| {
        Message::SetButtonAssignment(mb, opt.0)
    })
    .style(style::pick_list_style(pal))
    .text_size(12);

    container(
        row![
            container(text("⌖").size(13).style(style::text_dim(pal)))
                .padding([4, 8])
                .style(style::chip(pal)),
            text(mb.label()).size(13),
            Space::new().width(Length::Fill),
            picker,
        ]
        .align_y(Alignment::Center)
        .spacing(12)
        .padding([6, 8]),
    )
    .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActionOption(juhradial_shared::ButtonAction);

impl std::fmt::Display for ActionOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.label())
    }
}

// ============================================================================
// Right column: Actions Ring + Easy-Switch + Slices editor
// ============================================================================

fn right_panel(state: &State) -> Element<'_, Message> {
    container(
        column![
            radial_preview_card(state),
            easy_switch_panel(state),
            selected_slice_editor(state),
        ]
        .spacing(16),
    )
    .width(Length::FillPortion(1))
    .into()
}

/// Live radial preview — interactive Canvas. Click a slice to
/// select it (the editor below pins to that slot). Drag a slice
/// onto another slot to swap them.
fn radial_preview_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let v = &state.config.radial_menu.visuals;
    let font = crate::fonts::resolve(&v.font_family);
    let preview = radial_preview_widget(
        pal,
        &state.config.radial_menu.slices,
        state.selected_slice,
        state.icons.clone(),
        state.iced_handles.clone(),
        font,
        v.center_label_size,
        320.0,
    );
    container(
        column![
            row![
                text("Radial menu preview").size(14),
                Space::new().width(Length::Fill),
                text("Click to select · Drag to reorder")
                    .size(10)
                    .style(style::text_faint(pal)),
            ]
            .align_y(Alignment::Center),
            rule::horizontal(1).style(style::rule_style(pal)),
            container(preview).center_x(Length::Fill).padding(8),
        ]
        .spacing(8),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

/// Selected-slice editor — replaces the legacy "list every slice
/// inline" panel. When a slice is clicked in the preview, this
/// panel pins to it and shows the full editor; "+ Add slice" adds
/// to the end (and selects it).
fn selected_slice_editor(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    let header = row![
        text("Slice editor").size(14),
        Space::new().width(Length::Fill),
        button(text("+ Add slice").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::AddSlice),
    ]
    .align_y(Alignment::Center);

    let body: Element<Message> = match state.selected_slice {
        Some(idx) if idx < state.config.radial_menu.slices.len() => {
            let last = state.config.radial_menu.slices.len().saturating_sub(1);
            slice_editor_row(state, idx, &state.config.radial_menu.slices[idx], last)
        }
        _ => container(
            text("Click a slice in the preview above to edit it.")
                .size(12)
                .style(style::text_dim(pal)),
        )
        .padding(12)
        .into(),
    };

    container(
        column![header, rule::horizontal(1).style(style::rule_style(pal)), body].spacing(8),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

fn easy_switch_panel(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let enabled = state.config.radial_menu.easy_switch_shortcuts;
    container(
        row![
            container(text("ES").size(11).style(style::text_dim(pal)))
                .padding([3, 8])
                .style(style::chip(pal)),
            column![
                text("Easy-Switch Shortcuts").size(13),
                text("Replace Emoji slot with a 1-2-3 host submenu")
                    .size(11)
                    .style(style::text_dim(pal)),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            toggler(enabled)
                .on_toggle(Message::SetEasySwitchShortcuts)
                .style(style::toggler_style(pal)),
        ]
        .align_y(Alignment::Center)
        .spacing(12),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

// ============================================================================
// Per-slice editor row (used by selected_slice_editor above).
// ============================================================================

fn slice_editor_row<'a>(
    state: &'a State,
    idx: usize,
    slice: &'a Slice,
    last_idx: usize,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let label_input = text_input("Label", slice.label.as_str())
        .on_input(move |s| Message::SetSliceLabel(idx, s))
        .padding(6)
        .size(12);
    let cmd_input = text_input("Command", slice.command.as_str())
        .on_input(move |s| Message::SetSliceCommand(idx, s))
        .padding(6)
        .size(12);

    let kind_picker = pick_list(
        KIND_OPTIONS.as_slice(),
        Some(KindOption::from(slice.kind)),
        move |opt| Message::SetSliceKind(idx, opt.into()),
    )
    .style(style::pick_list_style(pal))
    .text_size(12);

    let color_picker = pick_list(
        color_options(),
        Some(ColorOption(if slice.color.is_empty() {
            "accent".to_string()
        } else {
            slice.color.clone()
        })),
        move |opt| Message::SetSliceColor(idx, opt.0),
    )
    .style(style::pick_list_style(pal))
    .text_size(12);

    let mut up_btn = button(text("↑").size(11)).style(style::btn_secondary(pal));
    if idx > 0 {
        up_btn = up_btn.on_press(Message::MoveSliceUp(idx));
    }
    let mut down_btn = button(text("↓").size(11)).style(style::btn_secondary(pal));
    if idx < last_idx {
        down_btn = down_btn.on_press(Message::MoveSliceDown(idx));
    }
    let del_btn = button(text("✕").size(11))
        .style(style::btn_danger(pal))
        .on_press(Message::DeleteSlice(idx));

    let header = row![
        text(format!("Slot {}", idx + 1))
            .size(11)
            .style(style::text_faint(pal)),
        Space::new().width(Length::Fill),
        up_btn,
        down_btn,
        del_btn,
    ]
    .align_y(Alignment::Center)
    .spacing(6);

    container(
        column![
            header,
            label_input,
            row![kind_picker, color_picker, cmd_input.width(Length::Fill)]
                .align_y(Alignment::Center)
                .spacing(8),
        ]
        .spacing(6),
    )
    .padding(10)
    .style(style::card_quiet(pal))
    .into()
}

// ============================================================================
// PickList option wrappers
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindOption(pub ActionKind);

impl From<ActionKind> for KindOption {
    fn from(k: ActionKind) -> Self {
        KindOption(k)
    }
}
impl From<KindOption> for ActionKind {
    fn from(o: KindOption) -> Self {
        o.0
    }
}
impl std::fmt::Display for KindOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            ActionKind::Exec => "Run command",
            ActionKind::Submenu => "Submenu",
            ActionKind::Macro => "Macro",
            ActionKind::EasySwitch => "Easy-Switch",
            ActionKind::Settings => "Open Settings",
            ActionKind::Emoji => "Emoji picker",
            ActionKind::Shortcut => "Keyboard shortcut",
            ActionKind::None => "Do nothing",
        })
    }
}

const KIND_OPTIONS: [KindOption; 8] = [
    KindOption(ActionKind::Exec),
    KindOption(ActionKind::Submenu),
    KindOption(ActionKind::Macro),
    KindOption(ActionKind::Shortcut),
    KindOption(ActionKind::EasySwitch),
    KindOption(ActionKind::Settings),
    KindOption(ActionKind::Emoji),
    KindOption(ActionKind::None),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorOption(pub String);

impl std::fmt::Display for ColorOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// Catppuccin-style colour key list (matches juhradial_shared's
// ThemeColors::slice_color_rgba lookup).
fn color_options() -> Vec<ColorOption> {
    [
        "accent", "green", "yellow", "red", "blue", "mauve", "pink", "peach", "teal", "sapphire",
        "lavender",
    ]
    .iter()
    .map(|s| ColorOption(s.to_string()))
    .collect()
}

// We rebuild the list lazily per render — pick_list takes the Vec
// by `Borrow<[T]>`, so the allocation lives only as long as the
// rendered Element.
