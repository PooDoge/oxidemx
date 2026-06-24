//! Reusable Freya components for OxideMX (filled by Tasks 7–8).
pub mod avatar;
pub mod bubble;
pub mod chip;
pub mod collapsible_panel;
pub mod composer;
pub mod list_item;
pub mod prompt_input;
pub mod rail_button;
pub mod resize_grip;
pub mod sidebar_header;
pub mod status_dot;
pub mod status_puck;
pub mod thread_header;

pub use avatar::Avatar;
pub use bubble::Bubble;
pub use chip::WorktreeChip;
pub use collapsible_panel::CollapsiblePanel;
pub use composer::ComposerConfig;
pub use list_item::ListItem;
pub use prompt_input::PromptInput;
pub use rail_button::RailButton;
pub use resize_grip::{ResizeGrip, clamp_height};
pub use sidebar_header::SidebarHeader;
pub use status_dot::StatusDot;
pub use status_puck::StatusPuck;
pub use thread_header::ThreadHeader;
