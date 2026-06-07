use iced::{Element, Length, Pixels};
use iced::widget::{self, container, column, row, text, space, scrollable};
use std::borrow::Cow;

/// Creates a new `Dialog` widget.
pub fn dialog<'a, Message>() -> Dialog<'a, Message> {
    Dialog::new()
}

/// A dialog widget that provides a standard layout for titles, content, and action buttons.
pub struct Dialog<'a, Message> {
    title: Option<Cow<'a, str>>,
    icon: Option<Element<'a, Message>>,
    body: Option<Cow<'a, str>>,
    controls: Vec<Element<'a, Message>>,
    primary_action: Option<Element<'a, Message>>,
    secondary_action: Option<Element<'a, Message>>,
    tertiary_action: Option<Element<'a, Message>>,
    width: Option<Length>,
    height: Option<Length>,
    max_width: Option<Pixels>,
    max_height: Option<Pixels>,
}

impl<Message> Default for Dialog<'_, Message> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, Message> Dialog<'a, Message> {
    /// Creates an empty `Dialog`.
    pub fn new() -> Self {
        Self {
            title: None,
            icon: None,
            body: None,
            controls: Vec::new(),
            primary_action: None,
            secondary_action: None,
            tertiary_action: None,
            width: None,
            height: None,
            max_width: None,
            max_height: None,
        }
    }

    /// Sets the title of the dialog.
    pub fn title(mut self, title: impl Into<Cow<'a, str>>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the icon of the dialog.
    pub fn icon(mut self, icon: impl Into<Element<'a, Message>>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Sets the body text of the dialog.
    pub fn body(mut self, body: impl Into<Cow<'a, str>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Adds a control widget to the dialog.
    pub fn control(mut self, control: impl Into<Element<'a, Message>>) -> Self {
        self.controls.push(control.into());
        self
    }

    /// Sets the primary action button.
    pub fn primary_action(mut self, button: impl Into<Element<'a, Message>>) -> Self {
        self.primary_action = Some(button.into());
        self
    }

    /// Sets the secondary action button.
    pub fn secondary_action(mut self, button: impl Into<Element<'a, Message>>) -> Self {
        self.secondary_action = Some(button.into());
        self
    }

    /// Sets the tertiary action button.
    pub fn tertiary_action(mut self, button: impl Into<Element<'a, Message>>) -> Self {
        self.tertiary_action = Some(button.into());
        self
    }

    /// Sets the width of the dialog.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = Some(width.into());
        self
    }

    /// Sets the height of the dialog.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = Some(height.into());
        self
    }

    /// Sets the max height of the dialog.
    pub fn max_height(mut self, max_height: impl Into<Pixels>) -> Self {
        self.max_height = Some(max_height.into());
        self
    }

    /// Sets the max width of the dialog.
    pub fn max_width(mut self, max_width: impl Into<Pixels>) -> Self {
        self.max_width = Some(max_width.into());
        self
    }
}

impl<'a, Message: Clone + 'static> From<Dialog<'a, Message>> for Element<'a, Message> {
    fn from(dialog: Dialog<'a, Message>) -> Self {
        let space_l = 24.0;
        let space_m = 16.0;
        let space_s = 12.0;
        let space_xxs = 4.0;

        let mut content_col = widget::Column::with_capacity(3 + dialog.controls.len() * 2);

        let mut should_space = false;

        if let Some(title) = dialog.title {
            content_col = content_col.push(text(title).size(24.0));
            should_space = true;
        }
        if let Some(body) = dialog.body {
            if should_space {
                content_col = content_col
                    .push(space::vertical().height(Length::Fixed(space_xxs)));
            }
            content_col = content_col.push(
                container(scrollable(text(body))).max_height(300.0),
            );
            should_space = true;
        }
        for control in dialog.controls {
            if should_space {
                content_col = content_col
                    .push(space::vertical().height(Length::Fixed(space_s)));
            }
            content_col = content_col.push(control);
            should_space = true;
        }

        let mut content_row = widget::Row::with_capacity(2).spacing(space_s);
        if let Some(icon) = dialog.icon {
            content_row = content_row.push(icon);
        }
        content_row = content_row.push(content_col);

        let mut button_row = widget::Row::with_capacity(4).spacing(space_xxs);
        if let Some(button) = dialog.tertiary_action {
            button_row = button_row.push(button);
        }
        button_row = button_row.push(space::horizontal().width(Length::Fill));
        if let Some(button) = dialog.secondary_action {
            button_row = button_row.push(button);
        }
        if let Some(button) = dialog.primary_action {
            button_row = button_row.push(button);
        }

        let mut dialog_container = container(
            widget::Column::with_children(vec![content_row.into(), button_row.into()]).spacing(space_l),
        )
        .padding(space_m)
        .width(dialog.width.unwrap_or(Length::Fixed(570.0)));

        if let Some(height) = dialog.height {
            dialog_container = dialog_container.height(height);
        }

        if let Some(max_width) = dialog.max_width {
            dialog_container = dialog_container.max_width(max_width);
        }

        if let Some(max_height) = dialog.max_height {
            dialog_container = dialog_container.max_height(max_height);
        }

        // We return an Element. The user should wrap it or style it appropriately using their theme context.
        Element::from(dialog_container)
    }
}
