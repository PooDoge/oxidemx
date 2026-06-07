use iced::{Color, Theme, theme};
use std::collections::HashMap;

pub fn parse_gtk_theme(css: &str) -> HashMap<String, Color> {
    let mut map = HashMap::new();
    for line in css.lines() {
        let line = line.trim();
        if line.starts_with("@define-color") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let name = parts[1];
                let value_str = parts[2].trim_end_matches(';');
                if let Some(color) = parse_color(value_str) {
                    map.insert(name.to_string(), color);
                }
            }
        }
    }
    map
}

fn parse_color(s: &str) -> Option<Color> {
    if let Ok(c) = csscolorparser::parse(s) {
        Some(Color::from_rgba(c.r as f32, c.g as f32, c.b as f32, c.a as f32))
    } else {
        None
    }
}

use iced::theme::palette::{Extended, Background, Primary, Secondary, Success, Warning, Danger, Pair};

pub fn create_theme(colors: &HashMap<String, Color>) -> Theme {
    let background_base = colors.get("window_bg_color").copied()
        .or_else(|| colors.get("theme_bg_color").copied())
        .unwrap_or(Color::from_rgb(0.9, 0.9, 0.9));
        
    let background_text = colors.get("window_fg_color").copied()
        .or_else(|| colors.get("theme_fg_color").copied())
        .unwrap_or(Color::from_rgb(0.1, 0.1, 0.1));

    let primary_base = colors.get("theme_selected_bg_color").copied()
        .unwrap_or(Color::from_rgb(0.2, 0.5, 0.8));
        
    let primary_text = colors.get("theme_selected_fg_color").copied()
        .unwrap_or(Color::WHITE);

    let success_base = colors.get("success_bg_color").copied()
        .or_else(|| colors.get("success_color").copied())
        .unwrap_or(Color::from_rgb(0.2, 0.8, 0.2));
        
    let success_text = colors.get("success_fg_color").copied()
        .unwrap_or(Color::WHITE);

    let warning_base = colors.get("warning_bg_color").copied()
        .or_else(|| colors.get("warning_color").copied())
        .unwrap_or(Color::from_rgb(0.8, 0.8, 0.2));
        
    let warning_text = colors.get("warning_fg_color").copied()
        .unwrap_or(Color::from_rgba(0.0, 0.0, 0.0, 0.8));

    let danger_base = colors.get("error_bg_color").copied()
        .or_else(|| colors.get("error_color").copied())
        .unwrap_or(Color::from_rgb(0.8, 0.2, 0.2));
        
    let danger_text = colors.get("error_fg_color").copied()
        .unwrap_or(Color::WHITE);

    let palette = theme::Palette {
        background: background_base,
        primary: primary_base,
        text: background_text,
        success: success_base,
        warning: warning_base,
        danger: danger_base,
    };

    Theme::custom_with_fn(
        "GtkTheme".to_string(),
        palette,
        move |p| {
            let mut ext = Extended::generate(p);
            // Override the generated text colors with explicit GTK fg colors!
            ext.primary.base.text = primary_text;
            ext.success.base.text = success_text;
            ext.warning.base.text = warning_text;
            ext.danger.base.text = danger_text;
            
            // Map some other GTK backgrounds into the weak/strong variants
            if let Some(&view_bg) = colors.get("view_bg_color") {
                ext.background.weak.color = view_bg;
            }
            if let Some(&header_bg) = colors.get("headerbar_bg_color") {
                ext.background.strong.color = header_bg;
            }
            
            ext
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_color() {
        let css = "
            @define-color theme_bg_color #ffffff;
            @define-color theme_fg_color #000;
            @define-color some_other #ff0000ff;
        ";
        let colors = parse_gtk_theme(css);
        assert_eq!(colors.get("theme_bg_color"), Some(&Color::from_rgb(1.0, 1.0, 1.0)));
        assert_eq!(colors.get("theme_fg_color"), Some(&Color::from_rgb(0.0, 0.0, 0.0)));
        assert_eq!(colors.get("some_other"), Some(&Color::from_rgba(1.0, 0.0, 0.0, 1.0)));
    }
}
