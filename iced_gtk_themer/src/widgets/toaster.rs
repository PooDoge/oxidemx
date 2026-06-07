// Copyright 2024 wiiznokes
// SPDX-License-Identifier: MPL-2.0

//! A widget that displays toasts.

use std::collections::VecDeque;
use std::rc::Rc;

use iced::Task;
use iced::widget::{container, row, text, button, column};
use iced::Element;
use slotmap::{SlotMap, new_key_type};

use iced::{Limits, Size, Length, Point, Rectangle, Vector};

use iced::event::Event;
use iced::advanced::renderer;
use iced::advanced::widget::Operation;
use iced::advanced::widget::tree::Tree;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout,
    mouse, overlay, overlay::Overlay
};

/// Create a new Toaster widget.
///
/// A Toaster widget manages and displays a queue of [`Toast`] notifications as an overlay
/// on top of the main application `content`. The toasts appear at the bottom center of the
/// content area and are stacked vertically.
///
/// # Usage
/// To use the `toaster`, you must maintain a [`Toasts`] state in your application's state.
/// This state handles the lifecycle and queueing of the toast notifications.
///
/// ```rust,no_run
/// use iced_gtk_themer::widgets::toaster::{Toasts, Toast, toaster};
/// use iced::widget::text;
///
/// #[derive(Clone)]
/// pub enum Message {
///     CloseToast(iced_gtk_themer::widgets::toaster::ToastId),
///     ShowToast,
/// }
///
/// struct State {
///     toasts: Toasts<Message>,
/// }
///
/// impl State {
///     fn new() -> Self {
///         Self {
///             toasts: Toasts::new(Message::CloseToast),
///         }
///     }
///
///     fn view(&self) -> iced::Element<Message> {
///         toaster(
///             &self.toasts,
///             text("Main Content..."),
///         )
///     }
/// }
/// ```
pub fn toaster<'a, Message: Clone + 'static>(
    toasts: &'a Toasts<Message>,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let space_xxxs = 4.0;
    let space_xxs = 8.0;
    let space_s = 12.0;
    let space_m = 16.0;

    let make_toast = move |(id, toast): (ToastId, &'a Toast<Message>)| {
        let mut r = row![text(&toast.message)];

        let mut actions = row![].spacing(space_xxs).align_y(iced::Alignment::Center);

        if let Some(action) = &toast.action {
            actions = actions.push(button(text(&action.description)).on_press((action.message)(id)));
        }

        actions = actions.push(button(text("×")).on_press((toasts.on_close)(id)));

        r = r.push(actions)
            .align_y(iced::Alignment::Center)
            .spacing(space_s);

        container(r)
            .padding([space_xxs, space_s, space_xxs, space_m])
    };

    let col = toasts
        .queue
        .iter()
        .filter_map(|id| Some((*id, toasts.toasts.get(*id)?)))
        .rev()
        .map(make_toast)
        .fold(column![].spacing(space_xxxs), |col, toast| col.push(toast));

    Toaster::new(col.into(), content.into(), toasts.toasts.is_empty()).into()
}

/// Duration for the [`Toast`]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Duration {
    #[default]
    Short,
    Long,
    Custom(std::time::Duration),
}

impl Duration {
    fn duration(&self) -> std::time::Duration {
        match self {
            Duration::Short => std::time::Duration::from_millis(5000),
            Duration::Long => std::time::Duration::from_millis(15000),
            Duration::Custom(duration) => *duration,
        }
    }
}

impl From<std::time::Duration> for Duration {
    fn from(value: std::time::Duration) -> Self {
        Self::Custom(value)
    }
}

/// Action that can be triggered by the user.
///
/// Example: `undo`
#[derive(Clone)]
pub struct Action<Message> {
    pub description: String,
    pub message: Rc<dyn Fn(ToastId) -> Message>,
}

impl<Message> std::fmt::Debug for Action<Message> {
    #[cold]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Action")
            .field("description", &self.description)
            .finish()
    }
}

/// Represent the data used to display a [`Toast`]
#[derive(Debug, Clone)]
pub struct Toast<Message> {
    message: String,
    action: Option<Action<Message>>,
    duration: Duration,
}

impl<Message> Toast<Message> {
    /// Construct a new [`Toast`] with the provided message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            action: None,
            duration: Duration::default(),
        }
    }

    /// Set the [`Action`] of this [`Toast`]
    #[must_use]
    pub fn action(
        mut self,
        description: String,
        message: impl Fn(ToastId) -> Message + 'static,
    ) -> Self {
        self.action.replace(Action {
            description,
            message: Rc::new(message),
        });
        self
    }

    /// Set the [`Duration`] of this [`Toast`]
    #[must_use]
    pub fn duration(mut self, duration: impl Into<Duration>) -> Self {
        self.duration = duration.into();
        self
    }
}

