//! "Indicator Popup" tab — configures the popup that opens when the
//! user clicks the JuhRadial GNOME indicator icon. The popup itself
//! is rendered by juhradial-popup (Phase 3); this tab edits the
//! `popup` config block on AppConfig that the popup binary watches
//! via inotify.

use juhradial_shared::{
    HostLabelStyle, PopupMode, QuickEntry,
    QUICK_SLIDER_CATALOG, QUICK_TOGGLE_CATALOG,
};
use juhradial_widgets::style;
use juhradial_widgets::widgets::section_header;

use iced::widget::{button, column, container, row, rule, text, toggler, Space};
use iced::{Alignment, Element, Length};

use crate::{Message, State};

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let popup = &state.config.popup;

    let sliders_card: Element<Message> = if popup.mode == PopupMode::Power {
        quick_sliders_card(state)
    } else {
        Space::new().height(Length::Fixed(0.0)).into()
    };

    column![
        section_header("Indicator Popup"),
        text(
            "Controls what appears when you click the JuhRadial icon in \
             the GNOME top bar. The popup is rendered by JuhRadial itself \
             — not by the GNOME extension — so it shares this app's \
             theming and reacts to everything you change here.",
        )
        .size(12)
        .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
        Space::new().height(Length::Fixed(8.0)),
        mode_card(state),
        Space::new().height(Length::Fixed(12.0)),
        easy_switch_card(state),
        Space::new().height(Length::Fixed(12.0)),
        quick_toggles_card(state),
        Space::new().height(Length::Fixed(12.0)),
        sliders_card,
        Space::new().height(Length::Fixed(12.0)),
        interactions_card(state),
    ]
    .spacing(10)
    .into()
}

// ============================================================================
// Mode card — Simple / Power User segmented control
// ============================================================================

