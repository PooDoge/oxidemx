//! Reusable menu primitives for OxideMX floating menus.
//!
//! Fixes:
//! - Light-on-light hover: `menu_theme()` sets `hover_background = surface_hi()`
//!   (a dark mid-tone) so near-white text stays readable.
//! - Full-width attach menu: `MenuSurface` wraps the `Menu` in a `Content::fit()`
//!   rect that constrains width to `[min_w, max_w]`.
//!
//! Usage:
//! ```ignore
//! MenuSurface::new(theme)
//!     .child(
//!         rect().direction(Direction::Vertical)
//!             .child(MenuSection::new(theme, "Files", Some(Tone::Blue)).icon("folder"))
//!             .child(MenuRow::new(theme).icon(Some("folder")).title("Open…").on_press(…))
//!     )
//! ```
pub mod popover;
pub mod row;
pub mod surface;
pub mod text_menu;
pub mod theme;

pub use popover::{Placement, Popover};
pub use row::{MenuRow, MenuSection};
pub use surface::MenuSurface;
pub use text_menu::{
    copy_only_menu, copy_selection, cut_selection, editor_clipboard_menu, paste_text, select_all,
};
pub use theme::menu_theme;
