//! Visual icon picker — popover panel with a search filter and a
//! grid of XDG symbolic icon thumbnails. Click an icon to apply
//! it to the slice or sub-item the picker was opened from.
//!
//! Why this exists: typing freedesktop icon names by memory is
//! brutal ("system-run-symbolic"? "edit-paste"?). The picker
//! ships a curated catalogue of common ones; for anything outside
//! the catalogue the user can still type by hand into the icon
//! text input — the picker is additive, not exclusive.
//!
//! Rendering: reuses the shared `IconCache` + `iced_handles` cache
//! that the radial preview uses, so opening the picker doesn't
//! re-rasterise icons the preview already rendered (and vice
//! versa). Filter pass is a case-insensitive substring match on
//! the icon name.

use crate::radial_preview::peek_icon_handle;
use crate::{style, Message};
use iced::widget::{button, column, container, image, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length};

/// In-flight icon-picker state: which slice/sub-item the picked
/// icon will be applied to + the current search filter text.
#[derive(Debug, Clone)]
pub struct IconPickerState {
    pub target: IconPickerTarget,
    pub search: String,
}

/// Where the picked icon lands. Slice edits the active page's
/// top-level slot at `usize`; SubItem edits a child of the
/// `parent` slot at `idx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconPickerTarget {
    Slice(usize),
    SubItem { parent: usize, idx: usize },
}

/// Thumbnail render size — kept in sync with the cell so the
/// prewarmer rasterises at the same dimensions the cells query.
pub const THUMB_PX: u32 = 28;

