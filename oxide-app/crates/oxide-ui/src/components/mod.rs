//! Reusable Freya components for OxideMX (filled by Tasks 7–8).
pub mod avatar;
pub mod bubble;
pub mod chip;
pub mod composer;
pub mod list_item;
pub mod menu;
pub mod prompt_input;
pub mod rail_button;
pub mod resize_grip;
pub mod sidebar_header;
pub mod status_dot;
pub mod status_puck;
pub mod text_input;
pub mod thread_header;

pub use avatar::Avatar;
pub use bubble::Bubble;
pub use chip::WorktreeChip;
pub use composer::{Composer, ComposerConfig, SubmitPayload};
pub use list_item::ListItem;
pub use menu::{
    copy_only_menu, copy_selection, cut_selection, editor_clipboard_menu, menu_theme, paste_text,
    select_all, MenuRow, MenuSection, MenuSurface, Placement, Popover,
};
pub use prompt_input::PromptInput;
pub use rail_button::RailButton;
pub use resize_grip::{ResizeGrip, clamp_height};
pub use sidebar_header::SidebarHeader;
pub use status_dot::StatusDot;
pub use status_puck::StatusPuck;
pub use text_input::TextInput;
pub use thread_header::ThreadHeader;
