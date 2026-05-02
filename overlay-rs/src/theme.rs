//! Resolve a `ThemeName` to the rendering palette the radial widget
//! needs. Wraps `juhradial_shared::Theme` and provides a couple of
//! cairo-friendly accessors so the rest of the overlay doesn't have
//! to care whether the active theme is a vector palette, a 3D pre-
//! rendered wheel, or a user-installed custom one.

use juhradial_shared::{Theme, ThemeName};
use tracing::warn;

/// Loaded theme + a fallback marker so callers know whether the
/// requested theme actually existed (and thus whether the slice
/// painter should warn about missing custom themes).
#[derive(Debug, Clone)]
pub struct ActiveTheme {
    pub requested: ThemeName,
    pub theme: Theme,
    /// `true` when we couldn't load `requested` and fell back to the
    /// default. Lets the editor surface a warning to the user.
    pub fell_back: bool,
}

impl ActiveTheme {
    /// Resolve a name to an `ActiveTheme`. Always returns a usable
    /// theme — falls back to the default (`juhradial-mx`) if the
    /// requested theme can't be loaded, then to the *first* bundled
    /// theme as a last resort if even the default is missing
    /// (defensive — should be impossible since juhradial-mx is in
    /// the bundle).
    pub fn resolve(requested: &ThemeName) -> Self {
        if let Some(theme) = Theme::load(requested) {
            return ActiveTheme {
                requested: requested.clone(),
                theme,
                fell_back: false,
            };
        }
        warn!(
            "theme '{}' not found, falling back to default",
            requested.as_str()
        );
        let default = ThemeName::default();
        if let Some(theme) = Theme::load(&default) {
            return ActiveTheme {
                requested: requested.clone(),
                theme,
                fell_back: true,
            };
        }
        // Should not happen — every bundled theme parses (verified by
        // juhradial-shared tests). If it does, decode the first
        // bundled JSON directly so we still have *something*.
        let (_, first) = juhradial_shared::theme::BUNDLED_THEME_JSON[0];
        let theme: Theme = serde_json::from_str(first)
            .expect("first bundled theme must parse — verified by tests");
        ActiveTheme {
            requested: requested.clone(),
            theme,
            fell_back: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_theme_falls_back() {
        let active = ActiveTheme::resolve(&ThemeName::Custom("does-not-exist".into()));
        assert!(active.fell_back);
        assert_eq!(active.theme.name, "JuhRadial MX");
    }

    #[test]
    fn known_theme_loads_directly() {
        let active = ActiveTheme::resolve(&ThemeName::Dracula);
        assert!(!active.fell_back);
        assert_eq!(active.theme.name, "Dracula");
    }
}