/// Curated XDG symbolic icon names organised by rough category.
/// Sticking to `-symbolic` flavour because they're alpha masks
/// (the radial overlay re-tints to the slice colour). Order is
/// roughly "things users reach for first" — list grows over time.
pub const COMMON_ICONS: &[&str] = &[
    // Actions — clipboard / undo / find
    "edit-copy-symbolic",
    "edit-cut-symbolic",
    "edit-paste-symbolic",
    "edit-undo-symbolic",
    "edit-redo-symbolic",
    "edit-find-symbolic",
    "edit-find-replace-symbolic",
    "edit-clear-symbolic",
    "edit-delete-symbolic",
    "edit-select-all-symbolic",
    // Document
    "document-new-symbolic",
    "document-open-symbolic",
    "document-open-recent-symbolic",
    "document-save-symbolic",
    "document-save-as-symbolic",
    "document-revert-symbolic",
    "document-print-symbolic",
    "document-print-preview-symbolic",
    "document-send-symbolic",
    "document-page-setup-symbolic",
    "document-properties-symbolic",
    "document-edit-symbolic",
    // View / navigation
    "view-refresh-symbolic",
    "view-fullscreen-symbolic",
    "view-restore-symbolic",
    "view-list-symbolic",
    "view-grid-symbolic",
    "view-paged-symbolic",
    "view-continuous-symbolic",
    "view-dual-symbolic",
    "view-more-symbolic",
    "view-pin-symbolic",
    "view-conceal-symbolic",
    "view-reveal-symbolic",
    "go-home-symbolic",
    "go-up-symbolic",
    "go-down-symbolic",
    "go-previous-symbolic",
    "go-next-symbolic",
    "go-jump-symbolic",
    "go-first-symbolic",
    "go-last-symbolic",
    "list-add-symbolic",
    "list-remove-symbolic",
    "object-flip-horizontal-symbolic",
    "object-flip-vertical-symbolic",
    "object-rotate-left-symbolic",
    "object-rotate-right-symbolic",
    // System
    "preferences-system-symbolic",
    "preferences-other-symbolic",
    "system-run-symbolic",
    "system-search-symbolic",
    "system-shutdown-symbolic",
    "system-reboot-symbolic",
    "system-suspend-symbolic",
    "system-hibernate-symbolic",
    "system-log-out-symbolic",
    "system-lock-screen-symbolic",
    "system-switch-user-symbolic",
    "system-help-symbolic",
    "open-menu-symbolic",
    // Apps
    "utilities-terminal-symbolic",
    "system-file-manager-symbolic",
    "web-browser-symbolic",
    "internet-mail-symbolic",
    "internet-chat-symbolic",
    "multimedia-player-symbolic",
    "applications-development-symbolic",
    "applications-graphics-symbolic",
    "applications-office-symbolic",
    "applications-multimedia-symbolic",
    "applications-internet-symbolic",
    "applications-system-symbolic",
    "applications-utilities-symbolic",
    "applications-games-symbolic",
    "applications-science-symbolic",
    "applications-engineering-symbolic",
    "applications-education-symbolic",
    "accessories-calculator-symbolic",
    "accessories-text-editor-symbolic",
    "accessories-character-map-symbolic",
    "accessories-dictionary-symbolic",
    "x-office-calendar-symbolic",
    "x-office-document-symbolic",
    "x-office-presentation-symbolic",
    "x-office-spreadsheet-symbolic",
    "x-office-address-book-symbolic",
    // Devices
    "computer-symbolic",
    "drive-harddisk-symbolic",
    "drive-multidisk-symbolic",
    "drive-removable-media-symbolic",
    "drive-removable-media-usb-symbolic",
    "drive-optical-symbolic",
    "audio-card-symbolic",
    "audio-headphones-symbolic",
    "audio-headset-symbolic",
    "audio-input-microphone-symbolic",
    "audio-speakers-symbolic",
    "camera-photo-symbolic",
    "camera-web-symbolic",
    "camera-video-symbolic",
    "input-mouse-symbolic",
    "input-keyboard-symbolic",
    "input-tablet-symbolic",
    "input-touchpad-symbolic",
    "input-gaming-symbolic",
    "input-dialpad-symbolic",
    "battery-symbolic",
    "battery-good-symbolic",
    "battery-low-symbolic",
    "battery-caution-symbolic",
    "battery-empty-charging-symbolic",
    "display-symbolic",
    "display-with-window-symbolic",
    "phone-symbolic",
    "phone-apple-iphone-symbolic",
    "printer-symbolic",
    "scanner-symbolic",
    "media-removable-symbolic",
    "media-tape-symbolic",
    // Places
    "folder-symbolic",
    "folder-new-symbolic",
    "folder-open-symbolic",
    "folder-saved-search-symbolic",
    "folder-download-symbolic",
    "folder-documents-symbolic",
    "folder-music-symbolic",
    "folder-pictures-symbolic",
    "folder-videos-symbolic",
    "folder-publicshare-symbolic",
    "folder-templates-symbolic",
    "folder-remote-symbolic",
    "folder-publish-symbolic",
    "folder-visiting-symbolic",
    "user-home-symbolic",
    "user-trash-symbolic",
    "user-trash-full-symbolic",
    "user-desktop-symbolic",
    "user-bookmarks-symbolic",
    "user-info-symbolic",
    "user-status-pending-symbolic",
    "user-available-symbolic",
    "user-busy-symbolic",
    "user-away-symbolic",
    "user-invisible-symbolic",
    "user-offline-symbolic",
    // Status / dialogs
    "dialog-information-symbolic",
    "dialog-warning-symbolic",
    "dialog-error-symbolic",
    "dialog-question-symbolic",
    "dialog-password-symbolic",
    "network-wireless-symbolic",
    "network-wired-symbolic",
    "network-cellular-signal-good-symbolic",
    "network-cellular-signal-excellent-symbolic",
    "network-vpn-symbolic",
    "network-vpn-acquiring-symbolic",
    "network-offline-symbolic",
    "network-transmit-receive-symbolic",
    "network-error-symbolic",
    "network-idle-symbolic",
    "bluetooth-symbolic",
    "bluetooth-active-symbolic",
    "bluetooth-disabled-symbolic",
    "audio-volume-high-symbolic",
    "audio-volume-medium-symbolic",
    "audio-volume-low-symbolic",
    "audio-volume-muted-symbolic",
    "microphone-sensitivity-high-symbolic",
    "microphone-sensitivity-medium-symbolic",
    "microphone-sensitivity-muted-symbolic",
    "weather-clear-symbolic",
    "weather-clear-night-symbolic",
    "weather-few-clouds-symbolic",
    "weather-overcast-symbolic",
    "weather-showers-symbolic",
    "weather-snow-symbolic",
    "weather-storm-symbolic",
    // Media playback
    "media-playback-start-symbolic",
    "media-playback-pause-symbolic",
    "media-playback-stop-symbolic",
    "media-skip-forward-symbolic",
    "media-skip-backward-symbolic",
    "media-seek-forward-symbolic",
    "media-seek-backward-symbolic",
    "media-record-symbolic",
    "media-eject-symbolic",
    "media-playlist-repeat-symbolic",
    "media-playlist-shuffle-symbolic",
    "media-playlist-consecutive-symbolic",
    "audio-x-generic-symbolic",
    "video-x-generic-symbolic",
    "image-x-generic-symbolic",
    "package-x-generic-symbolic",
    "text-x-generic-symbolic",
    "application-x-executable-symbolic",
    "font-x-generic-symbolic",
    "video-display-symbolic",
    "video-single-display-symbolic",
    // Window / layout
    "window-close-symbolic",
    "window-minimize-symbolic",
    "window-maximize-symbolic",
    "window-restore-symbolic",
    "window-new-symbolic",
    "tab-new-symbolic",
    "tab-symbolic",
    "send-to-symbolic",
    "object-select-symbolic",
    "open-menu-symbolic",
    "pan-down-symbolic",
    "pan-up-symbolic",
    "pan-start-symbolic",
    "pan-end-symbolic",
    "zoom-in-symbolic",
    "zoom-out-symbolic",
    "zoom-original-symbolic",
    "zoom-fit-best-symbolic",
    // Misc favourites
    "starred-symbolic",
    "non-starred-symbolic",
    "semi-starred-symbolic",
    "emblem-favorite-symbolic",
    "emblem-default-symbolic",
    "emblem-important-symbolic",
    "emblem-system-symbolic",
    "emblem-shared-symbolic",
    "emblem-readonly-symbolic",
    "emblem-synchronizing-symbolic",
    "emblem-ok-symbolic",
    "emoji-symbolic",
    "color-select-symbolic",
    "preferences-desktop-keyboard-symbolic",
    "preferences-desktop-display-symbolic",
    "preferences-desktop-theme-symbolic",
    "preferences-desktop-personal-symbolic",
    "help-about-symbolic",
    "help-faq-symbolic",
    "help-browser-symbolic",
    "find-location-symbolic",
    "mark-location-symbolic",
    "appointment-soon-symbolic",
    "appointment-missed-symbolic",
    "task-due-symbolic",
    "task-past-due-symbolic",
    "checkbox-checked-symbolic",
    "checkbox-symbolic",
    "radio-checked-symbolic",
    "radio-symbolic",
    // Power / energy
    "ac-adapter-symbolic",
    "battery-100-symbolic",
    "battery-080-symbolic",
    "battery-060-symbolic",
    "battery-040-symbolic",
    "battery-020-symbolic",
    "battery-000-symbolic",
    "battery-charged-symbolic",
];

