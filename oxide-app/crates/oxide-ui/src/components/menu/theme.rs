//! Shared menu theme helpers — container + item partials for all OxideMX menus.
//!
//! Key fix: `hover_background = surface_hi()` (a dark mid-tone) so near-white
//! text stays readable on hover, replacing the previous light-on-light default.
use freya::prelude::*;

use crate::tokens::Theme;

/// Returns `(container_theme, item_theme)` for a standard OxideMX menu.
///
/// - Container: `panel()` bg, `hairline()` border, transparent shadow (the
///   deep shadow lives on the `MenuSurface` wrapper), radius 12.
/// - Item: transparent bg, **`surface_hi()` hover** (dark — readable), accent
///   tinted select, radius 9, `text()` label color.
pub fn menu_theme(theme: Theme) -> (MenuContainerThemePartial, MenuItemThemePartial) {
    let accent = theme.accent();

    let container = MenuContainerThemePartial::new()
        .background(theme.panel())
        .border_fill(theme.hairline())
        .shadow(Color::TRANSPARENT)
        .corner_radius(CornerRadius::new_all(12.));

    let item = MenuItemThemePartial::new()
        .background(Color::TRANSPARENT)
        .hover_background(theme.surface_hi())
        .select_background(Theme::with_alpha(accent, 0x14))
        .border_fill(Color::TRANSPARENT)
        .select_border_fill(Theme::with_alpha(accent, 0x33))
        .corner_radius(CornerRadius::new_all(9.))
        .color(theme.text());

    (container, item)
}
