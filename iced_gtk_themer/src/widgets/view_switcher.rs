use iced::{Element, Length, Alignment, Padding};
use iced::widget::{row, column, button, container, text};
use crate::GtkTheme;
use crate::widgets::icon;

/// The policy dictating how the view switcher should lay out its buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewSwitcherPolicy {
    /// Icon and text are side-by-side
    Wide,
    /// Icon is above the text
    Narrow,
}

/// An item for the ViewSwitcher.
pub struct ViewSwitcherItem<V> {
    pub value: V,
    pub label: String,
    pub icon_name: String,
}

impl<V> ViewSwitcherItem<V> {
    pub fn new(value: V, label: impl Into<String>, icon_name: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            icon_name: icon_name.into(),
        }
    }
}

/// An adaptive view switcher.
pub fn view_switcher<'a, Message, V>(
    _theme: &'a GtkTheme,
    items: &'a [ViewSwitcherItem<V>],
    selected: V,
    on_select: impl Fn(V) -> Message + 'a,
    policy: ViewSwitcherPolicy,
) -> Element<'a, Message>
where
    V: Clone + PartialEq + 'a,
    Message: Clone + 'a,
{
    let mut row_container = row!().spacing(0).align_y(Alignment::Center);

    for item in items {
        let is_selected = item.value == selected;
        let value = item.value.clone();
        
        let content: Element<'a, Message> = match policy {
            ViewSwitcherPolicy::Wide => {
                row![
                    icon(&item.icon_name).size(16),
                    text(&item.label).size(14),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            ViewSwitcherPolicy::Narrow => {
                column![
                    icon(&item.icon_name).size(16),
                    text(&item.label).size(11),
                ]
                .spacing(4)
                .align_x(Alignment::Center)
                .into()
            }
        };

        let btn = button(
            container(content)
                .padding(match policy {
                    ViewSwitcherPolicy::Wide => Padding::new(10.0).left(16.0).right(16.0),
                    ViewSwitcherPolicy::Narrow => Padding::new(8.0).left(16.0).right(16.0),
                })
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
        )
        .style(move |_: &iced::Theme, s| {
            if is_selected {
                _theme.button_view_switcher_active(s)
            } else {
                _theme.button_view_switcher(s)
            }
        })
        .on_press(on_select(value));

        row_container = row_container.push(btn);
    }

    container(row_container)
        .padding(0)
        .align_x(Alignment::Center)
        .into()
}
