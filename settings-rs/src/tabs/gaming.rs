//! "Gaming" tab — gaming-mode toggle, DPI cycle, and rumble→haptic redirect.
//!
//! The daemon's `gaming::GamingMode` raises pointer DPI to a
//! preset, suppresses the radial overlay, and (optionally)
//! mutes haptics so the wheel ratchet doesn't fire mid-game. The
//! tab surfaces three groups of controls:
//!
//!   * **Master toggle** — calls `SetGamingMode(bool)` over D-Bus,
//!     daemon enables / disables.
//!   * **Cycle DPI** — calls `CycleGamingDpi()`. Daemon walks its
//!     preset list and reports back the new label.
//!   * **Game Rumble → Haptic** (this iteration) — config-only
//!     knobs that the daemon will pick up via inotify once the
//!     `gamepad_haptics` module ships. Until then the daemon
//!     reads the block and ignores it; users can pre-tune their
//!     defaults from the UI.
//!
//! See `oxidemx/HAPTIC_GAMEPAD_BRIDGE_DESIGN.md` §11 for the
//! design of the redirect card.

use crate::{Message, State};
use iced::widget::{button, column, container, pick_list, row, rule, text, toggler, Space};
use iced::{Alignment, Element, Length};
use oxidemx_shared::{HapticEventMode, HapticRedirectCurve, HapticRedirectMode};
use oxidemx_widgets::style;
use oxidemx_widgets::widgets::{labeled_int_slider, labeled_slider, section_header};

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let on = state.daemon.gaming_mode;

    let toggle_card = container(
        row![
            container(text("G").size(11).style(style::text_dim(pal)))
                .padding([3, 8])
                .style(style::chip(pal)),
            column![
                text("Gaming Mode").size(14),
                text(if on {
                    "On — DPI raised, overlay suppressed."
                } else {
                    "Off — normal pointer behaviour."
                })
                .size(11)
                .style(style::text_dim(pal)),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            toggler(on)
                .on_toggle(Message::SetGamingMode)
                .style(style::toggler_style(pal)),
        ]
        .align_y(Alignment::Center)
        .spacing(12),
    )
    .padding(14)
    .style(style::card(pal));

    let dpi_card = container(
        column![
            row![
                text("Gaming DPI").size(14),
                Space::new().width(Length::Fill),
                iced::widget::button(text("Cycle preset").size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::CycleGamingDpi),
            ]
            .align_y(Alignment::Center),
            text(
                "Walks through the preset DPI list (e.g. 1600 → 3200 → \
                 4800) on each click. Useful for hot-binding to a side \
                 button via the macro recorder.",
            )
            .size(11)
            .style(style::text_dim(pal)),
        ]
        .spacing(8),
    )
    .padding(14)
    .style(style::card(pal));

    let redirect_card = redirect_card_view(state);

    column![
        section_header("Gaming"),
        text(
            "Per-game pointer + haptics overrides. Toggle gaming mode \
             when starting a session — the daemon raises DPI, suppresses \
             the radial overlay, and (optionally) mutes haptics.",
        )
        .size(12)
        .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
        Space::new().height(Length::Fixed(8.0)),
        toggle_card,
        Space::new().height(Length::Fixed(12.0)),
        dpi_card,
        Space::new().height(Length::Fixed(12.0)),
        redirect_card,
    ]
    .spacing(10)
    .into()
}

