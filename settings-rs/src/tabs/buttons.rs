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
use juhradial_shared::{ActionKind, Condition, RadialPage, Slice};
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
    // Left column carries the mouse photo + per-button assignment
    // list. Both fit comfortably in a narrow column, so we cap the
    // portion to keep the radial menu editor (right column, where
    // most editing actually happens) wide enough to be usable.
    .width(Length::FillPortion(2))
    .max_width(560.0)
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
            page_picker_card(state),
            radial_preview_card(state),
            easy_switch_panel(state),
            selected_slice_editor(state),
        ]
        .spacing(16),
    )
    // Right column owns the radial-menu editor — slice editor +
    // page picker + icon picker. These widgets need real estate
    // to be usable; bumping the portion to 3 (vs. left's 2)
    // gives the editor side ~60% of the row at any window size.
    .width(Length::FillPortion(3))
    .into()
}

/// Multi-page menu: a picker for the active page + per-page
/// properties (name, app classes, include-in-scroll). The slice
/// editor + radial preview both pin to the active page.
fn page_picker_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let pages = &state.config.radial_menu.pages;

    // Defensive — should never happen post-`normalize_pages`, but
    // an empty pages list would otherwise show an empty picker.
    if pages.is_empty() {
        return container(text("(no pages — internal error)").size(11).style(style::text_dim(pal)))
            .padding(14)
            .style(style::card(pal))
            .into();
    }

    let options: Vec<PageChoice> = pages
        .iter()
        .enumerate()
        .map(|(i, p)| PageChoice {
            idx: i,
            display: format_page_label(i, p),
        })
        .collect();
    let active = state.active_page.min(pages.len() - 1);
    let selected = options.get(active).cloned();
    let picker = pick_list(options, selected, |c: PageChoice| Message::SetActivePage(c.idx))
        .style(style::pick_list_style(pal))
        .text_size(13);

    let intro = text(
        "Multi-page menu: scroll the wheel over the centre puck to \
         cycle pages. App-context pages auto-activate when their \
         class focuses; mark them as scrollable too if you also want \
         them in the wheel cycle.",
    )
    .size(11)
    .style(style::text_dim(pal));

    let header = row![
        text("Pages").size(14),
        Space::new().width(Length::Fill),
        button(text("+ Add page").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::AddPage),
    ]
    .align_y(Alignment::Center);

    let picker_row = row![
        text("Active").size(13).width(Length::Fixed(60.0)),
        picker,
        Space::new().width(Length::Fill),
        page_move_buttons(state, active),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    let editor = page_props_editor(state, active);

    container(
        column![
            header,
            rule::horizontal(1).style(style::rule_style(pal)),
            intro,
            picker_row,
            editor,
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

fn page_move_buttons(state: &State, active: usize) -> Element<'_, Message> {
    let pal = &state.palette;
    let last = state.config.radial_menu.pages.len().saturating_sub(1);
    let mut left_btn = button(text("◀").size(11)).style(style::btn_secondary(pal));
    if active > 0 {
        left_btn = left_btn.on_press(Message::MovePageLeft(active));
    }
    let mut right_btn = button(text("▶").size(11)).style(style::btn_secondary(pal));
    if active < last {
        right_btn = right_btn.on_press(Message::MovePageRight(active));
    }
    let mut delete_btn = button(text("Delete").size(11)).style(style::btn_danger(pal));
    if state.config.radial_menu.pages.len() > 1 {
        delete_btn = delete_btn.on_press(Message::DeletePage(active));
    }
    row![left_btn, right_btn, delete_btn].spacing(6).into()
}

fn page_props_editor(state: &State, active: usize) -> Element<'_, Message> {
    let pal = &state.palette;
    let page = match state.config.radial_menu.pages.get(active) {
        Some(p) => p,
        None => return Space::new().height(Length::Fixed(0.0)).into(),
    };

    let active_for_name = active;
    let name_input = text_input("Page name", page.name.as_str())
        .on_input(move |v| Message::SetPageName { page: active_for_name, name: v })
        .padding(6)
        .size(12);

    let active_for_classes = active;
    let csv = state
        .app_classes_drafts
        .get(&active)
        .cloned()
        .unwrap_or_else(|| page.app_classes.join(", "));
    let app_classes_input = text_input(
        "Window classes (comma-separated, e.g. firefox, chromium)",
        &csv,
    )
    .on_input(move |v| Message::SetPageAppClasses {
        page: active_for_classes,
        value: v,
    })
    .padding(6)
    .size(12);

    // "Detect from focused window" calls the GNOME extension's
    // GetFocusedWindowClass after a 4-second delay, giving the
    // user time to switch to the target app before the sample
    // fires. Repeated detects append (deduped), so multiple
    // window classes can be captured without losing the existing
    // list. While a detect is in flight, the button swaps to a
    // countdown + Cancel.
    let active_for_detect = active;
    let detect_in_flight = state
        .detect_in_flight
        .filter(|d| d.page == active_for_detect);
    let classes_row: Element<Message> = if detect_in_flight.is_some() {
        // Iced re-renders on messages, not on a timer, so a live
        // ticking countdown would need its own subscription. The
        // sample fires automatically after 4 s so a static label
        // is enough — the status bar already says "Sampling in 4s".
        let waiting_btn = button(text("Switch to target app… (4s)").size(11))
            .style(style::btn_secondary(pal));
        let cancel_btn = button(text("Cancel").size(11))
            .style(style::btn_danger(pal))
            .on_press(Message::CancelFocusedClassDetect);
        row![app_classes_input, waiting_btn, cancel_btn]
            .align_y(Alignment::Center)
            .spacing(8)
            .into()
    } else {
        let detect_button = button(text("Detect from focused window").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::DetectFocusedClass(active_for_detect));
        row![app_classes_input, detect_button]
            .align_y(Alignment::Center)
            .spacing(8)
            .into()
    };

    let context_hint = if page.app_classes.is_empty() {
        text("Global page — always reachable via the scroll cycle.")
            .size(11)
            .style(style::text_faint(pal))
    } else {
        text(format!(
            "App-context page — auto-selects when one of these classes is focused ({} class{}).",
            page.app_classes.len(),
            if page.app_classes.len() == 1 { "" } else { "es" }
        ))
        .size(11)
        .style(style::text_faint(pal))
    };

    // Include-in-scroll toggle is only meaningful for app-context
    // pages — global pages are always in the cycle.
    let scroll_row: Element<Message> = if page.app_classes.is_empty() {
        Space::new().height(Length::Fixed(0.0)).into()
    } else {
        let active_for_scroll = active;
        row![
            column![
                text("Also include in scroll cycle").size(13),
                text(
                    "When off, this page is only reachable through app focus \
                     — the scroll wheel skips over it."
                )
                .size(11)
                .style(style::text_dim(pal)),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            toggler(page.include_in_scroll)
                .on_toggle(move |v| Message::SetPageIncludeInScroll {
                    page: active_for_scroll,
                    value: v,
                })
                .style(style::toggler_style(pal)),
        ]
        .align_y(Alignment::Center)
        .spacing(12)
        .into()
    };

    column![name_input, classes_row, context_hint, scroll_row].spacing(8).into()
}

fn format_page_label(idx: usize, page: &RadialPage) -> String {
    let n = page.name.trim();
    let display_name = if n.is_empty() {
        format!("Page {}", idx + 1)
    } else {
        n.to_string()
    };
    if page.app_classes.is_empty() {
        format!("{}: {}", idx + 1, display_name)
    } else {
        // Show up to two classes inline so the picker entry stays
        // readable; longer lists collapse to "…".
        let preview: Vec<&str> = page.app_classes.iter().take(2).map(String::as_str).collect();
        let suffix = if page.app_classes.len() > preview.len() {
            format!("{}, …", preview.join(", "))
        } else {
            preview.join(", ")
        };
        format!("{}: {} ({})", idx + 1, display_name, suffix)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PageChoice {
    idx: usize,
    display: String,
}

impl std::fmt::Display for PageChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
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
        state.active_slices(),
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

    let active = state.active_slices();
    let body: Element<Message> = match state.selected_slice {
        Some(idx) if idx < active.len() => {
            let last = active.len().saturating_sub(1);
            slice_editor_row(state, idx, &active[idx], last)
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

    // Icon name + a "Browse" button that opens the visual picker.
    // Text input still works for icons outside the curated
    // catalogue (paste any freedesktop name or absolute path).
    let icon_input = text_input("Icon (e.g. \"system-run-symbolic\" or /path/to/icon.svg)", slice.icon.as_str())
        .on_input(move |s| Message::SetSliceIcon(idx, s))
        .padding(6)
        .size(12)
        .width(Length::Fill);
    let icon_browse_target = crate::icon_picker::IconPickerTarget::Slice(idx);
    let icon_browse = button(text("Browse…").size(11))
        .style(style::btn_secondary(pal))
        .on_press(Message::OpenIconPicker(icon_browse_target));
    let icon_file_btn = button(text("From file…").size(11))
        .style(style::btn_secondary(pal))
        .on_press(Message::BrowseIconFile(icon_browse_target));
    let icon_row = row![icon_input, icon_browse, icon_file_btn]
        .align_y(Alignment::Center)
        .spacing(8);

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

    // Test button — only meaningful for Exec slices, and only
    // when there's actually a command to spawn. We always render
    // the button so users have a stable UI; the handler reports
    // a hint when the slice isn't testable.
    let test_btn = button(text("▶ Test").size(11))
        .style(style::btn_secondary(pal))
        .on_press(Message::TestSliceAction(idx));

    let header = row![
        text(format!("Slot {}", idx + 1))
            .size(11)
            .style(style::text_faint(pal)),
        Space::new().width(Length::Fill),
        test_btn,
        up_btn,
        down_btn,
        del_btn,
    ]
    .align_y(Alignment::Center)
    .spacing(6);

    let mut col = column![
        header,
        label_input,
        row![kind_picker, color_picker, cmd_input.width(Length::Fill)]
            .align_y(Alignment::Center)
            .spacing(8),
        icon_row,
        visibility_editor(state, idx, slice.visible_if.as_ref()),
    ]
    .spacing(6);

    // Inline icon picker — visible when the picker is open
    // against this specific slice. Stacking it inside the editor
    // card keeps the user's editing context (label, command, etc.)
    // anchored above the picker.
    if let Some(p) = state.icon_picker.as_ref() {
        if matches!(p.target, crate::icon_picker::IconPickerTarget::Slice(t) if t == idx) {
            col = col.push(crate::icon_picker::view(state, p));
        }
    }

    // Submenu editor — only when this slice's kind is Submenu.
    // Lists each sub-item with a label / command / colour picker
    // and reorder + delete buttons. "+ Add item" appends a new
    // Exec sub-item to the end.
    if slice.kind == ActionKind::Submenu {
        col = col.push(submenu_editor(state, idx, &slice.submenu));
    }

    container(col).padding(10).style(style::card_quiet(pal)).into()
}

// =============================================================================
// Visibility predicate editor (slice.visible_if)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisKind {
    Always,
    Never,
    Executable,
    FileExists,
    ProcessRunning,
    EnvSet,
    EnvEquals,
    /// Compound (All / Any / Not). Read-only in the inline editor —
    /// users still edit those by hand in the JSON config since the
    /// recursive structure doesn't fit a single-row UI.
    Compound,
}

impl VisKind {
    const ALL_SIMPLE: &'static [VisKind] = &[
        VisKind::Always,
        VisKind::Never,
        VisKind::Executable,
        VisKind::FileExists,
        VisKind::ProcessRunning,
        VisKind::EnvSet,
        VisKind::EnvEquals,
    ];

    fn label(self) -> &'static str {
        match self {
            VisKind::Always => "Always visible",
            VisKind::Never => "Never (hidden)",
            VisKind::Executable => "Executable on $PATH",
            VisKind::FileExists => "File exists",
            VisKind::ProcessRunning => "Process running",
            VisKind::EnvSet => "Env var set",
            VisKind::EnvEquals => "Env var equals",
            VisKind::Compound => "Compound (edit JSON)",
        }
    }

    fn from_condition(c: Option<&Condition>) -> Self {
        match c {
            None => VisKind::Always,
            Some(Condition::Always) => VisKind::Always,
            Some(Condition::Never) => VisKind::Never,
            Some(Condition::Executable { .. }) => VisKind::Executable,
            Some(Condition::FileExists { .. }) => VisKind::FileExists,
            Some(Condition::ProcessRunning { .. }) => VisKind::ProcessRunning,
            Some(Condition::EnvSet { .. }) => VisKind::EnvSet,
            Some(Condition::EnvEquals { .. }) => VisKind::EnvEquals,
            Some(Condition::All { .. })
            | Some(Condition::Any { .. })
            | Some(Condition::Not { .. }) => VisKind::Compound,
        }
    }
}

impl std::fmt::Display for VisKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Build a fresh `Condition` value when the user picks a new
/// variant. Single-arg variants start with empty strings; the
/// argument inputs below the picker drive subsequent edits.
fn condition_for_kind(kind: VisKind, prev: Option<&Condition>) -> Option<Condition> {
    // Carry over arg values where possible so switching variants
    // doesn't always wipe what the user typed (e.g. EnvSet → EnvEquals
    // keeps the var name).
    let prev_string = prev.and_then(|c| match c {
        Condition::Executable { name } => Some(name.clone()),
        Condition::FileExists { path } => Some(path.clone()),
        Condition::ProcessRunning { comm } => Some(comm.clone()),
        Condition::EnvSet { var } => Some(var.clone()),
        Condition::EnvEquals { var, .. } => Some(var.clone()),
        _ => None,
    });
    let prev_value = prev.and_then(|c| match c {
        Condition::EnvEquals { value, .. } => Some(value.clone()),
        _ => None,
    });
    match kind {
        VisKind::Always => None,
        VisKind::Never => Some(Condition::Never),
        VisKind::Executable => Some(Condition::Executable {
            name: prev_string.unwrap_or_default(),
        }),
        VisKind::FileExists => Some(Condition::FileExists {
            path: prev_string.unwrap_or_default(),
        }),
        VisKind::ProcessRunning => Some(Condition::ProcessRunning {
            comm: prev_string.unwrap_or_default(),
        }),
        VisKind::EnvSet => Some(Condition::EnvSet {
            var: prev_string.unwrap_or_default(),
        }),
        VisKind::EnvEquals => Some(Condition::EnvEquals {
            var: prev_string.unwrap_or_default(),
            value: prev_value.unwrap_or_default(),
        }),
        // Compound is read-only in the inline editor; selecting it
        // is a no-op (the picker swap is suppressed at the call
        // site). Returning the existing condition keeps the slice
        // unchanged.
        VisKind::Compound => prev.cloned(),
    }
}

/// Render the visibility predicate editor row for one slice.
/// Layout: small header → variant picker + (optional) one or two
/// text inputs for the variant's args.
fn visibility_editor<'a>(
    state: &'a State,
    slice_idx: usize,
    current: Option<&'a Condition>,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let kind = VisKind::from_condition(current);

    // Compound variant is grey-only — show it in the picker so the
    // user can see "yeah this is set, but I have to edit JSON" but
    // don't include it in the picker options unless it's already
    // the current state (avoids accidental selection).
    let mut options: Vec<VisKind> = VisKind::ALL_SIMPLE.to_vec();
    if kind == VisKind::Compound {
        options.push(VisKind::Compound);
    }

    let prev_clone = current.cloned();
    let kind_picker = pick_list(options, Some(kind), move |new_kind| {
        // No-op when the user clicks the current Compound entry.
        if new_kind == VisKind::Compound {
            return Message::SetSliceVisibility {
                slice: slice_idx,
                condition: prev_clone.clone(),
            };
        }
        Message::SetSliceVisibility {
            slice: slice_idx,
            condition: condition_for_kind(new_kind, prev_clone.as_ref()),
        }
    })
    .style(style::pick_list_style(pal))
    .text_size(11);

    let header = row![
        text("Visibility").size(11).style(style::text_dim(pal)),
        Space::new().width(Length::Fixed(10.0)),
        kind_picker,
    ]
    .align_y(Alignment::Center)
    .spacing(6);

    // Per-variant arg inputs. We always emit a *replacement*
    // Condition on every keystroke — no draft state needed.
    let args: Element<Message> = match current {
        Some(Condition::Executable { name }) => {
            let prev = current.cloned();
            let owned_name = name.clone();
            text_input(
                "Executable name (e.g. \"git\", \"spotify\") or absolute path",
                &owned_name,
            )
            .on_input(move |v| Message::SetSliceVisibility {
                slice: slice_idx,
                condition: Some(Condition::Executable { name: v }),
            })
            .padding(5)
            .size(11)
            .into()
        }
        Some(Condition::FileExists { path }) => {
            let owned_path = path.clone();
            text_input(
                "Path (~ and $VAR are expanded, e.g. \"~/.config/foo\")",
                &owned_path,
            )
            .on_input(move |v| Message::SetSliceVisibility {
                slice: slice_idx,
                condition: Some(Condition::FileExists { path: v }),
            })
            .padding(5)
            .size(11)
            .into()
        }
        Some(Condition::ProcessRunning { comm }) => {
            let owned_comm = comm.clone();
            text_input(
                "Process comm (the short name in /proc/PID/comm, e.g. \"spotify\")",
                &owned_comm,
            )
            .on_input(move |v| Message::SetSliceVisibility {
                slice: slice_idx,
                condition: Some(Condition::ProcessRunning { comm: v }),
            })
            .padding(5)
            .size(11)
            .into()
        }
        Some(Condition::EnvSet { var }) => {
            let owned_var = var.clone();
            text_input(
                "Environment variable name (e.g. \"WAYLAND_DISPLAY\")",
                &owned_var,
            )
            .on_input(move |v| Message::SetSliceVisibility {
                slice: slice_idx,
                condition: Some(Condition::EnvSet { var: v }),
            })
            .padding(5)
            .size(11)
            .into()
        }
        Some(Condition::EnvEquals { var, value }) => {
            let var_for_var = var.clone();
            let var_for_val = var.clone();
            let val_for_var = value.clone();
            let val_for_val = value.clone();
            row![
                text_input("Variable", &var_for_var)
                    .on_input(move |v| Message::SetSliceVisibility {
                        slice: slice_idx,
                        condition: Some(Condition::EnvEquals {
                            var: v,
                            value: val_for_var.clone(),
                        }),
                    })
                    .padding(5)
                    .size(11)
                    .width(Length::FillPortion(2)),
                text("=").size(11).style(style::text_faint(pal)),
                text_input("Value", &val_for_val)
                    .on_input(move |v| Message::SetSliceVisibility {
                        slice: slice_idx,
                        condition: Some(Condition::EnvEquals {
                            var: var_for_val.clone(),
                            value: v,
                        }),
                    })
                    .padding(5)
                    .size(11)
                    .width(Length::FillPortion(3)),
            ]
            .align_y(Alignment::Center)
            .spacing(6)
            .into()
        }
        Some(Condition::All { conditions }) | Some(Condition::Any { conditions }) => {
            text(format!(
                "Compound predicate ({} sub-condition{}) — edit ~/.config/juhradial/config.json directly to modify.",
                conditions.len(),
                if conditions.len() == 1 { "" } else { "s" }
            ))
            .size(10)
            .style(style::text_faint(pal))
            .into()
        }
        Some(Condition::Not { .. }) => text(
            "Compound NOT predicate — edit ~/.config/juhradial/config.json directly to modify.",
        )
        .size(10)
        .style(style::text_faint(pal))
        .into(),
        // Always / Never / None — no args needed.
        _ => Space::new().height(Length::Fixed(0.0)).into(),
    };

    column![header, args].spacing(4).into()
}

fn submenu_editor<'a>(
    state: &'a State,
    parent: usize,
    items: &'a [Slice],
) -> Element<'a, Message> {
    let pal = &state.palette;
    let header = row![
        text("Sub-items").size(11).style(style::text_dim(pal)),
        Space::new().width(Length::Fill),
        button(text("+ Add item").size(10))
            .style(style::btn_secondary(pal))
            .on_press(Message::AddSubItem(parent)),
    ]
    .align_y(Alignment::Center)
    .spacing(6);

    let mut col = column![header, rule::horizontal(1).style(style::rule_style(pal))]
        .spacing(6);
    if items.is_empty() {
        col = col.push(
            text("No sub-items yet. Click \"+ Add item\" to create one.")
                .size(11)
                .style(style::text_faint(pal)),
        );
    } else {
        let last = items.len() - 1;
        for (i, item) in items.iter().enumerate() {
            col = col.push(submenu_item_row(state, parent, i, item, last));
        }
    }
    container(col)
        .padding(8)
        .style(style::card(pal))
        .into()
}

