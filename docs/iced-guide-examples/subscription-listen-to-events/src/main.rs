// ANCHOR: all
use iced::event::{self, Event};
use iced::keyboard;
use iced::widget::{center, text};
use iced::{Subscription, Task};

// ANCHOR: main
pub fn main() -> iced::Result {
    iced::application(State::default, update, view)
        .title("Event Listener")
        .subscription(subscription)
        .run()
}
// ANCHOR_END: main

#[derive(Default)]
struct State {
    last_key: Option<keyboard::Key>,
}

// ANCHOR: message_enum
#[derive(Debug, Clone)]
pub enum Message {
    EventOccurred(Event),
}
// ANCHOR_END: message_enum

// ANCHOR: update
fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::EventOccurred(Event::Keyboard(keyboard::Event::KeyPressed { key, .. })) => {
            state.last_key = Some(key);
            Task::none()
        }
        _ => Task::none(),
    }
}
// ANCHOR_END: update

fn view(state: &State) -> iced::Element<'_, Message> {
    let content = match &state.last_key {
        Some(key) => format!("Last key pressed: {:?}", key),
        None => String::from("Press any key..."),
    };

    center(text(content).size(40)).into()
}

fn subscription(_state: &State) -> Subscription<Message> {
    event::listen_with(|event, _status, _window| {
        match event {
            Event::Keyboard(keyboard::Event::KeyPressed { .. }) => Some(Message::EventOccurred(event)),
            _ => None,
        }
    })
}
// ANCHOR_END: all
