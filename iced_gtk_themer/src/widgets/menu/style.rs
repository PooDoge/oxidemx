use iced::{Color, Theme};
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub struct Appearance {
    pub background: Color,
    pub border_width: f32,
    pub bar_border_radius: [f32; 4],
    pub menu_border_radius: [f32; 4],
    pub border_color: Color,
    pub background_expand: [u16; 4],
    pub path: Color,
    pub text_color: Color,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            background: Color::WHITE,
            border_width: 1.0,
            bar_border_radius: [6.0; 4],
            menu_border_radius: [6.0; 4],
            border_color: Color::from_rgb(0.5, 0.5, 0.5),
            background_expand: [0; 4],
            path: Color::from_rgb(0.2, 0.5, 0.8),
            text_color: Color::BLACK,
        }
    }
}

#[derive(Default, Clone)]
pub enum MenuBarStyle {
    #[default]
    Default,
    Custom(Arc<dyn StyleSheet<Style = Theme> + Send + Sync>),
}

pub trait StyleSheet {
    type Style: Default;
    fn appearance(&self, style: &Self::Style) -> Appearance;
}

impl StyleSheet for Theme {
    type Style = MenuBarStyle;
    fn appearance(&self, _style: &Self::Style) -> Appearance {
        Appearance::default()
    }
}