fn submenu_item_row<'a>(
    state: &'a State,
    parent: usize,
    idx: usize,
    item: &'a Slice,
    last_idx: usize,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let label_input = text_input("Label", item.label.as_str())
        .on_input(move |s| Message::SetSubItemLabel(parent, idx, s))
        .padding(5)
        .size(11)
        .width(Length::FillPortion(2));
    let cmd_input = text_input("Command", item.command.as_str())
        .on_input(move |s| Message::SetSubItemCommand(parent, idx, s))
        .padding(5)
        .size(11)
        .width(Length::FillPortion(3));
    let color_picker = pick_list(
        color_options(),
        Some(ColorOption(if item.color.is_empty() {
            "accent".to_string()
        } else {
            item.color.clone()
        })),
        move |opt| Message::SetSubItemColor(parent, idx, opt.0),
    )
    .style(style::pick_list_style(pal))
    .text_size(11);

    let kind_picker = pick_list(
        KIND_OPTIONS.as_slice(),
        Some(KindOption::from(item.kind)),
        move |opt| Message::SetSubItemKind(parent, idx, opt.into()),
    )
    .style(style::pick_list_style(pal))
    .text_size(11);

    let mut up_btn = button(text("↑").size(10)).style(style::btn_secondary(pal));
    if idx > 0 {
        up_btn = up_btn.on_press(Message::MoveSubItemUp(parent, idx));
    }
    let mut down_btn = button(text("↓").size(10)).style(style::btn_secondary(pal));
    if idx < last_idx {
        down_btn = down_btn.on_press(Message::MoveSubItemDown(parent, idx));
    }
    let test_btn = button(text("▶").size(10))
        .style(style::btn_secondary(pal))
        .on_press(Message::TestSubItemAction { parent, idx });
    let del_btn = button(text("✕").size(10))
        .style(style::btn_danger(pal))
        .on_press(Message::DeleteSubItem(parent, idx));

    // Icon + kind on a second row underneath — the main row is
    // already dense (label + color + cmd + 3 buttons); piling on
    // more inputs there pushes everything off-screen.
    let icon_input = text_input("Icon (freedesktop name or path)", item.icon.as_str())
        .on_input(move |s| Message::SetSubItemIcon(parent, idx, s))
        .padding(5)
        .size(11);
    let sub_browse_target = crate::icon_picker::IconPickerTarget::SubItem { parent, idx };
    let sub_browse_btn = button(text("Browse…").size(10))
        .style(style::btn_secondary(pal))
        .on_press(Message::OpenIconPicker(sub_browse_target));
    let sub_file_btn = button(text("File…").size(10))
        .style(style::btn_secondary(pal))
        .on_press(Message::BrowseIconFile(sub_browse_target));

    let mut col = column![
        row![
            text(format!("{}.", idx + 1))
                .size(10)
                .style(style::text_faint(pal)),
            label_input,
            color_picker,
            cmd_input,
            test_btn,
            up_btn,
            down_btn,
            del_btn,
        ]
        .align_y(Alignment::Center)
        .spacing(6),
        row![kind_picker, icon_input.width(Length::Fill), sub_browse_btn, sub_file_btn]
            .align_y(Alignment::Center)
            .spacing(6),
    ]
    .spacing(4);

    if let Some(p) = state.icon_picker.as_ref() {
        if matches!(
            p.target,
            crate::icon_picker::IconPickerTarget::SubItem { parent: pp, idx: ii }
                if pp == parent && ii == idx
        ) {
            col = col.push(crate::icon_picker::view(state, p));
        }
    }

    col.into()
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
