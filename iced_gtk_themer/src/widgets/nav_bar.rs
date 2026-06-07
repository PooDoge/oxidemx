use iced::{Element, Length, Alignment, Padding};
use iced::widget::{column, row, button, container, text, scrollable};
use crate::GtkTheme;
use crate::widgets::icon;

pub struct NavBarItem<V> {
    pub value: V,
    pub label: String,
    pub icon_name: Option<String>,
}

impl<V> NavBarItem<V> {
    pub fn new(value: V, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            icon_name: None,
        }
    }

    pub fn with_icon(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }
}

/// A vertical navigation bar component, inspired by COSMIC's NavBar.
pub fn nav_bar<'a, Message, V>(
    theme: &'a GtkTheme,
    items: &'a [NavBarItem<V>],
    selected: V,
    on_select: impl Fn(V) -> Message + 'a,
) -> Element<'a, Message>
where
    V: Clone + PartialEq + 'a,
    Message: Clone + 'a,
{
    let mut col_container = column!().spacing(4).width(Length::Fill);

    for item in items {
        let is_selected = item.value == selected;
        let value = item.value.clone();
        
        let mut content = row!().spacing(12).align_y(Alignment::Center);

        if let Some(ref icon_name) = item.icon_name {
            content = content.push(icon(icon_name).size(16));
        }

        content = content.push(text(&item.label).size(14));

        let style = if is_selected {
            crate::style::button_view_switcher_active
        } else {
            crate::style::button_view_switcher
        };

        let btn = button(
            container(content)
                .padding(Padding::new(8.0).left(12.0).right(12.0))
                .width(Length::Fill)
                .align_y(Alignment::Center)
        )
        .width(Length::Fill)
        .style(move |_: &iced::Theme, s| {
            if is_selected {
                theme.button_view_switcher_active(s)
            } else {
                theme.button_view_switcher(s)
            }
        })
        .on_press(on_select(value));

        col_container = col_container.push(btn);
    }

    let scroll = scrollable(col_container)
        .width(Length::Fill)
        .height(Length::Fill);

    container(scroll)
        .width(Length::Fixed(200.0))
        .height(Length::Fill)
        .padding(8)
        .into()
}
