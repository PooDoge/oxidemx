//! "Menu" tab — radial menu configuration:
//!   - page picker (multi-page menu support)
//!   - live "Actions Ring" preview
//!   - Easy-Switch toggle
//!   - selected-slice editor (action / icon / colour / etc.)
//!
//! Mouse-button assignments USED to live on the left of this
//! tab; they're now on `Tab::MouseButtons` (separate sidebar
//! entry) so the radial-menu editor gets the full window width
//! to work in.

pub mod picker;
pub mod rows;
pub mod widget_options;

use crate::radial_preview::radial_preview_widget;
use crate::{Message, State};
use iced::widget::{
    button, column, container, pick_list, row, rule, text, text_input, toggler, Space,
};
use iced::{Alignment, Element, Length};
use oxidemx_shared::{ActionKind, Condition, RadialPage, Slice};
use oxidemx_widgets::style;

// ============================================================================
// View entry
// ============================================================================

pub fn view(state: &State) -> Element<'_, Message> {
    container(
        column![
            page_picker_card(state),
            radial_preview_card(state),
            easy_switch_panel(state),
            slice_editor_section(state),
        ]
        .spacing(16),
    )
    // Take full panel width — the editor can use every pixel
    // to keep slice rows + preview + easy-switch from cramping.
    .width(Length::Fill)
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
        return container(
            text("(no pages — internal error)")
                .size(11)
                .style(style::text_dim(pal)),
        )
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
    let picker = pick_list(options, selected, |c: PageChoice| {
        Message::SetActivePage(c.idx)
    })
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
    // Duplicate is always available — even with one page, copying
    // it gives the user a starting point for variant pages.
    let dup_btn = button(text("Duplicate").size(11))
        .style(style::btn_secondary(pal))
        .on_press(Message::DuplicatePage(active));
    let mut delete_btn = button(text("Delete").size(11)).style(style::btn_danger(pal));
    if state.config.radial_menu.pages.len() > 1 {
        delete_btn = delete_btn.on_press(Message::DeletePage(active));
    }
    row![left_btn, right_btn, dup_btn, delete_btn]
        .spacing(6)
        .into()
}

