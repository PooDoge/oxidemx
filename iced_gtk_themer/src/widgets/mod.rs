pub mod action_row;
pub mod avatar;
pub mod banner;
pub mod button_row;
pub mod carousel;
pub mod clamp;
pub mod dialog;
pub mod flex_row;
pub mod grid;
pub mod list;
pub mod overlay_split_view;
pub mod preferences_dialog;
pub mod preferences_group;
pub mod preferences_page;
pub mod segmented_button;
pub mod sidebar;
pub mod spin_row;
pub mod spinner;
pub mod tab_bar;
pub mod toaster;
pub mod toggle_group;
pub mod nav_bar;
pub mod view_switcher;

pub use action_row::action_row;
pub use avatar::avatar;
pub use banner::banner;
pub use button_row::button_row;
pub use carousel::carousel;
pub use clamp::clamp;
pub use dialog::{dialog, Dialog};
pub use flex_row::{flex_row, FlexRow};
pub use grid::{grid, Grid};
pub use list::{boxed_list, BoxedList};
pub use overlay_split_view::overlay_split_view;
pub use preferences_dialog::preferences_dialog;
pub use preferences_group::preferences_group;
pub use preferences_page::preferences_page;
pub use segmented_button::segmented_button;
pub use sidebar::sidebar;
pub use spin_row::spin_row;
pub use spinner::spinner;
pub use tab_bar::tab_bar;
pub use toaster::{toaster, Toaster};
pub use toggle_group::toggle_group;

/// A stub for an icon widget.
pub fn icon<'a>(name: impl Into<String>) -> iced::widget::Text<'a> {
    iced::widget::text(name.into())
}
pub use nav_bar::nav_bar;
pub use view_switcher::view_switcher;
