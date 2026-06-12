//! Page-list construction from config (`pages_from_config`,
//! including the auto-appended AI Assistant page) and focused-window
//! class matching.

use oxidemx_shared::{ActionKind, AppConfig, RadialPage, Slice};

/// Name of the auto-appended AI Assistant page. Shared between the
/// page builder, the morph trigger, and the view layer.
pub const AI_PAGE_NAME: &str = "AI Assistant";

/// Build the page list to seed `RadialState`. Mirrors the
/// loader's `normalize_pages` semantics so a config built in code
/// (e.g. `AppConfig::default()`) still yields a non-empty page
/// vec.
pub(super) fn pages_from_config(config: &AppConfig) -> Vec<RadialPage> {
    let mut pages = config.radial_menu.pages.clone();
    if pages.is_empty() {
        // Defensive: should never happen post-`normalize_pages`,
        // but a hand-built `AppConfig::default()` (used by the
        // overlay's panic path) has pages.is_empty()==true.
        if !config.radial_menu.slices.is_empty() {
            pages.push(RadialPage {
                name: "Default".into(),
                slices: config.radial_menu.slices.clone(),
                app_classes: Vec::new(),
                include_in_scroll: true,
                slot_count: 8,
            });
        } else {
            pages.push(RadialPage::default());
        }
    }
    // Append the AI page
    pages.push(RadialPage {
        name: AI_PAGE_NAME.into(),
        slices: vec![
            Slice {
                action_id: None,
                label: "Clear Chat".into(),
                kind: ActionKind::Macro,
                command: "ai_clear_history".into(),
                color: "red".into(),
                icon: "edit-clear-symbolic".into(),
                icon_untinted: false,
                description: String::new(),
                submenu: vec![],
                visible_if: None,
                widget: None,
                dial: None,
            },
            Slice {
                action_id: None,
                label: "Close Menu".into(),
                kind: ActionKind::Macro,
                command: "ai_close_menu".into(),
                color: "peach".into(),
                icon: "window-close-symbolic".into(),
                icon_untinted: false,
                description: String::new(),
                submenu: vec![],
                visible_if: None,
                widget: None,
                dial: None,
            },
        ],
        app_classes: vec![],
        include_in_scroll: true,
        slot_count: 8,
    });
    pages
}

/// Find the first page whose `app_classes` contains the given
/// focused window class (case-insensitive). Empty class returns
/// None so the caller keeps whatever page was previously active.
pub(super) fn match_page_for_class(pages: &[RadialPage], class: &str) -> Option<usize> {
    if class.is_empty() {
        return None;
    }
    let lc = class.to_lowercase();
    pages
        .iter()
        .position(|p| p.app_classes.iter().any(|c| c.to_lowercase() == lc))
}
