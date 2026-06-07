use iced::advanced::{layout, renderer, Clipboard, Layout, Shell, Widget};
use iced::advanced::widget::tree::{self, Tree};
use iced::{Element, Length, Rectangle, Size, Event, mouse, Color, Background, Border, Vector};
use iced::widget::canvas;

/// The appearance of an Avatar.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub background: Background,
    pub text_color: Color,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            background: Color::from_rgb(0.8, 0.8, 0.8).into(),
            text_color: Color::BLACK,
        }
    }
}

/// A widget that displays an avatar (initials in a circle).
pub struct Avatar<'a, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Theme: 'a,
    Renderer: iced::advanced::text::Renderer,
{
    initials: String,
    size: f32,
    style: Box<dyn Fn(&Theme) -> Style + 'a>,
    _phantom: std::marker::PhantomData<Renderer>,
}

impl<'a, Theme, Renderer> Avatar<'a, Theme, Renderer>
where
    Theme: 'a,
    Renderer: iced::advanced::text::Renderer,
{
    /// Creates a new [`Avatar`] displaying the given initials.
    pub fn new(initials: impl Into<String>) -> Self {
        Self {
            initials: initials.into(),
            size: 48.0,
            style: Box::new(|_| Style::default()),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Sets the size of the [`Avatar`].
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Sets the style of the [`Avatar`].
    pub fn style(mut self, style: impl Fn(&Theme) -> Style + 'a) -> Self {
        self.style = Box::new(style);
        self
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Avatar<'a, Theme, Renderer>
where
    Renderer: iced::advanced::text::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.size),
            height: Length::Fixed(self.size),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.size, self.size)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let custom_style = (self.style)(theme);

        // Draw circle background
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: (self.size / 2.0).into(),
                },
                shadow: Default::default(),
            },
            custom_style.background,
        );

        // Draw text
        let text_size = self.size * 0.4;
        renderer.fill_text(
            iced::advanced::text::Text {
                content: &self.initials,
                bounds: Size::new(bounds.width, bounds.height),
                size: iced::Pixels(text_size),
                line_height: iced::widget::text::LineHeight::default(),
                font: iced::Font::default(),
                horizontal_alignment: iced::alignment::Horizontal::Center,
                vertical_alignment: iced::alignment::Vertical::Center,
                shaping: iced::advanced::text::Shaping::Basic,
            },
            bounds.center(),
            custom_style.text_color,
            bounds,
        );
    }
}

impl<'a, Message, Theme, Renderer> From<Avatar<'a, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::text::Renderer + 'a,
{
    fn from(avatar: Avatar<'a, Theme, Renderer>) -> Self {
        Self::new(avatar)
    }
}