/// "Game Rumble → Haptic" card. Edits
/// `config.gaming.haptic_redirect`; daemon will hot-reload via the
/// existing inotify path once the `gamepad_haptics` module is
/// wired.
fn redirect_card_view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let hr = &state.config.gaming.haptic_redirect;
    let enabled = hr.enabled;

    let header = row![
        container(text("R").size(11).style(style::text_dim(pal)))
            .padding([3, 8])
            .style(style::chip(pal)),
        column![
            text("Game Rumble → Haptic").size(14),
            text(if enabled {
                "On — redirecting gamepad FF_RUMBLE to the MX Master 4 piezo."
            } else {
                "Off — rumble plays on the controller as usual."
            })
            .size(11)
            .style(style::text_dim(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        toggler(enabled)
            .on_toggle(Message::SetHapticRedirectEnabled)
            .style(style::toggler_style(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    let behaviour_section = column![
        text("Behaviour").size(13),
        rule::horizontal(1).style(style::rule_style(pal)),
        row![
            text("Mode").size(13).width(Length::Fixed(140.0)),
            Space::new().width(Length::Fill),
            pick_list(
                HapticRedirectMode::ALL.as_slice(),
                Some(hr.mode),
                Message::SetHapticRedirectMode,
            )
            .style(style::pick_list_style(pal))
            .text_size(13),
        ]
        .align_y(Alignment::Center)
        .spacing(8),
        row![
            text("Curve").size(13).width(Length::Fixed(140.0)),
            Space::new().width(Length::Fill),
            pick_list(
                HapticRedirectCurve::ALL.as_slice(),
                Some(hr.curve),
                Message::SetHapticRedirectCurve,
            )
            .style(style::pick_list_style(pal))
            .text_size(13),
        ]
        .align_y(Alignment::Center)
        .spacing(8),
        row![
            text("Event mode").size(13).width(Length::Fixed(140.0)),
            Space::new().width(Length::Fill),
            pick_list(
                HapticEventMode::ALL.as_slice(),
                Some(hr.event_mode),
                Message::SetHapticRedirectEventMode,
            )
            .style(style::pick_list_style(pal))
            .text_size(13),
        ]
        .align_y(Alignment::Center)
        .spacing(8),
    ]
    .spacing(6);

    let strength_section = column![
        text("Strength").size(13),
        rule::horizontal(1).style(style::rule_style(pal)),
        labeled_slider(
            "Intensity scale",
            hr.intensity_scale,
            0.0..=2.0,
            0.05,
            |v| format!("{v:.2}×"),
            Message::SetHapticRedirectIntensityScale,
        ),
        labeled_slider(
            "Min intensity (deadzone)",
            hr.min_intensity,
            0.0..=0.5,
            0.01,
            |v| format!("{v:.2}"),
            Message::SetHapticRedirectMinIntensity,
        ),
        labeled_slider(
            "Strong-motor weight",
            hr.strong_weight,
            0.0..=2.0,
            0.05,
            |v| format!("{v:.2}"),
            Message::SetHapticRedirectStrongWeight,
        ),
        labeled_slider(
            "Weak-motor weight",
            hr.weak_weight,
            0.0..=2.0,
            0.05,
            |v| format!("{v:.2}"),
            Message::SetHapticRedirectWeakWeight,
        ),
        labeled_int_slider(
            "Throttle",
            hr.throttle_ms as u32,
            10..=200,
            |v| format!("{v} ms"),
            |v| Message::SetHapticRedirectThrottleMs(v as u16),
        ),
    ]
    .spacing(8);

    let compat_section = column![
        text("Compatibility").size(13),
        rule::horizontal(1).style(style::rule_style(pal)),
        row![
            toggler(hr.passthrough_to_pad)
                .on_toggle(Message::SetHapticRedirectPassthroughToPad)
                .style(style::toggler_style(pal)),
            column![
                text("Passthrough rumble to real controller too").size(13),
                text(
                    "Forwards the FF effect to the wrapped pad in addition \
                     to firing the mouse haptic. Off by default."
                )
                .size(11)
                .style(style::text_dim(pal)),
            ]
            .spacing(2),
        ]
        .align_y(Alignment::Center)
        .spacing(10),
        row![
            toggler(hr.hard_hide_real_controller)
                .on_toggle(Message::SetHapticRedirectHardHide)
                .style(style::toggler_style(pal)),
            column![
                text("Hard-hide real controller from games (advanced)").size(13),
                text(
                    "Drops a transient udev rule that sets \
                     ID_INPUT_JOYSTICK=0 on the wrapped controller. \
                     Survives non-SDL enumeration paths but requires a \
                     brief disconnect-reconnect."
                )
                .size(11)
                .style(style::text_dim(pal)),
            ]
            .spacing(2),
        ]
        .align_y(Alignment::Center)
        .spacing(10),
        labeled_int_slider(
            "Keep gamepad 'awake'",
            hr.keep_gamepad_active_secs as u32,
            0..=60,
            |v| if v == 0 {
                "off".to_string()
            } else {
                format!("every {v} s")
            },
            |v| Message::SetHapticRedirectKeepGamepadActive(v as u16),
        ),
    ]
    .spacing(8);

    // Test fires a one-shot pulse through the daemon; Diagnose
    // pulls a report on Steam / virtual pads / controllers.
    let actions = row![
        button(text("Test haptic").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::TestHapticRedirect),
        button(text("Diagnose").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::DiagnoseHapticRedirect),
    ]
    .spacing(8);

    // The last diagnostic report, if the user has run one. Rendered
    // monospace so the report's column alignment holds.
    let diagnosis: Element<'_, Message> = match &state.haptic_diagnosis {
        Some(report) => container(
            text(report.as_str())
                .size(11)
                .font(iced::Font::MONOSPACE)
                .style(style::text_dim(pal)),
        )
        .padding(10)
        .width(Length::Fill)
        .style(style::card(pal))
        .into(),
        None => Space::new().height(Length::Fixed(0.0)).into(),
    };

    let footnote = text(
        "Changes are written to config.json now; the daemon applies them \
         the next time Game Mode is toggled (live mid-session reload is a \
         planned refinement). See HAPTIC_GAMEPAD_BRIDGE_DESIGN.md.",
    )
    .size(11)
    .style(style::text_dim(pal));

    container(
        column![
            header,
            Space::new().height(Length::Fixed(8.0)),
            behaviour_section,
            Space::new().height(Length::Fixed(8.0)),
            strength_section,
            Space::new().height(Length::Fixed(8.0)),
            compat_section,
            Space::new().height(Length::Fixed(10.0)),
            actions,
            Space::new().height(Length::Fixed(8.0)),
            diagnosis,
            Space::new().height(Length::Fixed(8.0)),
            footnote,
        ]
        .spacing(4),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}
