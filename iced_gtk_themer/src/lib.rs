pub mod parser;
pub mod asset_pipeline;
pub mod header_bar;
pub mod style;
pub mod ext;
pub use ext::*;
pub mod widgets;

pub use parser::{parse_gtk_theme, create_theme};
pub use widgets::*;
pub use asset_pipeline::ThemeAssets;
pub use header_bar::HeaderBar;

use iced::Theme;
use iced::Color;
use std::collections::HashMap;

#[derive(Clone)]
pub struct GtkTheme {
    pub theme: Theme,
    pub colors: HashMap<String, Color>,
    pub assets: ThemeAssets,
}

impl GtkTheme {
    pub fn load(theme_name: &str) -> Option<Self> {
        let theme_dirs = [
            dirs::data_dir()?.join("themes").join(theme_name),
            dirs::home_dir()?.join(".themes").join(theme_name),
            std::path::PathBuf::from("/usr/share/themes").join(theme_name),
        ];

        let mut theme_dir = None;
        for dir in theme_dirs {
            if dir.exists() {
                theme_dir = Some(dir);
                break;
            }
        }

        let theme_dir = theme_dir?;
        let gtk3_css = theme_dir.join("gtk-3.0").join("gtk.css");
        
        let css_content = if gtk3_css.exists() {
            std::fs::read_to_string(gtk3_css).ok()?
        } else {
            return None; // No css found
        };

        let colors = parse_gtk_theme(&css_content);
        let theme = create_theme(&colors);
        let assets = ThemeAssets::load(&theme_dir);

        Some(Self {
            theme,
            colors,
            assets,
        })
    }

    pub fn system_themes() -> Vec<String> {
        let mut themes = std::collections::HashSet::new();
        
        let theme_dirs = [
            dirs::data_dir().map(|d| d.join("themes")),
            dirs::home_dir().map(|d| d.join(".themes")),
            Some(std::path::PathBuf::from("/usr/share/themes")),
        ];

        for dir_opt in theme_dirs {
            if let Some(dir) = dir_opt {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            // Check if it has gtk-3.0/gtk.css
                            if entry.path().join("gtk-3.0").join("gtk.css").exists() {
                                if let Ok(name) = entry.file_name().into_string() {
                                    themes.insert(name);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        let mut themes: Vec<String> = themes.into_iter().collect();
        themes.sort_by_key(|a| a.to_lowercase());
        themes
    }

    pub fn button_primary(&self, status: iced::widget::button::Status) -> iced::widget::button::Style {
        crate::style::button_primary(&self.colors, &self.theme, status)
    }

    pub fn button_secondary(&self, status: iced::widget::button::Status) -> iced::widget::button::Style {
        crate::style::button_secondary(&self.colors, &self.theme, status)
    }

    pub fn button_destructive(&self, status: iced::widget::button::Status) -> iced::widget::button::Style {
        crate::style::button_destructive(&self.colors, &self.theme, status)
    }

    pub fn text_input(&self, status: iced::widget::text_input::Status) -> iced::widget::text_input::Style {
        crate::style::text_input_style(&self.colors, &self.theme, status)
    }

    pub fn checkbox(&self, status: iced::widget::checkbox::Status) -> iced::widget::checkbox::Style {
        crate::style::checkbox_style(&self.colors, &self.theme, status)
    }

    pub fn radio(&self, status: iced::widget::radio::Status) -> iced::widget::radio::Style {
        crate::style::radio_style(&self.colors, &self.theme, status)
    }

    pub fn slider(&self, status: iced::widget::slider::Status) -> iced::widget::slider::Style {
        crate::style::slider_style(&self.colors, &self.theme, status)
    }

    pub fn pick_list(&self, status: iced::widget::pick_list::Status) -> iced::widget::pick_list::Style {
        crate::style::pick_list_style(&self.colors, &self.theme, status)
    }

    pub fn container_card(&self) -> iced::widget::container::Style {
        crate::style::container_card(&self.colors, &self.theme)
    }
}
