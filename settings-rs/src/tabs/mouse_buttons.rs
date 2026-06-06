//! "Mouse buttons" tab — per-button action assignments for the
//! MX Master 4. Extracted from the legacy "Buttons" tab so the
//! mouse-side configuration sits in its own dedicated panel,
//! and the radial-menu slice editor (which used to live to the
//! right of this panel) gets the full window width on its own
//! tab.
//!
//! This tab owns:
//!   * the device photo with overlaid callouts
//!   * the per-button assignment list (target → action picker)
//!
//! All other configuration that USED to be on this tab (page
//! picker, radial preview, slice editor, easy-switch toggle) now
//! lives on `Tab::Menu`.

use crate::mouse_callouts::mouse_widget;
use oxidemx_widgets::widgets::section_header;
use crate::{style, Message, State};
use iced::widget::{column, container, pick_list, row, text, Space};
use iced::{Alignment, Element, Length};
use std::path::PathBuf;

const MOUSE_IMAGE_PATH: &str = "assets/devices/logitechmouse.png";

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    let mut button_col = column![].spacing(4);
    for mb in oxidemx_shared::MouseButton::all() {
        button_col = button_col.push(button_assignment_row(state, *mb));
    }

    container(
        column![
            section_header("MX Master 4"),
            text("Click any action below to remap that button.")
                .size(12)
                .style(style::text_dim(pal)),
            container(mouse_photo(state))
                .padding(8)
                .center_x(Length::Fill),
            container(button_col)
                .padding(12)
                .style(style::card_quiet(pal))
                .width(Length::Fill),
        ]
        .spacing(14),
    )
    // Take full panel width to match the Menu tab's layout.
    .width(Length::Fill)
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

fn button_assignment_row(
    state: &State,
    mb: oxidemx_shared::MouseButton,
) -> Element<'_, Message> {
    let pal = &state.palette;
    let current = mb.get(&state.config.buttons);

    let options: Vec<ActionOption> = oxidemx_shared::ButtonAction::all()
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
struct ActionOption(oxidemx_shared::ButtonAction);

impl std::fmt::Display for ActionOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.label())
    }
}
