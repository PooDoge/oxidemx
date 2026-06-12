//! Compact reorder rows for the slice editor list — the design
//! handoff's `SlotRow` (`jr-reorder-row` in
//! `docs/design-system/menu-page.jsx`). The Menu tab renders one
//! row per slot index; the selected slot renders the full slice
//! card in its place instead (inline expansion).
//!
//! A row is: stacked up/down chevrons (reorder handle), the slice
//! icon tinted with the slice colour, label + `Slot {n}` tag, a
//! one-line behavior summary (shared with the chip via
//! [`picker::chip_summary`]), and a kind tag (ACTION / WIDGET /
//! SUBMENU / …). Clicking anywhere selects the slot; the chevrons
//! capture their own clicks so reorder doesn't also re-select.

use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{Alignment, Element, Length};
use oxidemx_shared::{ActionKind, Slice};
use oxidemx_widgets::style;

use super::picker;
use crate::{Message, State};

/// Row icon size (design: 15 px).
const ROW_ICON_PX: u32 = 15;

/// Hard cap on how many slots the empty-slot padding may create —
/// matches the overlay's 8-wedge maximum (`RadialPage::
/// effective_slot_count` clamps to 2..=8).
pub const MAX_SLOTS: usize = 8;

/// A fresh "does nothing yet" slice used to back an empty slot the
/// user just clicked. Kind `None` renders as an empty wedge and
/// dispatches nothing, so creating it is semantically a no-op until
/// the user picks a behavior.
pub fn empty_slice() -> Slice {
    Slice {
        action_id: None,
        label: String::new(),
        kind: ActionKind::None,
        command: String::new(),
        color: "accent".into(),
        icon: String::new(),
        submenu: Vec::new(),
        visible_if: None,
        icon_untinted: false,
        description: String::new(),
        widget: None,
        dial: None,
    }
}

/// Pad `slices` with [`empty_slice`] placeholders so index `idx`
/// exists. Returns `true` when padding actually happened (caller
/// should `touch()`). Capped at [`MAX_SLOTS`] so a stray index can
/// never grow the list past what the overlay can render.
pub fn ensure_slot_exists(slices: &mut Vec<Slice>, idx: usize) -> bool {
    if idx >= MAX_SLOTS || idx < slices.len() {
        return false;
    }
    while slices.len() <= idx {
        slices.push(empty_slice());
    }
    true
}

/// Is this slice an unconfigured placeholder? Covers both shapes in
/// the wild: the empty-label slices from [`empty_slice`] and the
/// legacy `"(empty)"`-labelled pads written by the drag-to-swap
/// handler. A *deliberate* "Do nothing" slice (user gave it a label
/// or command) is NOT a placeholder — selecting it shouldn't fling
/// the picker open.
pub fn is_placeholder(s: &Slice) -> bool {
    s.kind == ActionKind::None
        && s.command.trim().is_empty()
        && (s.label.trim().is_empty() || s.label.trim() == "(empty)")
}

/// Mono kind tag on the row's right edge (design's
/// `jr-reorder-row-kind`).
pub fn kind_tag(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Widget => "WIDGET",
        ActionKind::Submenu => "SUBMENU",
        ActionKind::Dial => "DIAL",
        ActionKind::Macro => "MACRO",
        ActionKind::None => "EMPTY",
        _ => "ACTION",
    }
}

