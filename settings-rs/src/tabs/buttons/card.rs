//! Expanded slice card — the design handoff's `SliceCard`
//! (`docs/design-system/menu-page.jsx`). Rendered inline in the
//! reorder-row list (mod.rs) at the selected slot's position.
//!
//! Layout, top to bottom (design order):
//!   1. header — `SLOT {n}` mono tag · `WIDGET` accent tag (when
//!      applicable) · spacer · Test · move up/down · delete ·
//!      collapse
//!   2. label input (with a dim "auto label" tag when the label
//!      still equals the picked widget's auto-label) + description
//!   3. "Slice behavior" field — behavior chip / picker panel
//!      (picker.rs), the kind-specific value editor, and the
//!      schema-driven widget options card (widget_options.rs)
//!   4. "Appearance" field — colour swatch select + icon input /
//!      browse (or the widget explanatory line) with the Visibility
//!      selector right-aligned in the same row; predicate args (if
//!      any) stack on a row below
//!   5. the submenu sub-item editor, for Submenu slices
//!
//! All controls reuse the existing Messages — this module is pure
//! view chrome.

use iced::widget::{button, column, container, pick_list, row, text, text_input, Space};
use iced::{Alignment, Element, Length};
use oxidemx_shared::{ActionKind, Slice};
use oxidemx_widgets::style;

use super::{picker, widget_options, ColorOption, VisibilityTarget, FULL_COLOR_KEY};
use crate::{Message, State};