fn page_props_editor(state: &State, active: usize) -> Element<'_, Message> {
    let pal = &state.palette;
    let page = match state.config.radial_menu.pages.get(active) {
        Some(p) => p,
        None => return Space::new().height(Length::Fixed(0.0)).into(),
    };

    let active_for_name = active;
    let name_input = text_input("Page name", page.name.as_str())
        .on_input(move |v| Message::SetPageName {
            page: active_for_name,
            name: v,
        })
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
        let waiting_btn =
            button(text("Switch to target app… (4s)").size(11)).style(style::btn_secondary(pal));
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

    let context_hint =
        if page.app_classes.is_empty() {
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

    let active_for_slots = active;
    let slot_count_row = row![
        column![
            text("Slot count").size(13),
            text(
                "How many wedges this page renders. 8 is the default \
                 dense layout; 4-5 makes each slice bigger and easier \
                 to hit by mouse-flick."
            )
            .size(11)
            .style(style::text_dim(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        pick_list(
            SLOT_COUNT_OPTIONS.as_slice(),
            Some(SlotCountOption(page.effective_slot_count())),
            move |opt: SlotCountOption| Message::SetPageSlotCount {
                page: active_for_slots,
                count: opt.0,
            },
        )
        .style(style::pick_list_style(pal))
        .text_size(12),
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    column![
        name_input,
        classes_row,
        context_hint,
        scroll_row,
        slot_count_row
    ]
    .spacing(8)
    .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlotCountOption(u8);

impl std::fmt::Display for SlotCountOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} wedges", self.0)
    }
}

const SLOT_COUNT_OPTIONS: [SlotCountOption; 7] = [
    SlotCountOption(2),
    SlotCountOption(3),
    SlotCountOption(4),
    SlotCountOption(5),
    SlotCountOption(6),
    SlotCountOption(7),
    SlotCountOption(8),
];

fn format_page_label(idx: usize, page: &RadialPage) -> String {
    let n = page.name.trim();
    let display_name = if n.is_empty() {
        format!("Page {}", idx + 1)
    } else {
        n.to_string()
    };
    // Count slices that actually have content (non-empty label OR
    // command). The schema may pre-pad to 8 slots with "(empty)"
    // placeholders for the drag-to-swap UX, and those shouldn't
    // inflate the count shown to the user.
    let slice_count = page
        .slices
        .iter()
        .filter(|s| !s.label.trim().is_empty() || !s.command.trim().is_empty())
        .count();
    let count_str = format!(
        "{slice_count} slice{}",
        if slice_count == 1 { "" } else { "s" }
    );
    if page.app_classes.is_empty() {
        format!("{}: {} — {}", idx + 1, display_name, count_str)
    } else {
        let preview: Vec<&str> = page
            .app_classes
            .iter()
            .take(2)
            .map(String::as_str)
            .collect();
        let suffix = if page.app_classes.len() > preview.len() {
            format!("{}, …", preview.join(", "))
        } else {
            preview.join(", ")
        };
        format!("{}: {} ({}) — {}", idx + 1, display_name, suffix, count_str)
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

/// Slice editor — the design handoff's reorder-row list (`SlotRow`
/// and `JRSectionHead` in `docs/design-system/menu-page.jsx`): a
/// section head with an "Add slice" button, then one compact row
/// per slot index. The selected slot expands *inline*, rendering
/// the full slice card in its list position; clicking another row
/// moves the expansion (plain `SelectSlice` semantics — the radial
/// preview's click-to-select drives the same state).
fn slice_editor_section(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    let header = row![
        text("Slice editor").size(14),
        Space::new().width(Length::Fill),
        button(text("+ Add slice").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::AddSlice),
    ]
    .align_y(Alignment::Center);

    let slices = state.active_slices();
    let slot_count = state
        .config
        .radial_menu
        .pages
        .get(state.active_page)
        .map(|p| p.effective_slot_count() as usize)
        .unwrap_or(8);
    // One row per slot the page renders; rows past `slices.len()`
    // are "(empty)". When the slice list is *longer* than the slot
    // count (user lowered the count), the overflow slices still get
    // rows — hiding configured slices would look like data loss.
    let n_rows = slot_count.max(slices.len());
    let last_idx = slices.len().saturating_sub(1);

    let mut col = column![header].spacing(8);
    for idx in 0..n_rows {
        let slice = slices.get(idx);
        match (state.selected_slice == Some(idx), slice) {
            // Expanded: the full slice card replaces the row.
            (true, Some(s)) => {
                col = col.push(slice_editor_row(state, idx, s, last_idx));
            }
            // Collapsed (also covers a selected-but-missing slot —
            // transient, since SelectSlice pads the list in update).
            _ => col = col.push(rows::slot_row(state, idx, slice, last_idx)),
        }
    }
    col.into()
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
    let desc_input = text_input(
        "Description (notes / tooltip — optional)",
        slice.description.as_str(),
    )
    .on_input(move |s| Message::SetSliceDescription(idx, s))
    .padding(5)
    .size(11);
    // Action-kind-specific value editor. The slice's `command`
    // field carries different things depending on `kind`:
    //   * Exec / Settings / Emoji  → shell command
    //   * Macro                    → macro id (we render a pick_list
    //                                of saved macros for usability)
    //   * Shortcut                 → key chord ("ctrl+shift+v")
    //   * EasySwitch               → 1-based host index ("1", "2", "3")
    let cmd_input: Element<Message> = action_value_editor(state, idx, slice);
    // "Pick app…" only makes sense for shell-command kinds; hiding
    // it for Macro / Shortcut / EasySwitch keeps the row tidy and
    // avoids tempting the user with a control that would overwrite
    // their carefully-set macro id with a flatpak run command.
    let needs_app_pick = matches!(
        slice.kind,
        ActionKind::Exec | ActionKind::Emoji | ActionKind::Settings
    );
    let app_pick_btn: Element<Message> = if needs_app_pick {
        button(text("Pick app…").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::OpenAppCommandPicker(
                crate::app_picker::AppCommandTarget::Slice(idx),
            ))
            .into()
    } else {
        Space::new().width(Length::Shrink).into()
    };

    // Behavior chip + inline picker panel — replaces the legacy
    // kind pick_list (see picker.rs). The colour picker and the
    // kind-specific value editor below stay.
    let behavior = picker::behavior_section(state, idx, slice);

    // Schema-driven options card (spec §10d) — renders under the
    // chip when the slice hosts a Ready custom widget that declares
    // options; collapses to nothing otherwise (widget_options.rs
    // decides internally).
    let options_card = widget_options::options_section(state, idx, slice);

    // Selected colour: "Full colour" sentinel takes priority when
    // icon_untinted is set; otherwise the slice's actual palette
    // key (defaulting to "accent" for legacy / empty values).
    let selected_color = if slice.icon_untinted {
        ColorOption(FULL_COLOR_KEY.to_string())
    } else if slice.color.is_empty() {
        ColorOption("accent".to_string())
    } else {
        ColorOption(slice.color.clone())
    };
    let color_picker = pick_list(color_options(), Some(selected_color), move |opt| {
        Message::SetSliceColor(idx, opt.0)
    })
    .style(style::pick_list_style(pal))
    .text_size(12);

    // Icon name + a "Browse" button that opens the visual picker.
    // Text input still works for icons outside the curated
    // catalogue (paste any freedesktop name or absolute path).
    let icon_input = text_input(
        "Icon (e.g. \"system-run-symbolic\" or /path/to/icon.svg)",
        slice.icon.as_str(),
    )
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
    // Widget slices draw their own slice content, so the icon input
    // is hidden for them (colour + visibility stay, spec §10d).
    let icon_row: Element<Message> = if slice.kind == ActionKind::Widget {
        text("The widget draws its own slice content — no icon needed. Colour and visibility still apply.")
            .size(11)
            .style(style::text_faint(pal))
            .into()
    } else {
        row![icon_input, icon_browse, icon_file_btn]
            .align_y(Alignment::Center)
            .spacing(8)
            .into()
    };

    // (Original-colour toggle moved into the colour pick_list as
    // the "Full colour" option — saves vertical space and ties
    // the rendering mode to the colour choice that drives it.)

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
        behavior,
        options_card,
        label_input,
        desc_input,
        row![
            color_picker,
            iced::widget::container(cmd_input).width(Length::Fill),
            app_pick_btn,
        ]
        .align_y(Alignment::Center)
        .spacing(8),
        icon_row,
        visibility_editor(
            state,
            VisibilityTarget::Slice { idx },
            slice.visible_if.as_ref(),
        ),
    ]
    .spacing(6);

    // (Pickers used to render inline here, but now they take
    // over the entire content area as a full panel — see the
    // shell view() in main.rs which short-circuits when any
    // picker state is Some.)

    // Submenu editor — only when this slice's kind is Submenu.
    // Lists each sub-item with a label / command / colour picker
    // and reorder + delete buttons. "+ Add item" appends a new
    // Exec sub-item to the end.
    if slice.kind == ActionKind::Submenu {
        col = col.push(submenu_editor(state, idx, &slice.submenu));
    }

    container(col)
        .padding(10)
        .style(style::card_quiet(pal))
        .into()
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
/// Where a visibility-editor change should be sent. `Slice` writes
/// to `state.config[active_page].slices[idx].visible_if`; `SubItem`
/// writes one level deeper. The single editor function dispatches
/// the right `Message` variant based on which target is in use,
/// so the slice + sub-item editors share one implementation.
#[derive(Debug, Clone, Copy)]
enum VisibilityTarget {
    Slice { idx: usize },
    SubItem { parent: usize, idx: usize },
}

impl VisibilityTarget {
    fn make_message(&self, condition: Option<Condition>) -> Message {
        match *self {
            VisibilityTarget::Slice { idx } => Message::SetSliceVisibility {
                slice: idx,
                condition,
            },
            VisibilityTarget::SubItem { parent, idx } => Message::SetSubItemVisibility {
                parent,
                idx,
                condition,
            },
        }
    }
}

/// Layout: small header → variant picker + (optional) one or two
/// text inputs for the variant's args. Single function used for
/// both slice-level and sub-item visibility predicates — the
/// `target` enum carries enough context to dispatch the right
/// Message variant on every change.
fn visibility_editor<'a>(
    state: &'a State,
    target: VisibilityTarget,
    current: Option<&'a Condition>,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let kind = VisKind::from_condition(current);

    let mut options: Vec<VisKind> = VisKind::ALL_SIMPLE.to_vec();
    if kind == VisKind::Compound {
        options.push(VisKind::Compound);
    }

    let prev_clone = current.cloned();
    let target_for_kind = target;
    let kind_picker = pick_list(options, Some(kind), move |new_kind| {
        if new_kind == VisKind::Compound {
            return target_for_kind.make_message(prev_clone.clone());
        }
        target_for_kind.make_message(condition_for_kind(new_kind, prev_clone.as_ref()))
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
            let owned_name = name.clone();
            let target_for_input = target;
            text_input(
                "Executable name (e.g. \"git\", \"spotify\") or absolute path",
                &owned_name,
            )
            .on_input(move |v| {
                target_for_input.make_message(Some(Condition::Executable { name: v }))
            })
            .padding(5)
            .size(11)
            .into()
        }
        Some(Condition::FileExists { path }) => {
            let owned_path = path.clone();
            let target_for_input = target;
            text_input(
                "Path (~ and $VAR are expanded, e.g. \"~/.config/foo\")",
                &owned_path,
            )
            .on_input(move |v| {
                target_for_input.make_message(Some(Condition::FileExists { path: v }))
            })
            .padding(5)
            .size(11)
            .into()
        }
        Some(Condition::ProcessRunning { comm }) => {
            let owned_comm = comm.clone();
            let target_for_input = target;
            text_input(
                "Process comm (the short name in /proc/PID/comm, e.g. \"spotify\")",
                &owned_comm,
            )
            .on_input(move |v| {
                target_for_input.make_message(Some(Condition::ProcessRunning { comm: v }))
            })
            .padding(5)
            .size(11)
            .into()
        }
        Some(Condition::EnvSet { var }) => {
            let owned_var = var.clone();
            let target_for_input = target;
            text_input(
                "Environment variable name (e.g. \"WAYLAND_DISPLAY\")",
                &owned_var,
            )
            .on_input(move |v| {
                target_for_input.make_message(Some(Condition::EnvSet { var: v }))
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
            let target_for_var = target;
            let target_for_val = target;
            row![
                text_input("Variable", &var_for_var)
                    .on_input(move |v| {
                        target_for_var.make_message(Some(Condition::EnvEquals {
                            var: v,
                            value: val_for_var.clone(),
                        }))
                    })
                    .padding(5)
                    .size(11)
                    .width(Length::FillPortion(2)),
                text("=").size(11).style(style::text_faint(pal)),
                text_input("Value", &val_for_val)
                    .on_input(move |v| {
                        target_for_val.make_message(Some(Condition::EnvEquals {
                            var: var_for_val.clone(),
                            value: v,
                        }))
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
                "Compound predicate ({} sub-condition{}) — edit ~/.config/oxidemx/config.json directly to modify.",
                conditions.len(),
                if conditions.len() == 1 { "" } else { "s" }
            ))
            .size(10)
            .style(style::text_faint(pal))
            .into()
        }
        Some(Condition::Not { .. }) => text(
            "Compound NOT predicate — edit ~/.config/oxidemx/config.json directly to modify.",
        )
        .size(10)
        .style(style::text_faint(pal))
        .into(),
        _ => Space::new().height(Length::Fixed(0.0)).into(),
    };

    column![header, args].spacing(4).into()
}

fn submenu_editor<'a>(state: &'a State, parent: usize, items: &'a [Slice]) -> Element<'a, Message> {
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

    let mut col = column![header, rule::horizontal(1).style(style::rule_style(pal))].spacing(6);
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
    container(col).padding(8).style(style::card(pal)).into()
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
    // Same kind-aware value editor as `slice_editor_row` — Macro
    // gets a dropdown of saved macros, EasySwitch gets a host
    // picker, Shortcut shows a chord-format placeholder, etc.
    let cmd_input: Element<Message> = sub_item_value_editor(state, parent, idx, item);
    let selected_color = if item.icon_untinted {
        ColorOption(FULL_COLOR_KEY.to_string())
    } else if item.color.is_empty() {
        ColorOption("accent".to_string())
    } else {
        ColorOption(item.color.clone())
    };
    let color_picker = pick_list(color_options(), Some(selected_color), move |opt| {
        Message::SetSubItemColor(parent, idx, opt.0)
    })
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
    let sub_needs_app_pick = matches!(
        item.kind,
        ActionKind::Exec | ActionKind::Emoji | ActionKind::Settings
    );
    let sub_app_btn: Element<Message> = if sub_needs_app_pick {
        button(text("Pick app…").size(10))
            .style(style::btn_secondary(pal))
            .on_press(Message::OpenAppCommandPicker(
                crate::app_picker::AppCommandTarget::SubItem { parent, idx },
            ))
            .into()
    } else {
        Space::new().width(Length::Shrink).into()
    };
    let sub_browse_btn = button(text("Browse…").size(10))
        .style(style::btn_secondary(pal))
        .on_press(Message::OpenIconPicker(sub_browse_target));
    let sub_file_btn = button(text("File…").size(10))
        .style(style::btn_secondary(pal))
        .on_press(Message::BrowseIconFile(sub_browse_target));

    // (Untinted toggle folded into the colour pick_list above.)
    let col = column![
        row![
            text(format!("{}.", idx + 1))
                .size(10)
                .style(style::text_faint(pal)),
            label_input,
            color_picker,
            iced::widget::container(cmd_input).width(Length::FillPortion(3)),
            test_btn,
            up_btn,
            down_btn,
            del_btn,
        ]
        .align_y(Alignment::Center)
        .spacing(6),
        row![
            kind_picker,
            icon_input.width(Length::Fill),
            sub_app_btn,
            sub_browse_btn,
            sub_file_btn
        ]
        .align_y(Alignment::Center)
        .spacing(6),
        visibility_editor(
            state,
            VisibilityTarget::SubItem { parent, idx },
            item.visible_if.as_ref(),
        ),
    ]
    .spacing(4);

    // (Pickers render full-panel via the shell short-circuit in
    // main.rs::view, no inline rendering here any more.)
    col.into()
}

// ============================================================================
// Action-kind-specific value editor
// ============================================================================

/// Render the right widget for editing `slice.command` based on
/// the slice's `kind`. Generic kinds (Exec / Settings / Emoji)
/// fall back to a free-form text input. Macro / EasySwitch get
/// pick_lists so the user doesn't have to remember an opaque id.
/// Shortcut keeps a text input but with a kind-specific
/// placeholder hint.
fn action_value_editor<'a>(state: &'a State, idx: usize, slice: &'a Slice) -> Element<'a, Message> {
    let pal = &state.palette;
    match slice.kind {
        ActionKind::Macro => {
            let options: Vec<MacroOption> = state
                .macros
                .iter()
                .map(|m| MacroOption {
                    id: m.id.clone(),
                    label: if m.name.trim().is_empty() {
                        m.id.clone()
                    } else {
                        m.name.clone()
                    },
                })
                .collect();
            let selected = options.iter().find(|o| o.id == slice.command).cloned();
            if options.is_empty() {
                // No macros recorded yet — render a hint instead of
                // an empty pick_list so the user knows where to go.
                return text("Record a macro on the Macros tab first.")
                    .size(11)
                    .style(style::text_dim(pal))
                    .into();
            }
            pick_list(options, selected, move |opt: MacroOption| {
                Message::SetSliceCommand(idx, opt.id)
            })
            .style(style::pick_list_style(pal))
            .text_size(12)
            .placeholder("Pick a macro…")
            .into()
        }
        ActionKind::EasySwitch => {
            let options = host_options(state);
            let selected = slice.command.parse::<u8>().ok().map(|n| HostOption {
                idx: n,
                name: options
                    .iter()
                    .find(|o| o.idx == n)
                    .and_then(|o| o.name.clone()),
            });
            pick_list(options, selected, move |opt: HostOption| {
                Message::SetSliceCommand(idx, opt.idx.to_string())
            })
            .style(style::pick_list_style(pal))
            .text_size(12)
            .placeholder("Pick host…")
            .into()
        }
        ActionKind::Shortcut => {
            let target = crate::ShortcutCaptureTarget::Slice(idx);
            let capturing = state.capturing_shortcut == Some(target);
            let placeholder = if capturing {
                "Press a key chord… (Esc to cancel)".to_string()
            } else {
                "Key chord (e.g. \"ctrl+shift+v\", \"super+e\")".to_string()
            };
            let input = text_input(&placeholder, slice.command.as_str())
                .on_input(move |s| Message::SetSliceCommand(idx, s))
                .padding(6)
                .size(12);
            let capture_btn = button(text(if capturing { "Cancel" } else { "Capture" }).size(11))
                .style(style::btn_secondary(pal))
                .on_press(Message::BeginShortcutCapture(target));
            row![input.width(Length::Fill), capture_btn]
                .align_y(Alignment::Center)
                .spacing(6)
                .into()
        }
        ActionKind::Submenu => text("(no command — sub-items dispatch instead)")
            .size(11)
            .style(style::text_dim(pal))
            .into(),
        ActionKind::Power => {
            let selected = POWER_OPTIONS.iter().find(|o| o.0 == slice.command).copied();
            pick_list(POWER_OPTIONS.to_vec(), selected, move |opt: PowerOption| {
                Message::SetSliceCommand(idx, opt.0.to_string())
            })
            .style(style::pick_list_style(pal))
            .text_size(12)
            .placeholder("Pick power action…")
            .into()
        }
        ActionKind::MouseSetting => text_input(
            "dpi:1600 | smartshift | haptics | gaming",
            slice.command.as_str(),
        )
        .on_input(move |s| Message::SetSliceCommand(idx, s))
        .padding(6)
        .size(12)
        .into(),
        ActionKind::NightLight => text("(no command — toggles GNOME night light)")
            .size(11)
            .style(style::text_dim(pal))
            .into(),
        ActionKind::Widget => {
            // Data source is chosen through the behavior picker
            // (chip → Change…) — the legacy pick_list is gone.
            text("(data source — pick via Change… above)")
                .size(11)
                .style(style::text_dim(pal))
                .into()
        }
        ActionKind::Dial => {
            let selected = slice.dial.map(DialKindOption);
            pick_list(DIAL_OPTIONS.to_vec(), selected, move |o: DialKindOption| {
                Message::SetSliceDial(idx, o.0)
            })
            .style(style::pick_list_style(pal))
            .text_size(12)
            .placeholder("Pick dial target…")
            .into()
        }
        ActionKind::None => text("(no action)")
            .size(11)
            .style(style::text_dim(pal))
            .into(),
        ActionKind::Exec | ActionKind::Settings | ActionKind::Emoji => {
            text_input("Command", slice.command.as_str())
                .on_input(move |s| Message::SetSliceCommand(idx, s))
                .padding(6)
                .size(12)
                .into()
        }
    }
}

/// Sub-item analogue of `action_value_editor`. Identical control
/// shapes; only the `Message` constructor differs (`SetSubItemCommand`
/// vs `SetSliceCommand`) so the changes flow back into the right
/// nested struct.
fn sub_item_value_editor<'a>(
    state: &'a State,
    parent: usize,
    idx: usize,
    item: &'a Slice,
) -> Element<'a, Message> {
    let pal = &state.palette;
    match item.kind {
        ActionKind::Macro => {
            let options: Vec<MacroOption> = state
                .macros
                .iter()
                .map(|m| MacroOption {
                    id: m.id.clone(),
                    label: if m.name.trim().is_empty() {
                        m.id.clone()
                    } else {
                        m.name.clone()
                    },
                })
                .collect();
            let selected = options.iter().find(|o| o.id == item.command).cloned();
            if options.is_empty() {
                return text("Record a macro on the Macros tab first.")
                    .size(10)
                    .style(style::text_dim(pal))
                    .into();
            }
            pick_list(options, selected, move |opt: MacroOption| {
                Message::SetSubItemCommand(parent, idx, opt.id)
            })
            .style(style::pick_list_style(pal))
            .text_size(11)
            .placeholder("Pick a macro…")
            .into()
        }
        ActionKind::EasySwitch => {
            let options = host_options(state);
            let selected = item.command.parse::<u8>().ok().map(|n| HostOption {
                idx: n,
                name: options
                    .iter()
                    .find(|o| o.idx == n)
                    .and_then(|o| o.name.clone()),
            });
            pick_list(options, selected, move |opt: HostOption| {
                Message::SetSubItemCommand(parent, idx, opt.idx.to_string())
            })
            .style(style::pick_list_style(pal))
            .text_size(11)
            .placeholder("Pick host…")
            .into()
        }
        ActionKind::Shortcut => {
            let target = crate::ShortcutCaptureTarget::SubItem { parent, idx };
            let capturing = state.capturing_shortcut == Some(target);
            let placeholder = if capturing {
                "Press a key chord… (Esc to cancel)".to_string()
            } else {
                "Key chord (e.g. \"ctrl+shift+v\")".to_string()
            };
            let input = text_input(&placeholder, item.command.as_str())
                .on_input(move |s| Message::SetSubItemCommand(parent, idx, s))
                .padding(5)
                .size(11);
            let capture_btn = button(text(if capturing { "Cancel" } else { "Capture" }).size(10))
                .style(style::btn_secondary(pal))
                .on_press(Message::BeginShortcutCapture(target));
            row![input.width(Length::Fill), capture_btn]
                .align_y(Alignment::Center)
                .spacing(4)
                .into()
        }
        ActionKind::Submenu => text("(submenu — sub-items don't nest)")
            .size(10)
            .style(style::text_dim(pal))
            .into(),
        ActionKind::Power => {
            let selected = POWER_OPTIONS.iter().find(|o| o.0 == item.command).copied();
            pick_list(POWER_OPTIONS.to_vec(), selected, move |opt: PowerOption| {
                Message::SetSubItemCommand(parent, idx, opt.0.to_string())
            })
            .style(style::pick_list_style(pal))
            .text_size(11)
            .placeholder("Pick power action…")
            .into()
        }
        ActionKind::MouseSetting => text_input(
            "dpi:1600 | smartshift | haptics | gaming",
            item.command.as_str(),
        )
        .on_input(move |s| Message::SetSubItemCommand(parent, idx, s))
        .padding(5)
        .size(11)
        .into(),
        ActionKind::NightLight => text("(no command — toggles GNOME night light)")
            .size(10)
            .style(style::text_dim(pal))
            .into(),
        ActionKind::Widget | ActionKind::Dial => {
            text("(live wedge — data source set in config.json)")
                .size(10)
                .style(style::text_dim(pal))
                .into()
        }
        ActionKind::None => text("(no action)")
            .size(10)
            .style(style::text_dim(pal))
            .into(),
        ActionKind::Exec | ActionKind::Settings | ActionKind::Emoji => {
            text_input("Command", item.command.as_str())
                .on_input(move |s| Message::SetSubItemCommand(parent, idx, s))
                .padding(5)
                .size(11)
                .into()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MacroOption {
    id: String,
    label: String,
}

impl std::fmt::Display for MacroOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// A single host slot in the Easy-Switch picker. `idx` is the
/// 1-based number printed on the mouse; `name` is the friendly
/// label the daemon learned from the device pairing (e.g.
/// "MacBook Pro", "ThinkPad", or "Host 1" when nothing's bonded).
/// `PartialEq` ignores `name` so the picker correctly highlights
/// the saved index even if the friendly name lookup hasn't
/// completed (or the host name has since changed).
#[derive(Debug, Clone)]
struct HostOption {
    idx: u8,
    name: Option<String>,
}

impl PartialEq for HostOption {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx
    }
}
impl Eq for HostOption {}

impl std::fmt::Display for HostOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.name.as_deref().filter(|n| !n.trim().is_empty()) {
            Some(n) => write!(f, "Host {} — {}", self.idx, n),
            None => write!(f, "Host {}", self.idx),
        }
    }
}

/// Build the host picker options. Pulls names from the daemon
/// snapshot when available (slot count + names polled every 5 s
/// in the main poll loop); falls back to a generic 3-host list
/// when the device hasn't been read yet or isn't connected.
fn host_options(state: &State) -> Vec<HostOption> {
    let names = state
        .daemon
        .easy_switch
        .as_ref()
        .map(|es| es.host_names.clone())
        .unwrap_or_default();
    let slot_count = state
        .daemon
        .easy_switch
        .as_ref()
        .map(|es| es.slot_count.max(1) as usize)
        .unwrap_or(3);
    (1..=slot_count as u8)
        .map(|idx| HostOption {
            idx,
            name: names.get(idx as usize - 1).cloned(),
        })
        .collect()
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
            ActionKind::Widget => "Live widget",
            ActionKind::Dial => "Dial (scroll-adjust)",
            ActionKind::Power => "Power action",
            ActionKind::NightLight => "Night light toggle",
            ActionKind::MouseSetting => "Mouse quick setting",
            ActionKind::None => "Do nothing",
        })
    }
}

const KIND_OPTIONS: [KindOption; 13] = [
    KindOption(ActionKind::Exec),
    KindOption(ActionKind::Submenu),
    KindOption(ActionKind::Macro),
    KindOption(ActionKind::Shortcut),
    KindOption(ActionKind::EasySwitch),
    KindOption(ActionKind::Settings),
    KindOption(ActionKind::Emoji),
    KindOption(ActionKind::Widget),
    KindOption(ActionKind::Dial),
    KindOption(ActionKind::Power),
    KindOption(ActionKind::NightLight),
    KindOption(ActionKind::MouseSetting),
    KindOption(ActionKind::None),
];

/// One row of the Power-action picker — wraps the command string so
/// the pick_list can show a friendly label while storing the
/// machine value in `slice.command`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PowerOption(&'static str, &'static str);

impl std::fmt::Display for PowerOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}

