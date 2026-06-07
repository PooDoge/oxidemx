use iced::widget::svg;
use std::path::{Path, PathBuf};

#[derive(Clone, Default)]
pub struct ThemeAssets {
    pub close: Option<svg::Handle>,
    pub maximize: Option<svg::Handle>,
    pub minimize: Option<svg::Handle>,
}

impl ThemeAssets {
    pub fn load(theme_dir: &Path) -> Self {
        let assets_dir = theme_dir.join("gtk-3.0").join("assets");
        let assets_dir_fallback = theme_dir.join("assets");

        let search_dirs = vec![assets_dir, assets_dir_fallback];

        let mut close = None;
        let mut maximize = None;
        let mut minimize = None;

        for dir in search_dirs {
            if close.is_none() {
                close = find_svg(&dir, "titlebutton-close");
            }
            if maximize.is_none() {
                maximize = find_svg(&dir, "titlebutton-maximize");
            }
            if minimize.is_none() {
                minimize = find_svg(&dir, "titlebutton-minimize");
            }
        }

        if close.is_none() {
            close = find_system_icon("titlebutton-close");
        }
        if maximize.is_none() {
            maximize = find_system_icon("titlebutton-maximize");
        }
        if minimize.is_none() {
            minimize = find_system_icon("titlebutton-minimize");
        }

        Self {
            close: close.map(svg::Handle::from_path),
            maximize: maximize.map(svg::Handle::from_path),
            minimize: minimize.map(svg::Handle::from_path),
        }
    }
}

fn find_svg(dir: &Path, prefix: &str) -> Option<PathBuf> {
    if !dir.exists() {
        return None;
    }

    let exact = dir.join(format!("{}.svg", prefix));
    if exact.exists() {
        return Some(exact);
    }
    
    let active = dir.join(format!("{}-active.svg", prefix));
    if active.exists() {
        return Some(active);
    }

    let symbol = dir.join(format!("{}-symbolic.svg", prefix));
    if symbol.exists() {
        return Some(symbol);
    }

    None
}

fn find_system_icon(prefix: &str) -> Option<PathBuf> {
    // Map titlebutton-* to window-*-symbolic
    let icon_name = prefix.replace("titlebutton-", "window-") + "-symbolic.svg";
    let adwaita_path = PathBuf::from("/usr/share/icons/Adwaita/symbolic/ui").join(&icon_name);
    
    if adwaita_path.exists() {
        return Some(adwaita_path);
    }
    
    None
}
