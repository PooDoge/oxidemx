use iced::advanced::{layout, renderer, Clipboard, Layout, Shell, Widget};
use iced::advanced::widget::tree::{self, Tree};
use iced::widget::{row, column, container, text};
use iced::{Element, Length, Rectangle, Size, Event, mouse, Color, Background, Border, Padding};

/// The appearance of a Banner.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub background: Background,
    pub text_color: Color,
    pub border: Border,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            background: Color::from_rgb(0.9, 0.95, 1.0).into(),
            text_color: Color::from_rgb(0.1, 0.2, 0.4),
            border: Border {
                color: Color::from_rgb(0.2, 0.5, 0.8),
                width: 1.0,
                radius: 4.0.into(),
            },
        }
    }
}

/// A banner widget for displaying alerts or important information.
pub struct Banner<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Theme: 'a,
    Renderer: iced::advanced::Renderer + iced::advanced::text::Renderer + 'a,
{
    content: Element<'a, Message, Theme, Renderer>,
    style: Box<dyn Fn(&Theme) -> Style + 'a>,
}

impl<'a, Message, Theme, Renderer> Banner<'a, Message, Theme, Renderer>
where
    Theme: 'a,
    Renderer: iced::advanced::Renderer + iced::advanced::text::Renderer + 'a,
{
    /// Creates a new [`Banner`] wrapping the provided content.
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            style: Box::new(|_| Style::default()),
        }
    }

    /// Sets the style of the [`Banner`].
    pub fn style(mut self, style: impl Fn(&Theme) -> Style + 'a) -> Self {
        self.style = Box::new(style);
        self
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Banner<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer + iced::advanced::text::Renderer + 'a,
{
    fn children(&self) -> Vec<tree::Tree> {
        vec![tree::Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut tree::Tree) {
        tree.diff_children(&mut [&mut self.content])
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut tree::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let padding = Padding::new(12.0);
        let limits = limits.width(Length::Fill).height(Length::Shrink).shrink(padding);
        
        let mut content_node = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &limits,
        );
        
        let size = limits.resolve(Length::Fill, Length::Shrink, content_node.size());
        content_node = content_node.move_to(iced::Point::new(padding.left, padding.top));
        
        layout::Node::with_children(
            Size::new(size.width + padding.left + padding.right, size.height + padding.top + padding.bottom),
            vec![content_node],
        )
    }

    fn operate(
        &mut self,
        tree: &mut tree::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut tree::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.children().next().unwrap(),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn mouse_interaction(
        &self,
        tree: &tree::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout.children().next().unwrap(),
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &tree::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let custom_style = (self.style)(theme);

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: custom_style.border,
                shadow: Default::default(),
                snap: true,
            },
            custom_style.background,
        );

        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            &renderer::Style {
                text_color: custom_style.text_color,
                ..*_style
            },
            layout.children().next().unwrap(),
            cursor,
            viewport,
        );
    }
}

impl<'a, Message, Theme, Renderer> From<Banner<'a, Message, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + iced::advanced::text::Renderer + 'a,
{
    fn from(banner: Banner<'a, Message, Theme, Renderer>) -> Self {
        Self::new(banner)
    }
}

pub fn banner<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>
) -> Banner<'a, Message, Theme, Renderer>
where
    Theme: 'a,
    Renderer: iced::advanced::Renderer + iced::advanced::text::Renderer + 'a,
{
    Banner::new(content)
}

