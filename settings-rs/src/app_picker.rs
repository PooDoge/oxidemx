//! Inline application picker — given an installed `.desktop` file
//! this picker fills *both* the slice's command field AND its
//! icon in a single click. The flow users actually want is "make
//! a slice that launches Firefox" — typing the command, then
//! switching to the icon picker, then setting the colour mode
//! is three steps. This collapses it to one.
//!
//! Reuses the State.installed_apps cache populated at startup by
//! `oxidemx_shared::enumerate_applications()` (system + user +
//! flatpak XDG application dirs).

use crate::radial_preview::peek_icon_handle_untinted;
use crate::Message;
use iced::widget::{
    button, column, container, image, row, scrollable, text, text_input, toggler, Space,
};
use iced::{Alignment, Element, Length};
use oxidemx_widgets::style;

/// In-flight state of the app picker. `target` carries which
/// slice / sub-item the picked app's command + icon will land
/// on; `search` is a case-insensitive substring filter against
/// the app's display name; `replace_icon` controls whether the
/// pick also overwrites the slice's existing icon (default true
/// — pick a new app, get its icon too — but flips off when the
/// user wants to keep an icon they've already picked).
#[derive(Debug, Clone)]
pub struct AppCommandPickerState {
    pub target: AppCommandTarget,
    pub search: String,
    pub replace_icon: bool,
}

/// Where the app pick lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCommandTarget {
    Slice(usize),
    SubItem { parent: usize, idx: usize },
}

/// Thumbnail size — kept small since apps render as a list with
/// icon + name + exec preview, not the dense grid the icon
/// picker uses.
pub const THUMB_PX: u32 = 28;

/// Render the picker panel. Caller embeds the returned element
/// inside the slice editor so the user keeps editing context.
pub fn view<'a>(
    state: &'a crate::State,
    picker: &'a AppCommandPickerState,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let lc = picker.search.to_lowercase();
    let matched: Vec<&oxidemx_shared::DesktopEntry> = state
        .installed_apps
        .iter()
        .filter(|a| picker.search.is_empty() || a.name.to_lowercase().contains(&lc))
        .collect();

    let header = row![
        text("Pick app for command").size(13),
        Space::new().width(Length::Fill),
        text(format!(
            "{} match{}",
            matched.len(),
            if matched.len() == 1 { "" } else { "es" }
        ))
        .size(10)
        .style(style::text_faint(pal)),
        Space::new().width(Length::Fixed(8.0)),
        button(text("Close").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::CloseAppCommandPicker),
    ]
    .align_y(Alignment::Center);

    let intro = text(
        "Click an app to fill the slice's command, label (if empty), \
         and (when 'Also replace icon' is on) icon + Full colour mode. \
         Slice kind is set to Exec.",
    )
    .size(10)
    .style(style::text_dim(pal));

    // 'Also replace icon' toggle. Default on — most "pick an app"
    // flows want both command and icon. Toggle off when the user
    // already picked an icon they like and only wants the
    // command swapped out.
    let replace_toggle = row![
        toggler(picker.replace_icon)
            .on_toggle(Message::SetAppCommandReplaceIcon)
            .style(style::toggler_style(pal)),
        Space::new().width(Length::Fixed(8.0)),
        text("Also replace icon (and switch to Full colour)").size(11),
    ]
    .align_y(Alignment::Center);

    let search_input = text_input("Filter apps", &picker.search)
        .on_input(Message::SetAppCommandSearch)
        .padding(6)
        .size(12);

    let body: Element<Message> = if matched.is_empty() {
        text(if state.installed_apps.is_empty() {
            "No installed apps detected. (Empty $XDG_DATA_DIRS, or no .desktop files installed.)"
        } else {
            "No apps match that filter."
        })
        .size(11)
        .style(style::text_dim(pal))
        .into()
    } else {
        let mut col = column![].spacing(4);
        for app in matched.iter() {
            col = col.push(app_row(state, app));
        }
        scrollable(col).height(Length::Fill).into()
    };

    container(column![header, intro, replace_toggle, search_input, body].spacing(8))
        .padding(10)
        .style(style::card_quiet(pal))
        .into()
}

/// One app row: icon + name + exec preview, click anywhere → pick.
fn app_row<'a>(
    state: &'a crate::State,
    entry: &'a oxidemx_shared::DesktopEntry,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let pick_msg = Message::PickAppForCommand {
        command: oxidemx_shared::clean_exec_line(&entry.exec),
        icon: entry.icon.clone(),
        label: entry.name.clone(),
    };

    let handle = peek_icon_handle_untinted(&state.iced_handles, &entry.icon, THUMB_PX);
    let icon_el: Element<Message> = if let Some(h) = handle {
        image(h)
            .width(Length::Fixed(THUMB_PX as f32))
            .height(Length::Fixed(THUMB_PX as f32))
            .into()
    } else {
        Space::new()
            .width(Length::Fixed(THUMB_PX as f32))
            .height(Length::Fixed(THUMB_PX as f32))
            .into()
    };

    // Show a trimmed exec preview so the user can verify they're
    // picking the right launcher (matters for flatpak vs native
    // duplicates).
    let exec_preview = if entry.exec.is_empty() {
        "(no exec)".to_string()
    } else {
        let cleaned = oxidemx_shared::clean_exec_line(&entry.exec);
        if cleaned.chars().count() > 60 {
            let mut s: String = cleaned.chars().take(57).collect();
            s.push('…');
            s
        } else {
            cleaned
        }
    };
    let flatpak_badge: Element<Message> = if entry.is_flatpak {
        container(text("flatpak").size(9))
            .padding([2, 6])
            .style(style::chip(pal))
            .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    button(
        row![
            icon_el,
            column![
                row![
                    text(entry.name.clone()).size(12),
                    Space::new().width(Length::Fixed(8.0)),
                    flatpak_badge,
                ]
                .align_y(Alignment::Center),
                text(exec_preview).size(9).style(style::text_faint(pal)),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
        ]
        .align_y(Alignment::Center)
        .spacing(8),
    )
    .padding([6, 8])
    .style(style::btn_secondary(pal))
    .on_press(pick_msg)
    .width(Length::Fill)
    .into()
}
