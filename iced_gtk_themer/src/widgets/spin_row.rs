use iced::{Element, Length, Alignment, Padding, Theme};
use iced::widget::{row, column, text, container, button, text_input, component, Component};
use crate::GtkTheme;

pub struct SpinRowState {
    input_text: Option<String>,
}

impl Default for SpinRowState {
    fn default() -> Self {
        Self { input_text: None }
    }
}

pub struct SpinRow<'a, Message> {
    gtk: &'a GtkTheme,
    title: String,
    subtitle: Option<String>,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    on_change: Box<dyn Fn(f64) -> Message + 'a>,
}

#[derive(Clone)]
pub enum Event {
    Increase,
    Decrease,
    InputChanged(String),
    InputSubmit,
}

impl<'a, Message: Clone + 'a> Component<Message, iced::Theme, iced::Renderer> for SpinRow<'a, Message> {
    type State = SpinRowState;
    type Event = Event;

    fn update(&mut self, state: &mut Self::State, event: Self::Event) -> Option<Message> {
        match event {
            Event::Increase => {
                state.input_text = None;
                let new_val = self.value + self.step;
                if new_val <= self.max {
                    return Some((self.on_change)(new_val));
                }
            }
            Event::Decrease => {
                state.input_text = None;
                let new_val = self.value - self.step;
                if new_val >= self.min {
                    return Some((self.on_change)(new_val));
                }
            }
            Event::InputChanged(s) => {
                // Only allow valid float characters
                let is_valid = s.is_empty() || s == "-" || s == "." || s == "-." || s.parse::<f64>().is_ok();
                
                if is_valid {
                    state.input_text = Some(s.clone());
                    // Live update if valid
                    if let Ok(v) = s.parse::<f64>() {
                        if v >= self.min && v <= self.max {
                            return Some((self.on_change)(v));
                        }
                    }
                }
            }
            Event::InputSubmit => {
                if let Some(s) = &state.input_text {
                    if let Ok(v) = s.parse::<f64>() {
                        if v >= self.min && v <= self.max {
                            state.input_text = None;
                            return Some((self.on_change)(v));
                        }
                    }
                }
                // Invalid or out of bounds, reset the text
                state.input_text = None;
            }
        }
        None
    }

    fn view(&self, state: &Self::State) -> Element<Self::Event, iced::Theme, iced::Renderer> {
        let mut text_col = column![text(&self.title).size(16)].spacing(4);

        if let Some(sub) = &self.subtitle {
            text_col = text_col.push(
                text(sub).size(13).style(move |theme: &Theme| {
                    let mut c = theme.palette().text;
                    c.a = 0.7; // subtle subtitle
                    iced::widget::text::Style { color: Some(c), ..Default::default() }
                })
            );
        }

        let decrease_val = self.value - self.step;
        let increase_val = self.value + self.step;

        let mut btn_minus = button(text("-").width(Length::Fill).align_x(iced::alignment::Horizontal::Center))
            .width(Length::Fixed(32.0))
            .padding(Padding::from([8, 0]));
            
        if decrease_val >= self.min {
            btn_minus = btn_minus.on_press(Event::Decrease);
        }

        btn_minus = btn_minus.style(move |theme: &Theme, status| {
            let fg = theme.palette().text;
            match status {
                button::Status::Hovered | button::Status::Pressed => button::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(0.5, 0.5, 0.5, 0.15))),
                    text_color: fg,
                    border: iced::Border { radius: [6.0, 0.0, 0.0, 6.0].into(), ..Default::default() },
                    ..Default::default()
                },
                button::Status::Disabled => button::Style {
                    text_color: iced::Color { a: 0.3, ..fg },
                    ..Default::default()
                },
                _ => button::Style {
                    text_color: fg,
                    ..Default::default()
                }
            }
        });

        let val_str = state.input_text.clone().unwrap_or_else(|| format!("{}", self.value));
        
        let val_input = text_input("", &val_str)
            .on_input(Event::InputChanged)
            .on_submit(Event::InputSubmit)
            .align_x(iced::alignment::Horizontal::Center)
            .width(Length::Fixed(48.0))
            .padding(Padding::from([8, 0]))
            .style(move |theme: &Theme, _status| {
                let fg = theme.palette().text;
                iced::widget::text_input::Style {
                    background: iced::Background::Color(iced::Color::TRANSPARENT),
                    border: iced::Border { width: 0.0, ..Default::default() },
                    icon: fg,
                    placeholder: iced::Color { a: 0.5, ..fg },
                    value: fg,
                    selection: theme.palette().primary,
                }
            });

        let mut btn_plus = button(text("+").width(Length::Fill).align_x(iced::alignment::Horizontal::Center))
            .width(Length::Fixed(32.0))
            .padding(Padding::from([8, 0]));

        if increase_val <= self.max {
            btn_plus = btn_plus.on_press(Event::Increase);
        }

        btn_plus = btn_plus.style(move |theme: &Theme, status| {
            let fg = theme.palette().text;
            match status {
                button::Status::Hovered | button::Status::Pressed => button::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(0.5, 0.5, 0.5, 0.15))),
                    text_color: fg,
                    border: iced::Border { radius: [0.0, 6.0, 6.0, 0.0].into(), ..Default::default() },
                    ..Default::default()
                },
                button::Status::Disabled => button::Style {
                    text_color: iced::Color { a: 0.3, ..fg },
                    ..Default::default()
                },
                _ => button::Style {
                    text_color: fg,
                    ..Default::default()
                }
            }
        });

        let spin_control_inner = row![btn_minus, val_input, btn_plus].spacing(0).align_y(Alignment::Center);

        let gtk = self.gtk;
        let spin_control = container(spin_control_inner)
            .style(move |theme: &Theme| {
                let s = gtk.button_secondary(iced::widget::button::Status::Active);
                iced::widget::container::Style {
                    background: s.background,
                    text_color: Some(s.text_color),
                    border: s.border,
                    ..Default::default()
                }
            });

        let row_content = row![
            text_col.width(Length::Fill),
            spin_control
        ]
        .align_y(Alignment::Center)
        .spacing(12)
        .padding(Padding::from(16));

        container(row_content).into()
    }
}

/// A row with a title, optional subtitle, and a spin control (numeric input).
/// Mimics AdwSpinRow from Libadwaita.
pub fn spin_row<'a, Message: Clone + 'a>(
    gtk: &'a GtkTheme,
    title: &'a str,
    subtitle: Option<&'a str>,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    on_change: impl Fn(f64) -> Message + 'a,
) -> Element<'a, Message> {
    component(SpinRow {
        gtk,
        title: title.to_string(),
        subtitle: subtitle.map(|s| s.to_string()),
        value,
        min,
        max,
        step,
        on_change: Box::new(on_change),
    })
}