// (The legacy WidgetSourceOption pick_list lived here; the behavior
// picker in picker.rs replaces it. Message::SetSliceWidgetSource
// stays handled in main.rs for compatibility.)

/// Pick-list wrapper for a dial wedge's target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialKindOption(pub oxidemx_shared::DialKind);

impl std::fmt::Display for DialKindOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            oxidemx_shared::DialKind::Brightness => "Brightness",
            oxidemx_shared::DialKind::Volume => "Volume",
        })
    }
}

const DIAL_OPTIONS: [DialKindOption; 2] = [
    DialKindOption(oxidemx_shared::DialKind::Brightness),
    DialKindOption(oxidemx_shared::DialKind::Volume),
];

const POWER_OPTIONS: [PowerOption; 5] = [
    PowerOption("lock", "Lock"),
    PowerOption("logoff", "Log off"),
    PowerOption("suspend", "Suspend"),
    PowerOption("restart", "Restart"),
    PowerOption("shutdown", "Shut down"),
];

/// Sentinel string used in the colour pick_list to signal "render
/// the icon at its original colours" (i.e. set
/// `Slice.icon_untinted = true`). Distinguished from real palette
/// keys by the leading underscores so it can never collide with a
/// theme colour name.
pub const FULL_COLOR_KEY: &str = "__full_color__";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorOption(pub String);