/// One collapsed slot row. `slice` is `None` for slots past the end
/// of the page's slice list (rendered as "(empty)"); `last_idx` is
/// `slices.len() - 1` and bounds the reorder arrows.
pub fn slot_row<'a>(
    state: &'a State,
    idx: usize,
    slice: Option<&'a Slice>,
    last_idx: usize,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let empty = slice.is_none_or(is_placeholder);

    // --- reorder handle: stacked chevrons (design's jr-reorder-handle)
    let mut up_btn = button(text("▲").size(7)).padding([1, 5]).style(style::btn_flat(pal));
    if slice.is_some() && idx > 0 {
        up_btn = up_btn.on_press(Message::MoveSliceUp(idx));
    }
    let mut down_btn = button(text("▼").size(7)).padding([1, 5]).style(style::btn_flat(pal));
    if slice.is_some() && idx < last_idx {
        down_btn = down_btn.on_press(Message::MoveSliceDown(idx));
    }
    let handle = column![up_btn, down_btn].spacing(1);

    // --- 15 px icon tinted with the slice colour
    let icon: Element<Message> = {
        let tint = slice
            .map(|s| {
                let (r, g, b) = crate::radial_preview::slice_color(pal, s);
                iced::Color::from_rgb(r, g, b)
            })
            .unwrap_or(pal.overlay0);
        let handle = slice.and_then(|s| {
            crate::radial_preview::resolve_icon_handle(
                &state.icons,
                &state.iced_handles,
                &s.icon,
                ROW_ICON_PX,
                tint,
            )
        });
        let inner: Element<Message> = match handle {
            Some(h) => iced::widget::image(h)
                .width(Length::Fixed(ROW_ICON_PX as f32))
                .height(Length::Fixed(ROW_ICON_PX as f32))
                .into(),
            None => text("•").size(13).color(tint).into(),
        };
        container(inner)
            .width(Length::Fixed(18.0))
            .center_x(Length::Fixed(18.0))
            .into()
    };

    // --- label + Slot {n} tag
    let label_str = match slice {
        Some(s) if !empty && !s.label.trim().is_empty() => s.label.clone(),
        Some(s) if !empty => picker::action_kind_name(s.kind).to_string(),
        _ => "(empty)".to_string(),
    };
    let label: Element<Message> = if empty {
        text(label_str).size(13).style(style::text_dim(pal)).into()
    } else {
        text(label_str).size(13).into()
    };
    let slot_tag = text(format!("Slot {}", idx + 1))
        .size(9)
        .style(style::text_faint(pal));

    // --- one-line behavior summary (same helper as the chip)
    let summary = match slice {
        Some(s) if !empty => picker::chip_summary(state, s),
        _ => "Nothing assigned".to_string(),
    };

    // --- kind tag
    let tag = kind_tag(slice.map(|s| s.kind).unwrap_or(ActionKind::None));

    let mut body = row![
        handle,
        icon,
        label,
        slot_tag,
        Space::new().width(Length::Fill),
        text(summary).size(11).style(style::text_dim(pal)),
        container(text(tag).size(8).style(style::text_dim(pal)))
            .padding([2, 6])
            .style(style::chip(pal)),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    // Empty slots advertise the click with an explicit affordance;
    // SelectSlice creates the backing slice + opens the picker (the
    // handler pads the list — see main.rs).
    if empty {
        body = body.push(
            button(text("Assign…").size(11))
                .style(style::btn_secondary(pal))
                .on_press(Message::SelectSlice(idx)),
        );
    }

    mouse_area(
        container(body)
            .padding([6, 10])
            .width(Length::Fill)
            .style(style::card_quiet(pal)),
    )
    .on_press(Message::SelectSlice(idx))
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

// ============================================================================
// Tests — pure helpers (the row view is exercised by cargo check)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn exec_slice(label: &str) -> Slice {
        Slice {
            kind: ActionKind::Exec,
            label: label.into(),
            command: "true".into(),
            ..empty_slice()
        }
    }

    #[test]
    fn ensure_slot_pads_with_none_placeholders() {
        let mut v = vec![exec_slice("a")];
        assert!(ensure_slot_exists(&mut v, 4));
        assert_eq!(v.len(), 5);
        assert!(v[1..].iter().all(|s| s.kind == ActionKind::None));
        assert!(v[1..].iter().all(is_placeholder));
    }

    #[test]
    fn ensure_slot_noop_when_index_exists() {
        let mut v = vec![exec_slice("a"), exec_slice("b")];
        assert!(!ensure_slot_exists(&mut v, 1));
        assert_eq!(v.len(), 2);
        assert!(!ensure_slot_exists(&mut v, 0));
    }

    #[test]
    fn ensure_slot_caps_at_max_slots() {
        let mut v = Vec::new();
        assert!(!ensure_slot_exists(&mut v, MAX_SLOTS));
        assert!(v.is_empty());
        assert!(ensure_slot_exists(&mut v, MAX_SLOTS - 1));
        assert_eq!(v.len(), MAX_SLOTS);
    }

    #[test]
    fn placeholder_detection() {
        assert!(is_placeholder(&empty_slice()));
        // legacy drag-swap padding shape
        let mut legacy = empty_slice();
        legacy.label = "(empty)".into();
        assert!(is_placeholder(&legacy));
        // a deliberate "Do nothing" slice is NOT a placeholder
        let mut named = empty_slice();
        named.label = "Spacer".into();
        assert!(!is_placeholder(&named));
        // …and other kinds never are
        assert!(!is_placeholder(&exec_slice("x")));
    }

    #[test]
    fn kind_tags_match_design_vocabulary() {
        assert_eq!(kind_tag(ActionKind::Widget), "WIDGET");
        assert_eq!(kind_tag(ActionKind::Submenu), "SUBMENU");
        assert_eq!(kind_tag(ActionKind::Dial), "DIAL");
        assert_eq!(kind_tag(ActionKind::Macro), "MACRO");
        assert_eq!(kind_tag(ActionKind::None), "EMPTY");
        assert_eq!(kind_tag(ActionKind::Exec), "ACTION");
        assert_eq!(kind_tag(ActionKind::Shortcut), "ACTION");
    }
}
