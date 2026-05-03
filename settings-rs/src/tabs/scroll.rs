//! "Point & Scroll" tab — pointer speed/accel + scroll wheel
//! behaviour. Schema lives in `juhradial_shared::PointerConfig` /
//! `ScrollConfig`; values land in `~/.config/juhradial/config.json`
//! under `pointer` and `scroll` keys, matching the legacy daemon.

use crate::widgets::{labeled_int_slider, section_header};
use crate::{style, Message, State};
use iced::widget::{column, container, pick_list, row, rule, text, toggler, Space};
use iced::{Alignment, Element, Length};

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    column![
        section_header("Point & Scroll"),
        text(
            "Pointer feel + scroll wheel behaviour. The daemon hot-reads \
             these and re-applies on the next config change.",
        )
        .size(12)
        .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
        Space::new().height(Length::Fixed(8.0)),
        dpi_card(state),
        Space::new().height(Length::Fixed(12.0)),
        pointer_card(state),
        Space::new().height(Length::Fixed(12.0)),
        scroll_card(state),
    ]
    .spacing(10)
    .into()
}

// ============================================================================
// DPI card — live read + write via daemon over D-Bus
// ============================================================================

fn dpi_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let dpi = state.daemon.dpi.unwrap_or(1000);
    let supported = state.daemon.dpi_supported;

    let header = row![
        text("Sensitivity (DPI)").size(14),
        Space::new().width(Length::Fill),
        text(if supported {
            format!("{dpi}")
        } else {
            "—".into()
        })
        .size(13)
        .style(style::text_dim(pal)),
    ]
    .align_y(Alignment::Center);

    let body: Element<Message> = if supported {
        column![
            text(
                "Pointer DPI on the device (HID++). Slider applies live \
                 to the mouse — no daemon reload needed."
            )
            .size(11)
            .style(style::text_dim(pal)),
            crate::widgets::labeled_int_slider(
                "DPI",
                dpi as u32,
                400..=8000,
                |v| format!("{v} DPI"),
                |v| Message::SetDpi(v as u16),
            ),
            // Quick presets — each writes immediately.
            row![
                preset_button(state, "400", 400),
                preset_button(state, "800", 800),
                preset_button(state, "1200", 1200),
                preset_button(state, "1600", 1600),
                preset_button(state, "2400", 2400),
                preset_button(state, "3200", 3200),
            ]
            .spacing(6),
        ]
        .spacing(10)
        .into()
    } else {
        text(
            "DPI is not supported on this device, or the daemon \
             hasn't connected yet. Make sure juhradiald is running \
             and the mouse is paired via the Bolt receiver.",
        )
        .size(12)
        .style(style::text_dim(pal))
        .into()
    };

    container(
        column![header, rule::horizontal(1).style(style::rule_style(pal)), body].spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

fn preset_button<'a>(state: &'a State, label: &str, value: u16) -> Element<'a, Message> {
    let pal = &state.palette;
    iced::widget::button(text(label.to_string()).size(11))
        .style(style::btn_secondary(pal))
        .on_press(Message::SetDpi(value))
        .into()
}

// ============================================================================
// Pointer card
// ============================================================================

fn pointer_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let p = &state.config.pointer;
    container(
        column![
            text("Pointer").size(14),
            text("Cursor speed + libinput acceleration profile.")
                .size(11)
                .style(style::text_dim(pal)),
            rule::horizontal(1).style(style::rule_style(pal)),
            labeled_int_slider(
                "Speed",
                p.speed,
                1..=20,
                |v| format!("{v} / 20"),
                Message::SetPointerSpeed,
            ),
            row![
                column![
                    text("Acceleration").size(13),
                    text(
                        "Adaptive cursor velocity — slow movements stay fine, \
                         fast movements travel further. Disable for a flat \
                         pointer-to-pixel ratio.",
                    )
                    .size(11)
                    .style(style::text_dim(pal)),
                ]
                .spacing(2),
                Space::new().width(Length::Fill),
                toggler(p.acceleration)
                    .on_toggle(Message::SetPointerAcceleration)
                    .style(style::toggler_style(pal)),
            ]
            .align_y(Alignment::Center)
            .spacing(12),
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

// ============================================================================
// Scroll card
// ============================================================================

fn scroll_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let s = &state.config.scroll;

    let mode_options: Vec<ScrollMode> = SCROLL_MODES
        .iter()
        .map(|(slug, label)| ScrollMode {
            slug: (*slug).to_string(),
            label: (*label).to_string(),
        })
        .collect();
    let current_mode = mode_options
        .iter()
        .find(|m| m.slug == s.mode)
        .cloned()
        .unwrap_or_else(|| mode_options[0].clone());
    let mode_picker = pick_list(mode_options, Some(current_mode), |m: ScrollMode| {
        Message::SetScrollMode(m.slug)
    })
    .style(style::pick_list_style(pal))
    .text_size(13);

    container(
        column![
            text("Scroll wheel").size(14),
            text("Direction, smoothness, and MX SmartShift behaviour.")
                .size(11)
                .style(style::text_dim(pal)),
            rule::horizontal(1).style(style::rule_style(pal)),
            row![
                text("Wheel mode").size(13).width(Length::Fixed(140.0)),
                Space::new().width(Length::Fill),
                mode_picker,
            ]
            .align_y(Alignment::Center)
            .spacing(8),
            switch_row(
                state,
                "Natural scrolling",
                "Reverse scroll direction (macOS-style).",
                s.natural,
                Message::SetScrollNatural,
            ),
            switch_row(
                state,
                "Smooth scrolling",
                "Sub-line smoothing instead of ratcheted line steps.",
                s.smooth,
                Message::SetScrollSmooth,
            ),
            switch_row(
                state,
                "SmartShift",
                "Auto-disengage the ratchet when the wheel spins fast \
                 enough — MX-specific.",
                s.smartshift,
                Message::SetScrollSmartshift,
            ),
            labeled_int_slider(
                "SmartShift threshold",
                s.smartshift_threshold,
                1..=100,
                |v| format!("{v}"),
                Message::SetScrollSmartshiftThreshold,
            ),
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
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
        toggler(on).on_toggle(msg).style(style::toggler_style(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(12)
    .into()
}

const SCROLL_MODES: &[(&str, &str)] = &[
    ("smartshift", "SmartShift (auto)"),
    ("ratchet", "Ratchet (clicky)"),
    ("free", "Free spin"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScrollMode {
    slug: String,
    label: String,
}

impl std::fmt::Display for ScrollMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}