/// Render the picker panel. Caller embeds the returned element
/// inside the slice editor card; the picker doesn't try to be a
/// modal overlay (iced 0.14 has no modal primitive — Stack +
/// padding gets ugly fast on narrow columns).
pub fn view<'a>(state: &'a crate::State, picker: &'a IconPickerState) -> Element<'a, Message> {
    let pal = &state.palette;
    let lc = picker.search.to_lowercase();
    let matched: Vec<&'static &'static str> = COMMON_ICONS
        .iter()
        .filter(|n| picker.search.is_empty() || n.to_lowercase().contains(&lc))
        .collect();

    let header = row![
        text("Pick an icon").size(13),
        Space::new().width(Length::Fill),
        text(format!("{} match{}", matched.len(), if matched.len() == 1 { "" } else { "es" }))
            .size(10)
            .style(style::text_faint(pal)),
        Space::new().width(Length::Fixed(8.0)),
        button(text("Close").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::CloseIconPicker),
    ]
    .align_y(Alignment::Center);

    let search_input = text_input("Filter icons (e.g. \"edit\", \"folder\", \"play\")", &picker.search)
        .on_input(Message::SetIconPickerSearch)
        .padding(6)
        .size(12);

    // Recently-used row — only rendered when the search bar is
    // empty (otherwise the search filter is what the user is
    // narrowing by). Surfaces frequently-reached icons at the top
    // so the user doesn't have to filter or scroll for them.
    let recents_row: Option<Element<Message>> = if picker.search.is_empty()
        && !state.recent_icons.is_empty()
    {
        const COLS: usize = 6;
        let mut grid = column![].spacing(6);
        let mut current = row![].spacing(6);
        let mut count = 0usize;
        for name in state.recent_icons.iter().take(COLS * 2) {
            current = current.push(icon_cell(state, name));
            count += 1;
            if count == COLS {
                grid = grid.push(current);
                current = row![].spacing(6);
                count = 0;
            }
        }
        if count > 0 {
            for _ in count..COLS {
                current = current.push(Space::new().width(Length::FillPortion(1)));
            }
            grid = grid.push(current);
        }
        let label = row![
            text("Recently used").size(11).style(style::text_dim(pal)),
            Space::new().width(Length::Fill),
            text(format!("{} icon{}", state.recent_icons.len(), if state.recent_icons.len() == 1 { "" } else { "s" }))
                .size(9)
                .style(style::text_faint(pal)),
        ]
        .align_y(Alignment::Center);
        Some(column![label, grid].spacing(4).into())
    } else {
        None
    };

    let body: Element<Message> = if matched.is_empty() {
        text("No icons match that filter. Try something shorter, or type the name directly into the icon field above.")
            .size(11)
            .style(style::text_dim(pal))
            .into()
    } else {
        const COLS: usize = 6;
        let mut grid = column![].spacing(6);
        let mut current = row![].spacing(6);
        let mut count = 0usize;
        for name in &matched {
            current = current.push(icon_cell(state, name));
            count += 1;
            if count == COLS {
                grid = grid.push(current);
                current = row![].spacing(6);
                count = 0;
            }
        }
        if count > 0 {
            for _ in count..COLS {
                current = current.push(Space::new().width(Length::FillPortion(1)));
            }
            grid = grid.push(current);
        }
        scrollable(grid).height(Length::Fixed(280.0)).into()
    };

    let mut col = column![header, search_input].spacing(8);
    if let Some(recents) = recents_row {
        col = col.push(recents);
    }
    col = col.push(text("All icons").size(11).style(style::text_dim(pal)));
    col = col.push(body);
    container(col).padding(10).style(style::card_quiet(pal)).into()
}

