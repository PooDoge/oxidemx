use iced::advanced::{layout, renderer, Clipboard, Layout, Shell, Widget};
use iced::advanced::widget::tree::{self, Tree};
use iced::widget::canvas;
use iced::{Element, Length, Rectangle, Size, Event, mouse, Color, Vector};
use std::f32::consts::PI;
use std::time::Instant;

/// The appearance of a Spinner.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub track_color: Color,
    pub bar_color: Color,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            track_color: Color::from_rgb(0.9, 0.9, 0.9),
            bar_color: Color::from_rgb(0.2, 0.5, 0.8),
        }
    }
}

/// A spinner widget that continuously rotates.
pub struct Spinner<'a, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Theme: 'a,
    Renderer: iced::advanced::graphics::geometry::Renderer,
{
    size: f32,
    bar_height: f32,
    style: Box<dyn Fn(&Theme) -> Style + 'a>,
    _phantom: std::marker::PhantomData<Renderer>,
}

impl<'a, Theme, Renderer> Spinner<'a, Theme, Renderer>
where
    Theme: 'a,
    Renderer: iced::advanced::graphics::geometry::Renderer,
{
    /// Creates a new [`Spinner`].
    pub fn new() -> Self {
        Self {
            size: 40.0,
            bar_height: 4.0,
            style: Box::new(|_| Style::default()),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Sets the size of the [`Spinner`].
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Sets the bar height of the [`Spinner`].
    pub fn bar_height(mut self, height: f32) -> Self {
        self.bar_height = height;
        self
    }

    /// Sets the style of the [`Spinner`].
    pub fn style(mut self, style: impl Fn(&Theme) -> Style + 'a) -> Self {
        self.style = Box::new(style);
        self
    }
}

#[derive(Clone, Copy)]
struct State {
    start: Instant,
}

impl Default for State {
    fn default() -> Self {
        Self { start: Instant::now() }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Spinner<'a, Theme, Renderer>
where
    Renderer: iced::advanced::graphics::geometry::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

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

    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Event::Window(iced::window::Event::RedrawRequested(_)) = event {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let custom_style = (self.style)(theme);

        let elapsed = state.start.elapsed().as_secs_f32();
        let rotation = elapsed * 2.0 * PI; // 1 full rotation per second

        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let center = frame.center();
        let radius = self.size / 2.0 - self.bar_height / 2.0;

        // Draw track
        let track_path = canvas::Path::circle(center, radius);
        frame.stroke(
            &track_path,
            canvas::Stroke::default()
                .with_color(custom_style.track_color)
                .with_width(self.bar_height),
        );

        // Draw spinning arc
        let mut builder = canvas::path::Builder::new();
        builder.arc(canvas::path::Arc {
            center,
            radius,
            start_angle: iced::Radians(rotation),
            end_angle: iced::Radians(rotation + PI / 2.0), // 90 degree arc
        });

        frame.stroke(
            &builder.build(),
            canvas::Stroke::default()
                .with_color(custom_style.bar_color)
                .with_width(self.bar_height)
                .with_line_cap(canvas::LineCap::Round),
        );

        let geometry = frame.into_geometry();

        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            renderer.draw_geometry(geometry);
        });
    }
}

impl<'a, Message, Theme, Renderer> From<Spinner<'a, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::graphics::geometry::Renderer + 'a,
{
    fn from(spinner: Spinner<'a, Theme, Renderer>) -> Self {
        Self::new(spinner)
    }
}

pub fn spinner<'a, Theme, Renderer>() -> Spinner<'a, Theme, Renderer>
where
    Theme: 'a,
    Renderer: iced::advanced::graphics::geometry::Renderer,
{
    Spinner::new()
}