new_key_type! { pub struct ToastId; }

#[derive(Debug, Clone)]
pub struct Toasts<Message> {
    toasts: SlotMap<ToastId, Toast<Message>>,
    queue: VecDeque<ToastId>,
    on_close: fn(ToastId) -> Message,
    limit: usize,
}

impl<Message: Clone + Send + 'static> Toasts<Message> {
    pub fn new(on_close: fn(ToastId) -> Message) -> Self {
        let limit = 5;
        Self {
            toasts: SlotMap::with_capacity_and_key(limit),
            queue: VecDeque::new(),
            on_close,
            limit,
        }
    }

    /// Add a new [`Toast`]
    pub fn push(&mut self, toast: Toast<Message>) -> Task<Message> {
        while self.toasts.len() >= self.limit {
            self.toasts.remove(
                self.queue
                    .pop_front()
                    .expect("Queue must contain all toast ids"),
            );
        }

        let duration = toast.duration.duration();

        let id = self.toasts.insert(toast);
        self.queue.push_back(id);

        let on_close = self.on_close;
        Task::perform(
            async move {
                tokio::time::sleep(duration).await;
                id
            },
            on_close
        )
    }

    /// Remove a [`Toast`]
    pub fn remove(&mut self, id: ToastId) {
        self.toasts.remove(id);
        if let Some(pos) = self.queue.iter().position(|key| *key == id) {
            self.queue.remove(pos);
        }
    }
}

pub struct Toaster<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    toasts: Element<'a, Message, Theme, Renderer>,
    content: Element<'a, Message, Theme, Renderer>,
    is_empty: bool,
}

impl<'a, Message, Theme, Renderer> Toaster<'a, Message, Theme, Renderer> {
    pub fn new(
        toasts: Element<'a, Message, Theme, Renderer>,
        content: Element<'a, Message, Theme, Renderer>,
        is_empty: bool,
    ) -> Self {
        Self {
            toasts,
            content,
            is_empty,
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Toaster<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content), Tree::new(&self.toasts)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut [&mut self.content, &mut self.toasts]);
    }

    fn operate<'b>(
        &'b mut self,
        state: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation<()>,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut state.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        state: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut state.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn mouse_interaction(
        &self,
        state: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &state.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        if self.is_empty {
            self.content.as_widget_mut().overlay(
                &mut state.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        } else {
            Some(overlay::Element::new(Box::new(ToasterOverlay::new(
                &mut state.children[1],
                &mut self.toasts,
            ))))
        }
    }

    fn drag_destinations(
        &self,
        state: &Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        dnd_rectangles: &mut iced::advanced::clipboard::DndDestinationRectangles,
    ) {
        self.content.as_widget().drag_destinations(
            &state.children[0],
            layout,
            renderer,
            dnd_rectangles,
        );
    }
}

struct ToasterOverlay<'a, 'b, Message, Theme, Renderer> {
    state: &'b mut Tree,
    element: &'b mut Element<'a, Message, Theme, Renderer>,
}

impl<'a, 'b, Message, Theme, Renderer> ToasterOverlay<'a, 'b, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn new(state: &'b mut Tree, element: &'b mut Element<'a, Message, Theme, Renderer>) -> Self {
        Self { state, element }
    }
}

impl<Message, Theme, Renderer> Overlay<Message, Theme, Renderer>
    for ToasterOverlay<'_, '_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let limits = Limits::new(Size::ZERO, bounds);

        let mut node = self
            .element
            .as_widget_mut()
            .layout(self.state, renderer, &limits);

        let offset = 15.;

        let position = Point::new(
            (bounds.width / 2.) - (node.size().width / 2.),
            bounds.height - (node.size().height + offset),
        );

        node.move_to_mut(position);
        node
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let bounds = layout.bounds();
        self.element
            .as_widget()
            .draw(self.state, renderer, theme, style, layout, cursor, &bounds);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<Message>,
    ) {
        self.element.as_widget_mut().update(
            self.state,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &layout.bounds(),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.element.as_widget().mouse_interaction(
            self.state,
            layout,
            cursor,
            &layout.bounds(),
            renderer,
        )
    }

    fn overlay<'c>(
        &'c mut self,
        layout: Layout<'c>,
        renderer: &Renderer,
    ) -> Option<overlay::Element<'c, Message, Theme, Renderer>> {
        self.element.as_widget_mut().overlay(
            self.state,
            layout,
            renderer,
            &layout.bounds(),
            Default::default(),
        )
    }
}

impl<'a, Message, Theme, Renderer> From<Toaster<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + 'a,
    Theme: 'a,
    Message: 'a,
{
    fn from(
        toaster: Toaster<'a, Message, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(toaster)
    }
}