/// The full expanded card for slot `idx`. `last_idx` is
/// `slices.len() - 1` and bounds the move buttons.
pub fn slice_card<'a>(
    state: &'a State,
    idx: usize,
    slice: &'a Slice,
    last_idx: usize,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let is_widget = slice.kind == ActionKind::Widget;

    // ------------------------------------------------------------------
    // 1. Header — SLOT tag · WIDGET tag · spacer · controls
    // ------------------------------------------------------------------
    let mut header = row![container(
        text(format!("SLOT {}", idx + 1))
            .size(9)
            .style(style::text_dim(pal))
    )
    .padding([2, 6])
    .style(style::chip(pal)),]
    .align_y(Alignment::Center)
    .spacing(8);
    if is_widget {
        header = header.push(
            container(text("WIDGET").size(8).style(style::text_accent(pal)))
                .padding([2, 6])
                .style(style::chip(pal)),
        );
    }
    header = header.push(Space::new().width(Length::Fill));

    // Test is always rendered for a stable UI; non-testable kinds
    // report a status hint from the handler.
    header = header.push(
        button(text("▶ Test").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::TestSliceAction(idx)),
    );
    let mut up_btn = button(text("↑").size(11)).style(style::btn_secondary(pal));
    if idx > 0 {
        up_btn = up_btn.on_press(Message::MoveSliceUp(idx));
    }
    let mut down_btn = button(text("↓").size(11)).style(style::btn_secondary(pal));
    if idx < last_idx {
        down_btn = down_btn.on_press(Message::MoveSliceDown(idx));
    }
    header = header
        .push(up_btn)
        .push(down_btn)
        .push(
            button(text("✕").size(11))
                .style(style::btn_danger(pal))
                .on_press(Message::DeleteSlice(idx)),
        )
        .push(
            button(text("Collapse").size(11))
                .style(style::btn_flat(pal))
                .on_press(Message::DismissSliceSelection),
        );

    // ------------------------------------------------------------------
    // 2. Label (+ "auto label" tag) and description
    // ------------------------------------------------------------------
    let label_input = text_input("Label", slice.label.as_str())
        .on_input(move |s| Message::SetSliceLabel(idx, s))
        .padding(6)
        .size(12);
    // The dim tag tells the user this label will follow the widget
    // on the next re-pick (picker.rs's should_auto_label rule).
    let auto_labelled = picker::current_auto_label(slice, &state.widget_registry)
        .is_some_and(|auto| auto == slice.label);
    let label_row: Element<Message> = if auto_labelled {
        row![
            label_input,
            container(text("auto label").size(9).style(style::text_faint(pal)))
                .padding([2, 6])
                .style(style::chip(pal)),
        ]
        .align_y(Alignment::Center)
        .spacing(8)
        .into()
    } else {
        label_input.into()
    };
    let desc_input = text_input(
        "Description (notes / tooltip — optional)",
        slice.description.as_str(),
    )
    .on_input(move |s| Message::SetSliceDescription(idx, s))
    .padding(5)
    .size(11);

    // ------------------------------------------------------------------
    // 3. "Slice behavior" — chip/picker + value editor + options card
    // ------------------------------------------------------------------
    let mut behavior_label = row![text("Slice behavior").size(11).style(style::text_dim(pal))]
        .align_y(Alignment::Center)
        .spacing(8);
    if state.picker_open == Some(idx) {
        behavior_label = behavior_label.push(
            text("single click to choose")
                .size(10)
                .style(style::text_faint(pal)),
        );
    }

    let behavior = picker::behavior_section(state, idx, slice);

    // Kind-specific value editor (command / macro / chord / host /
    // …). "Pick app…" only for shell-command kinds — see mod.rs.
    let cmd_input: Element<Message> = super::action_value_editor(state, idx, slice);
    let needs_app_pick = matches!(
        slice.kind,
        ActionKind::Exec | ActionKind::Emoji | ActionKind::Settings
    );
    let value_row: Element<Message> = if needs_app_pick {
        row![
            container(cmd_input).width(Length::Fill),
            button(text("Pick app…").size(11))
                .style(style::btn_secondary(pal))
                .on_press(Message::OpenAppCommandPicker(
                    crate::app_picker::AppCommandTarget::Slice(idx),
                )),
        ]
        .align_y(Alignment::Center)
        .spacing(8)
        .into()
    } else {
        container(cmd_input).width(Length::Fill).into()
    };

    // Schema-driven options card (spec §10d) — collapses to nothing
    // for slices that aren't a Ready custom widget with options.
    let options_card = widget_options::options_section(state, idx, slice);

    // ------------------------------------------------------------------
    // 4. "Appearance" — colour + icon, Visibility right-aligned
    // ------------------------------------------------------------------
    let selected_color = if slice.icon_untinted {
        ColorOption(FULL_COLOR_KEY.to_string())
    } else if slice.color.is_empty() {
        ColorOption("accent".to_string())
    } else {
        ColorOption(slice.color.clone())
    };
    let color_picker = pick_list(super::color_options(), Some(selected_color), move |opt| {
        Message::SetSliceColor(idx, opt.0)
    })
    .style(style::pick_list_style(pal))
    .text_size(12);

    let mut appearance_row = row![color_picker].align_y(Alignment::Center).spacing(8);
    if is_widget {
        // Widget slices draw their own slice content, so the icon
        // input is hidden (colour + visibility stay, spec §10d).
        appearance_row = appearance_row.push(
            text("The widget draws its own slice content — no icon needed.")
                .size(11)
                .style(style::text_faint(pal)),
        );
    } else {
        let icon_browse_target = crate::icon_picker::IconPickerTarget::Slice(idx);
        appearance_row = appearance_row
            .push(
                text_input(
                    "Icon (e.g. \"system-run-symbolic\" or /path/to/icon.svg)",
                    slice.icon.as_str(),
                )
                .on_input(move |s| Message::SetSliceIcon(idx, s))
                .padding(6)
                .size(12)
                .width(Length::Fill),
            )
            .push(
                button(text("Browse…").size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::OpenIconPicker(icon_browse_target)),
            )
            .push(
                button(text("From file…").size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::BrowseIconFile(icon_browse_target)),
            );
    }
    appearance_row = appearance_row
        .push(Space::new().width(Length::Fill))
        .push(text("Visibility").size(11).style(style::text_dim(pal)))
        .push(super::visibility_picker(
            state,
            VisibilityTarget::Slice { idx },
            slice.visible_if.as_ref(),
        ));

    // ------------------------------------------------------------------
    // Assemble
    // ------------------------------------------------------------------
    let mut col = column![
        header,
        label_row,
        desc_input,
        column![behavior_label, behavior, value_row, options_card].spacing(6),
        column![
            text("Appearance").size(11).style(style::text_dim(pal)),
            appearance_row,
        ]
        .spacing(6),
    ]
    .spacing(10);

    // Predicate args (path / process / env inputs) stack under the
    // Appearance row — they don't fit inline next to the selector.
    if let Some(args) = super::visibility_args(
        state,
        VisibilityTarget::Slice { idx },
        slice.visible_if.as_ref(),
    ) {
        col = col.push(args);
    }

    // Submenu editor — only when this slice hosts sub-items.
    if slice.kind == ActionKind::Submenu {
        col = col.push(super::submenu_editor(state, idx, &slice.submenu));
    }

    container(col).padding(14).style(style::card(pal)).into()
}
