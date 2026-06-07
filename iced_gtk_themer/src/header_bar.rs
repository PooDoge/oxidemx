use iced::widget::{button, container, row, svg, text, Space, mouse_area};
use iced::{Alignment, Element, Length, Padding, Theme};
use iced::widget::button::Status;
use iced::{Background, Color};
use crate::asset_pipeline::ThemeAssets;

pub struct HeaderBar;

fn svg_style(theme: &Theme, _status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(theme.extended_palette().background.base.text),
    }
}

fn control_style(theme: &Theme, status: Status) -> button::Style {
    let mut style = button::text(theme, status);
    
    // Always draw a subtle circular background to match Bazzite's Adwaita
    // We derive this dynamically from the GTK text color, just like GTK CSS mix/alpha does!
    let mut base_bg = theme.extended_palette().background.base.text;
    base_bg.a = 0.05;
    
    let mut hover_bg = theme.extended_palette().background.base.text;
    hover_bg.a = 0.15;
    
    if status == Status::Hovered || status == Status::Pressed {
        style.background = Some(Background::Color(hover_bg));
    } else {
        style.background = Some(Background::Color(base_bg));
    }
    
    style.border = iced::Border {
        color: Color::TRANSPARENT,
        width: 0.0,
        radius: 16.0.into(),
    };
    
    style
}

fn close_style(theme: &Theme, status: Status) -> button::Style {
    let mut style = button::text(theme, status);
    
    if status == Status::Hovered || status == Status::Pressed {
        style.background = Some(Background::Color(theme.extended_palette().danger.base.color));
        style.text_color = theme.extended_palette().danger.base.text;
    } else {
        // Base state is a subtle circle like the others
        let mut base_bg = theme.extended_palette().background.base.text;
        base_bg.a = 0.05;
        style.background = Some(Background::Color(base_bg));
    }
    
    style.border = iced::Border {
        color: Color::TRANSPARENT,
        width: 0.0,
        radius: 16.0.into(),
    };
    
    style
}

#[derive(Debug, Clone, PartialEq)]
pub enum HeaderButton {
    Minimize,
    Maximize,
    Close,
    Menu,
}

#[derive(Debug, Clone)]
pub struct ButtonLayout {
    pub left: Vec<HeaderButton>,
    pub right: Vec<HeaderButton>,
}

impl Default for ButtonLayout {
    fn default() -> Self {
        Self {
            left: vec![],
            right: vec![HeaderButton::Minimize, HeaderButton::Maximize, HeaderButton::Close],
        }
    }
}

pub fn get_system_button_layout() -> ButtonLayout {
    use std::process::Command;
    let output = Command::new("gsettings")
        .args(&["get", "org.gnome.desktop.wm.preferences", "button-layout"])
        .output();
        
    let layout_str = match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_string()
        }
        _ => ":minimize,maximize,close".to_string(), // fallback
    };
    
    let mut parts = layout_str.split(':');
    let left_str = parts.next().unwrap_or("");
    let right_str = parts.next().unwrap_or("");
    
    let parse_side = |s: &str| -> Vec<HeaderButton> {
        s.split(',')
         .map(|s| s.trim())
         .filter_map(|s| match s {
             "minimize" => Some(HeaderButton::Minimize),
             "maximize" => Some(HeaderButton::Maximize),
             "close" => Some(HeaderButton::Close),
             "appmenu" | "menu" => Some(HeaderButton::Menu),
             _ => None,
         })
         .collect()
    };
    
    ButtonLayout {
        left: parse_side(left_str),
        right: parse_side(right_str),
    }
}

impl HeaderBar {
    pub fn view<'a, Message: Clone + 'a>(
        title: &'a str,
        assets: &'a ThemeAssets,
        layout: &'a ButtonLayout,
        on_close: Message,
        on_maximize: Message,
        on_minimize: Message,
        on_click: Message,
    ) -> Element<'a, Message> {
        let close_btn = if let Some(ref handle) = assets.close {
            button(svg(handle.clone()).width(Length::Fixed(16.0)).height(Length::Fixed(16.0)).style(svg_style))
                .on_press(on_close)
                .padding(Padding::from(4))
                .style(close_style)
        } else {
            button("X").on_press(on_close).padding(Padding::from(4)).style(close_style)
        };

        let max_btn = if let Some(ref handle) = assets.maximize {
            button(svg(handle.clone()).width(Length::Fixed(16.0)).height(Length::Fixed(16.0)).style(svg_style))
                .on_press(on_maximize)
                .padding(Padding::from(4))
                .style(control_style)
        } else {
            button("O").on_press(on_maximize).padding(Padding::from(4)).style(control_style)
        };

        let min_btn = if let Some(ref handle) = assets.minimize {
            button(svg(handle.clone()).width(Length::Fixed(16.0)).height(Length::Fixed(16.0)).style(svg_style))
                .on_press(on_minimize)
                .padding(Padding::from(4))
                .style(control_style)
        } else {
            button("_").on_press(on_minimize).padding(Padding::from(4)).style(control_style)
        };

        let mut min_btn_opt = Some(min_btn);
        let mut max_btn_opt = Some(max_btn);
        let mut close_btn_opt = Some(close_btn);

        let mut build_controls = |btns: &Vec<HeaderButton>| -> Element<'a, Message> {
            let mut row_controls = row![].spacing(4);
            for btn in btns {
                let w = match btn {
                    HeaderButton::Minimize => min_btn_opt.take(),
                    HeaderButton::Maximize => max_btn_opt.take(),
                    HeaderButton::Close => close_btn_opt.take(),
                    HeaderButton::Menu => None, // Add menu icon logic later if needed
                };
                if let Some(btn_element) = w {
                    row_controls = row_controls.push(btn_element);
                }
            }
            row_controls.into()
        };

        let left_controls = build_controls(&layout.left);
        let right_controls = build_controls(&layout.right);

        // We wrap the title in a mouse_area to catch the click for dragging/maximizing
        let drag_area = mouse_area(
            container(text(title).size(16))
                .width(Length::Fill)
                .center_x(Length::Fill)
        )
        .on_press(on_click);

        // Balance controls based on size so title stays centered
        let content = row![
            if layout.left.is_empty() { Space::new().width(Length::Fixed(72.0)).height(Length::Shrink).into() } else { left_controls },
            drag_area,
            if layout.right.is_empty() { Space::new().width(Length::Fixed(72.0)).height(Length::Shrink).into() } else { right_controls }
        ]
        .align_y(Alignment::Center)
        .padding(Padding::from(8));

        container(content)
            .width(Length::Fill)
            .height(Length::Fixed(48.0)) // GNOME HIG suggests decent size
            .into()
    }
}
