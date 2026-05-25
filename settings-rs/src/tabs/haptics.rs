//! "Haptic Feedback" tab — enable toggle + per-event pattern
//! pickers + debounce sliders. Mirrors the legacy Python overlay's
//! HapticsPage.
//!
//! All edits go to `state.config.haptics`; the overlay's daemon
//! reads the same JSON and re-applies on the next reload.

use juhradial_widgets::widgets::{labeled_int_slider, section_header};
use crate::{Message, State};
use juhradial_widgets::style;
use iced::widget::{column, container, pick_list, row, rule, text, toggler, Space};
use iced::{Alignment, Element, Length};
use juhradial_shared::HAPTIC_PATTERNS;

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let h = &state.config.haptics;

    let enable_card = container(
        row![
            column![
                text("Haptic Feedback").size(15),
                text("Feel vibrations when using the radial menu.")
                    .size(12)
                    .style(style::text_dim(pal)),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            toggler(h.enabled)
                .on_toggle(Message::SetHapticsEnabled)
                .style(style::toggler_style(pal)),
        ]
        .align_y(Alignment::Center)
        .spacing(12),
    )
    .padding(14)
    .style(style::card(pal));

    let per_event_card = container(
        column![
            text("Per-event patterns").size(14),
            text(
                "Pick a pattern for each radial-menu event. \"Default\" \
                 below covers any event not explicitly configured."
            )
            .size(11)
            .style(style::text_dim(pal)),
            rule::horizontal(1).style(style::rule_style(pal)),
            event_row(
                state,
                "Menu opens",
                &h.per_event.menu_appear,
                |s| Message::SetHapticsPerEvent(HapticsEvent::MenuAppear, s),
                Some("menu_appear"),
            ),
            event_row(
                state,
                "Slice change",
                &h.per_event.slice_change,
                |s| Message::SetHapticsPerEvent(HapticsEvent::SliceChange, s),
                Some("slice_change"),
            ),
            event_row(
                state,
                "Page change",
                &h.per_event.page_change,
                |s| Message::SetHapticsPerEvent(HapticsEvent::PageChange, s),
                Some("page_change"),
            ),
            event_row(
                state,
                "Submenu opens",
                &h.per_event.submenu_open,
                |s| Message::SetHapticsPerEvent(HapticsEvent::SubmenuOpen, s),
                Some("submenu_open"),
            ),
            event_row(
                state,
                "Submenu closes",
                &h.per_event.submenu_close,
                |s| Message::SetHapticsPerEvent(HapticsEvent::SubmenuClose, s),
                Some("submenu_close"),
            ),
            event_row(
                state,
                "Confirm / dispatch",
                &h.per_event.confirm,
                |s| Message::SetHapticsPerEvent(HapticsEvent::Confirm, s),
                Some("confirm"),
            ),
            event_row(
                state,
                "Invalid action",
                &h.per_event.invalid,
                |s| Message::SetHapticsPerEvent(HapticsEvent::Invalid, s),
                Some("invalid"),
            ),
            event_row(
                state,
                "Default",
                &h.default_pattern,
                Message::SetHapticsDefaultPattern,
                // No test button on Default — it's a fallback,
                // not an event that fires on its own. The daemon's
                // trigger_haptic only knows the named events above.
                None,
            ),
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal));

    let debounce_card = container(
        column![
            text("Debounce timings").size(14),
            text(
                "Minimum time between haptic triggers. Raise these if the \
                 motor feels overworked or you get rapid double-taps."
            )
            .size(11)
            .style(style::text_dim(pal)),
            rule::horizontal(1).style(style::rule_style(pal)),
            labeled_int_slider(
                "Global debounce",
                h.debounce_ms,
                0..=200,
                |v| format!("{v} ms"),
                Message::SetHapticsDebounce,
            ),
            labeled_int_slider(
                "Slice-change debounce",
                h.slice_debounce_ms,
                0..=200,
                |v| format!("{v} ms"),
                Message::SetHapticsSliceDebounce,
            ),
            labeled_int_slider(
                "Re-entry debounce",
                h.reentry_debounce_ms,
                0..=500,
                |v| format!("{v} ms"),
                Message::SetHapticsReentryDebounce,
            ),
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal));

    column![
        section_header("Haptic Feedback"),
        text("MX Master 4 haptic patterns — disabled on non-Logitech devices.")
            .size(12)
            .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
        Space::new().height(Length::Fixed(8.0)),
        enable_card,
        Space::new().height(Length::Fixed(12.0)),
        per_event_card,
        Space::new().height(Length::Fixed(12.0)),
        debounce_card,
    ]
    .spacing(10)
    .into()
}

fn event_row<'a>(
    state: &'a State,
    label: &str,
    current: &str,
    on_change: impl Fn(String) -> Message + 'a,
    test_event: Option<&'static str>,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let options: Vec<PatternChoice> = HAPTIC_PATTERNS
        .iter()
        .map(|(slug, name, _)| PatternChoice {
            slug: (*slug).to_string(),
            display: (*name).to_string(),
        })
        .collect();
    let selected = options.iter().find(|c| c.slug == current).cloned();

    let picker = pick_list(options, selected, move |c: PatternChoice| on_change(c.slug))
        .style(style::pick_list_style(pal))
        .text_size(12);

    let mut row = row![
        text(label.to_string()).size(13).width(Length::Fixed(180.0)),
        Space::new().width(Length::Fill),
        picker,
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    if let Some(event) = test_event {
        let test_btn = iced::widget::button(text("Test").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::TestHapticEvent(event.to_string()));
        row = row.push(test_btn);
    }

    row.into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HapticsEvent {
    MenuAppear,
    SliceChange,
    Confirm,
    Invalid,
    PageChange,
    SubmenuOpen,
    SubmenuClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PatternChoice {
    slug: String,
    display: String,
}

impl std::fmt::Display for PatternChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}