fn icon_cell<'a>(state: &'a crate::State, name: &'a str) -> Element<'a, Message> {
    let pal = &state.palette;
    let owned = name.to_string();
    let pick_msg = Message::PickIcon(owned);

    // Render thumbnail tinted with the theme text colour. The
    // shared cache means the same thumbnail rendered in the picker
    // is reused on the radial preview if/when this icon lands on
    // a slice.
    // Cache-only lookup. The first time the picker opens, the
    // cache is empty — every cell renders a placeholder. The
    // PrewarmIcons handler trickles rasterisations through 4 at a
    // time and triggers redraws so cells fill in progressively.
    let handle = peek_icon_handle(&state.iced_handles, name, THUMB_PX, pal.text);

    let label = short_label(name);
    let body: Element<Message> = if let Some(h) = handle {
        column![
            image(h)
                .width(Length::Fixed(THUMB_PX as f32))
                .height(Length::Fixed(THUMB_PX as f32)),
            text(label).size(8).style(style::text_faint(pal)),
        ]
        .spacing(2)
        .align_x(Alignment::Center)
        .into()
    } else {
        // Cache miss — placeholder + name. Either the prewarmer
        // hasn't reached this icon yet (transient) OR the icon
        // doesn't exist in the active theme (permanent miss).
        // Either way the cell is still clickable; if the user
        // selects an icon that ultimately can't be resolved, the
        // overlay falls back to a tinted dot at render time.
        let dot = Space::new()
            .width(Length::Fixed(THUMB_PX as f32))
            .height(Length::Fixed(THUMB_PX as f32));
        column![
            dot,
            text(label).size(8).style(style::text_faint(pal)),
        ]
        .spacing(2)
        .align_x(Alignment::Center)
        .into()
    };

    button(body)
        .padding(4)
        .style(style::btn_secondary(pal))
        .on_press(pick_msg)
        .width(Length::FillPortion(1))
        .into()
}

/// Strip the `-symbolic` suffix for the cell label so the visible
/// text fits in the small grid cell.
fn short_label(full: &str) -> &str {
    full.strip_suffix("-symbolic").unwrap_or(full)
}
