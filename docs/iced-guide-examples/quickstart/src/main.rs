// ANCHOR: all
use iced::widget::{button, column, text};
use iced::{Alignment, Element, Task};

// ANCHOR: main
pub fn main() -> iced::Result {
    iced::application(Counter::new, Counter::update, Counter::view)
        .title("Counter")
        .run()
}
// ANCHOR_END: main

// ANCHOR: counter_struct
struct Counter {
    value: i32,
}
// ANCHOR_END: counter_struct

// ANCHOR: message_enum
#[derive(Debug, Clone, Copy)]
enum Message {
    IncrementPressed,
    DecrementPressed,
}
// ANCHOR_END: message_enum

impl Counter {
    // ANCHOR: new
    fn new() -> (Self, Task<Message>) {
        (Self { value: 0 }, Task::none())
    }
    // ANCHOR_END: new

    // ANCHOR: update
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::IncrementPressed => {
                self.value += 1;
            }
            Message::DecrementPressed => {
                self.value -= 1;
            }
        }
        Task::none()
    }
    // ANCHOR_END: update

    // ANCHOR: view
    fn view(&self) -> Element<Message> {
        column![
            button("Increment").on_press(Message::IncrementPressed),
            text(self.value).size(50),
            button("Decrement").on_press(Message::DecrementPressed)
        ]
        .padding(20)
        .align_x(Alignment::Center)
        .into()
    }
    // ANCHOR_END: view
}
// ANCHOR_END: all
