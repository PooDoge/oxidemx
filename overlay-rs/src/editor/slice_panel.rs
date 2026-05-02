//! Per-slice editor panel. Bound to whichever slice is currently
//! selected in the editor's slice list. Mutates a `Slice` in place and
//! emits a "changed" signal back to the editor window so the live
//! preview re-renders.

// TODO: GtkEntry for label, GtkDropDown for ActionKind, GtkEntry for
// command, GtkColorButton (or a custom palette dropdown) for color,
// IconPickerButton for icon, GtkSwitch + nested SlicePanel(s) for
// submenu items.