fn mode_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let popup = &state.config.popup;
    let is_simple = popup.mode == PopupMode::Simple;

    let simple_btn: Element<Message> = if is_simple {
        button(
            column![
                text("Simple").size(13),
                text("Battery, device, and a small set of on/off toggles.")
                    .size(10)
                    .style(style::text_dim(pal)),
            ]
            .spacing(3),
        )
        .style(style::btn_primary(pal))
        .padding([10, 16])
        .on_press(Message::SetPopupMode(PopupMode::Simple))
        .into()
    } else {
        button(
            column![
                text("Simple").size(13),
                text("Battery, device, and a small set of on/off toggles.")
                    .size(10)
                    .style(style::text_dim(pal)),
            ]
            .spacing(3),
        )
        .style(style::btn_secondary(pal))
        .padding([10, 16])
        .on_press(Message::SetPopupMode(PopupMode::Simple))
        .into()
    };

    let power_btn: Element<Message> = if !is_simple {
        button(
            column![
                text("Power User").size(13),
                text("Adds sliders (DPI, scroll, haptic intensity) and more toggles.")
                    .size(10)
                    .style(style::text_dim(pal)),
            ]
            .spacing(3),
        )
        .style(style::btn_primary(pal))
        .padding([10, 16])
        .on_press(Message::SetPopupMode(PopupMode::Power))
        .into()
    } else {
        button(
            column![
                text("Power User").size(13),
                text("Adds sliders (DPI, scroll, haptic intensity) and more toggles.")
                    .size(10)
                    .style(style::text_dim(pal)),
            ]
            .spacing(3),
        )
        .style(style::btn_secondary(pal))
        .padding([10, 16])
        .on_press(Message::SetPopupMode(PopupMode::Power))
        .into()
    };

    container(
        column![
            text("Popup mode").size(14),
            text(
                "Choose how much the indicator popup shows. \
                 Simple keeps things clean; Power User surfaces \
                 sliders and extra toggles.",
            )
            .size(11)
            .style(style::text_dim(pal)),
            rule::horizontal(1).style(style::rule_style(pal)),
            row![simple_btn, Space::new().width(Length::Fixed(8.0)), power_btn]
                .align_y(Alignment::Start),
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

// ============================================================================
// Easy-Switch card — host buttons toggle + label style radio
// ============================================================================

fn easy_switch_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let popup = &state.config.popup;

    // Label-style radio buttons (inline styles to avoid opaque type mismatch)
    let is_hostname = popup.host_label_style == HostLabelStyle::Hostname;
    let hostname_btn: Element<Message> = if is_hostname {
        button(text("Hostname").size(12))
            .style(style::btn_primary(pal))
            .padding([6, 12])
            .on_press(Message::SetPopupHostLabelStyle(HostLabelStyle::Hostname))
            .into()
    } else {
        button(text("Hostname").size(12))
            .style(style::btn_secondary(pal))
            .padding([6, 12])
            .on_press(Message::SetPopupHostLabelStyle(HostLabelStyle::Hostname))
            .into()
    };
    let channel_btn: Element<Message> = if !is_hostname {
        button(text("Channel").size(12))
            .style(style::btn_primary(pal))
            .padding([6, 12])
            .on_press(Message::SetPopupHostLabelStyle(HostLabelStyle::Channel))
            .into()
    } else {
        button(text("Channel").size(12))
            .style(style::btn_secondary(pal))
            .padding([6, 12])
            .on_press(Message::SetPopupHostLabelStyle(HostLabelStyle::Channel))
            .into()
    };

    let label_radio = row![
        hostname_btn,
        Space::new().width(Length::Fixed(4.0)),
        channel_btn,
    ]
    .align_y(Alignment::Center);

    // Host button preview — mock 3 hosts since daemon per-device
    // host info isn't piped into settings-rs yet.
    // TODO: thread real host info from state.daemon when available.
    let mock_hosts: [Option<&str>; 3] =
        [Some("jim-thinkpad"), Some("jim-macbook"), None];

    let preview_buttons: Vec<Element<Message>> = mock_hosts
        .iter()
        .enumerate()
        .map(|(i, host)| {
            let label = match popup.host_label_style {
                HostLabelStyle::Hostname => host
                    .map(|h| h.to_string())
                    .unwrap_or_else(|| format!("Channel {}", i + 1)),
                HostLabelStyle::Channel => format!("Channel {}", i + 1),
            };
            container(
                text(label).size(11),
            )
            .padding([4, 10])
            .style(style::chip(pal))
            .into()
        })
        .collect();

    let mut preview_row = row![].spacing(6).align_y(Alignment::Center);
    for btn in preview_buttons {
        preview_row = preview_row.push(btn);
    }

    let toggle_row = row![
        column![
            text("Show host buttons").size(13),
            text(
                "Displays Easy-Switch host buttons in the popup so you \
                 can hop between paired devices without opening the full \
                 settings window.",
            )
            .size(11)
            .style(style::text_dim(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        toggler(popup.show_host_buttons)
            .on_toggle(Message::SetPopupShowHostButtons)
            .style(style::toggler_style(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    let label_row = row![
        column![
            text("Button label style").size(13),
            text("Show paired hostname or numeric channel number.")
                .size(11)
                .style(style::text_dim(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        label_radio,
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    container(
        column![
            text("Easy-Switch").size(14),
            text("Host-switching buttons shown at the top of the popup.")
                .size(11)
                .style(style::text_dim(pal)),
            rule::horizontal(1).style(style::rule_style(pal)),
            toggle_row,
            label_row,
            text("Preview:").size(11).style(style::text_dim(pal)),
            preview_row,
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

// ============================================================================
// Quick toggles card — reorderable list + available rail
// ============================================================================

fn quick_toggles_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let popup = &state.config.popup;

    let active_ids: &[String] = if popup.mode == PopupMode::Simple {
        &popup.simple_toggles
    } else {
        &popup.power_toggles
    };

    // Build the enabled (reorderable) list
    let total = active_ids.len();
    let mut enabled_col = column![].spacing(6);
    for (idx, id) in active_ids.iter().enumerate() {
        let entry = QUICK_TOGGLE_CATALOG.iter().find(|q| q.id == id);
        if let Some(entry) = entry {
            enabled_col = enabled_col.push(reorder_row(
                pal,
                entry,
                idx == 0,
                idx + 1 == total,
                Message::PopupToggleMoveUp(id.clone()),
                Message::PopupToggleMoveDown(id.clone()),
                Message::PopupToggleRemove(id.clone()),
            ));
        }
    }

    // Available (not currently enabled) rail
    let available: Vec<&QuickEntry> = QUICK_TOGGLE_CATALOG
        .iter()
        .filter(|q| !active_ids.iter().any(|a| a == q.id))
        .collect();

    let mut available_col = column![].spacing(4);
    for entry in &available {
        available_col = available_col.push(add_row(
            pal,
            entry,
            Message::PopupToggleAdd(entry.id.to_string()),
        ));
    }

    let available_section: Element<Message> = if available.is_empty() {
        text("All toggles are enabled.")
            .size(11)
            .style(style::text_dim(pal))
            .into()
    } else {
        column![
            text("Available").size(12).style(style::text_dim(pal)),
            available_col,
        ]
        .spacing(4)
        .into()
    };

    container(
        column![
            text("Quick toggles").size(14),
            text(
                "On/off switches shown in the popup. Drag the arrows to \
                 reorder; the trash icon removes a toggle from the popup \
                 (it doesn't change the underlying setting).",
            )
            .size(11)
            .style(style::text_dim(pal)),
            rule::horizontal(1).style(style::rule_style(pal)),
            enabled_col,
            rule::horizontal(1).style(style::rule_style(pal)),
            available_section,
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

// ============================================================================
// Quick sliders card — Power User mode only
// ============================================================================

fn quick_sliders_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let popup = &state.config.popup;
    let active_ids = &popup.power_sliders;

    let total = active_ids.len();
    let mut enabled_col = column![].spacing(6);
    for (idx, id) in active_ids.iter().enumerate() {
        let entry = QUICK_SLIDER_CATALOG.iter().find(|q| q.id == id);
        if let Some(entry) = entry {
            enabled_col = enabled_col.push(reorder_row(
                pal,
                entry,
                idx == 0,
                idx + 1 == total,
                Message::PopupSliderMoveUp(id.clone()),
                Message::PopupSliderMoveDown(id.clone()),
                Message::PopupSliderRemove(id.clone()),
            ));
        }
    }

    let available: Vec<&QuickEntry> = QUICK_SLIDER_CATALOG
        .iter()
        .filter(|q| !active_ids.iter().any(|a| a == q.id))
        .collect();

    let mut available_col = column![].spacing(4);
    for entry in &available {
        available_col = available_col.push(add_row(
            pal,
            entry,
            Message::PopupSliderAdd(entry.id.to_string()),
        ));
    }

    let available_section: Element<Message> = if available.is_empty() {
        text("All sliders are enabled.")
            .size(11)
            .style(style::text_dim(pal))
            .into()
    } else {
        column![
            text("Available").size(12).style(style::text_dim(pal)),
            available_col,
        ]
        .spacing(4)
        .into()
    };

    container(
        column![
            text("Quick sliders").size(14),
            text("Sliders shown in Power User mode (DPI, scroll sensitivity, etc.).")
                .size(11)
                .style(style::text_dim(pal)),
            rule::horizontal(1).style(style::rule_style(pal)),
            enabled_col,
            rule::horizontal(1).style(style::rule_style(pal)),
            available_section,
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

// ============================================================================
// Interactions card — volume-on-scroll, close-on-action, animations
// ============================================================================

fn interactions_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let popup = &state.config.popup;

    container(
        column![
            text("Interactions").size(14),
            text("Behaviour tweaks for the popup.")
                .size(11)
                .style(style::text_dim(pal)),
            rule::horizontal(1).style(style::rule_style(pal)),
            switch_row(
                state,
                "Volume on scroll",
                "Scrolling inside the popup adjusts system volume.",
                popup.volume_on_scroll,
                Message::SetPopupVolumeOnScroll,
            ),
            switch_row(
                state,
                "Close after action",
                "The popup closes automatically after you click a \
                 toggle or slider. Off = stays open so you can change \
                 multiple settings in one go.",
                popup.close_on_action,
                Message::SetPopupCloseOnAction,
            ),
            switch_row(
                state,
                "Animations",
                "Fade-in / fade-out transitions when the popup opens \
                 and closes. Disable for snappier feel on slow hardware.",
                popup.animations,
                Message::SetPopupAnimations,
            ),
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

// ============================================================================
// Helpers
// ============================================================================

/// A row for an enabled quick entry that can be moved up/down or removed.
/// `is_first` disables the up-arrow; `is_last` disables the down-arrow.
fn reorder_row<'a>(
    pal: &'a juhradial_widgets::palette::Palette,
    entry: &'a QuickEntry,
    is_first: bool,
    is_last: bool,
    move_up: Message,
    move_down: Message,
    remove: Message,
) -> Element<'a, Message> {
    let up_disabled = is_first;
    let down_disabled = is_last;

    // Icon placeholder — QuickEntry.icon is a freedesktop symbolic
    // name; rendering it would require an icon resolver pass here.
    // Use a small text glyph as a stand-in for now.
    let icon_text = text("◯").size(14).style(style::text_dim(pal));

    let label_col = column![
        text(entry.label).size(13),
        text(entry.desc)
            .size(10)
            .style(style::text_dim(pal)),
    ]
    .spacing(2);

    let mut up_btn = button(text("▲").size(10)).style(style::btn_secondary(pal));
    if !up_disabled {
        up_btn = up_btn.on_press(move_up);
    }

    let mut down_btn = button(text("▼").size(10)).style(style::btn_secondary(pal));
    if !down_disabled {
        down_btn = down_btn.on_press(move_down);
    }

    let trash_btn = button(text("✕").size(10))
        .style(style::btn_danger(pal))
        .on_press(remove);

    row![
        icon_text,
        Space::new().width(Length::Fixed(8.0)),
        label_col,
        Space::new().width(Length::Fill),
        up_btn,
        Space::new().width(Length::Fixed(2.0)),
        down_btn,
        Space::new().width(Length::Fixed(6.0)),
        trash_btn,
    ]
    .align_y(Alignment::Center)
    .into()
}

/// A row for an available (not yet enabled) quick entry with an add button.
fn add_row<'a>(
    pal: &'a juhradial_widgets::palette::Palette,
    entry: &'a QuickEntry,
    add: Message,
) -> Element<'a, Message> {
    let icon_text = text("◯").size(14).style(style::text_dim(pal));

    let label_col = column![
        text(entry.label).size(13),
        text(entry.desc)
            .size(10)
            .style(style::text_dim(pal)),
    ]
    .spacing(2);

    let add_btn = button(text("+ Add").size(10))
        .style(style::btn_secondary(pal))
        .on_press(add);

    row![
        icon_text,
        Space::new().width(Length::Fixed(8.0)),
        label_col,
        Space::new().width(Length::Fill),
        add_btn,
    ]
    .align_y(Alignment::Center)
    .into()
}

fn switch_row<'a>(
    state: &'a State,
    label: &str,
    description: &str,
    on: bool,
    msg: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    let pal = &state.palette;
    row![
        column![
            text(label.to_string()).size(13),
            text(description.to_string())
                .size(11)
                .style(style::text_dim(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        toggler(on)
            .on_toggle(msg)
            .style(style::toggler_style(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(12)
    .into()
}
