//! "Point & Scroll" tab — pointer speed/accel + scroll wheel
//! behaviour. Schema lives in `oxidemx_shared::PointerConfig` /
//! `ScrollConfig`; values land in `~/.config/oxidemx/config.json`
//! under `pointer` and `scroll` keys, matching the legacy daemon.

use crate::{Message, State};
use iced::widget::{column, container, pick_list, row, rule, text, toggler, Space};
use iced::{Alignment, Element, Length};
use oxidemx_widgets::style;
use oxidemx_widgets::widgets::{labeled_int_slider, section_header};

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
        Space::new().height(Length::Fixed(12.0)),
        hires_scroll_card(state),
    ]
    .spacing(10)
    .into()
}

// ============================================================================
// HiResScroll card — three device-level wheel toggles (live HID++).
// ============================================================================

fn hires_scroll_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let hrs = state.daemon.hiresscroll;

    let body: Element<Message> = match hrs {
        Some(h) => column![
            switch_row(
                state,
                "High-resolution wheel",
                "More events per detent — gives smooth, sub-line scroll \
                 in compatible apps (browsers, IDEs).",
                h.hires,
                Message::SetHiResScrollHires,
            ),
            switch_row(
                state,
                "Invert wheel direction",
                "Flips the wheel direction at the device level. Stacks \
                 with the OS-level natural-scroll toggle in Scroll above.",
                h.invert,
                Message::SetHiResScrollInvert,
            ),
            switch_row(
                state,
                "Target HID directly",
                "Routes scroll events through HID instead of the device's \
                 firmware-translated path. Default on; turn off only if \
                 your DE consumes wheel events twice.",
                h.target,
                Message::SetHiResScrollTarget,
            ),
        ]
        .spacing(10)
        .into(),
        None => text(
            "HiResScroll isn't supported on this device, or the daemon \
             hasn't connected yet.",
        )
        .size(12)
        .style(style::text_dim(pal))
        .into(),
    };

    container(
        column![
            text("HiResScroll (HID++)").size(14),
            text(
                "Direct wheel-protocol controls. Writes apply to the \
                 device immediately — no daemon reload needed."
            )
            .size(11)
            .style(style::text_dim(pal)),
            rule::horizontal(1).style(style::rule_style(pal)),
            body,
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
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
            oxidemx_widgets::widgets::labeled_int_slider(
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
             hasn't connected yet. Make sure oxidemxd is running \
             and the mouse is paired via the Bolt receiver.",
        )
        .size(12)
        .style(style::text_dim(pal))
        .into()
    };

    container(
        column![
            header,
            rule::horizontal(1).style(style::rule_style(pal)),
            body
        ]
        .spacing(10),
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
    // Resolve the current mode against SCROLL_MODES, with a
    // back-compat alias: legacy "free" → "freespin". Any other
    // unknown slug falls back to the first option (smartshift).
    let resolved_mode = if s.mode == "free" {
        "freespin"
    } else {
        s.mode.as_str()
    };
    let current_mode = mode_options
        .iter()
        .find(|m| m.slug == resolved_mode)
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
                "Reverse vertical scroll direction (macOS-style). \
                 Applies to both axes if your compositor doesn't \
                 split horizontal — see the dedicated horizontal \
                 toggle below for axis-only inversion.",
                s.natural,
                Message::SetScrollNatural,
            ),
            switch_row(
                state,
                "Reverse horizontal scroll",
                "Flip the thumb wheel's left/right direction. \
                 Routes the wheel through HID++ and re-emits via a \
                 small uinput device — the MX Master 4 firmware's \
                 invert bit alone has no effect, so the daemon does \
                 the flip in software. Toggling off restores normal \
                 kernel-managed scrolling.",
                s.horizontal_invert,
                Message::SetScrollHorizontalInvert,
            ),
            // Smooth-scrolling toggle removed — compositor-level
            // smoothing isn't part of any daemon path we can drive
            // (it's owned by GNOME / KWin / wlroots), so a control
            // here would be misleading. Field still exists in the
            // schema for back-compat; just hidden from the UI.
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

// Slugs match the legacy Python settings (proven working on
// MX Master 4) so older config.json files round-trip cleanly:
//   * "smartshift" — clicky-at-rest, auto-disengage past threshold
//   * "ratchet"    — clicky always
//   * "freespin"   — free always (legacy Python wrote "freespin",
//                    early Rust wrote "free"; both still accepted
//                    on the daemon side for back-compat)
const SCROLL_MODES: &[(&str, &str)] = &[
    ("smartshift", "SmartShift (auto)"),
    ("ratchet", "Ratchet (clicky)"),
    ("freespin", "Free spin"),
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