impl std::fmt::Display for ColorOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 == FULL_COLOR_KEY {
            f.write_str("Full color")
        } else {
            f.write_str(&self.0)
        }
    }
}

/// The curated slice palette tokens (matches oxidemx_shared's
/// ThemeColors::slice_color_rgba lookup). Shared with the widget
/// options card's `color` swatch row so widget-declared colours
/// stay on the same curated ring palette (spec §5).
pub const SLICE_PALETTE_KEYS: [&str; 11] = [
    "accent", "green", "yellow", "red", "blue", "mauve", "pink", "peach", "teal", "sapphire",
    "lavender",
];

// Catppuccin-style colour key list with a special "Full colour"
// sentinel at the top that maps to icon_untinted = true rather
// than a tint colour. Selecting it leaves the slice's underlying
// `color` unchanged so the user's previous palette choice is
// preserved when they toggle back.
fn color_options() -> Vec<ColorOption> {
    let mut v: Vec<ColorOption> = vec![ColorOption(FULL_COLOR_KEY.to_string())];
    for s in SLICE_PALETTE_KEYS {
        v.push(ColorOption(s.to_string()));
    }
    v
}

// We rebuild the list lazily per render — pick_list takes the Vec
// by `Borrow<[T]>`, so the allocation lives only as long as the
// rendered Element.
