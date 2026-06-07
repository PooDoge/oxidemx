// ANCHOR: all
use iced::Element;
use iced::widget::{button, column, row, text, text_input};

// ANCHOR: state
#[derive(Debug, Clone, Default)]
pub struct NewJoke {
    setup: String,
    punchline: String,
}
// ANCHOR_END: state

// ANCHOR: message
#[derive(Debug, Clone)]
pub enum Message {
    SetupChanged(String),
    PunchlineChanged(String),
    Submit,
    Cancel,
}
// ANCHOR_END: message

// ANCHOR: action
#[derive(Debug, Clone)]
pub enum Action {
    AddJoke { setup: String, punchline: String },
    Cancel,
}
// ANCHOR_END: action

impl NewJoke {
    // ANCHOR: new
    pub fn new() -> Self {
        Self::default()
    }
    // ANCHOR_END: new

    // ANCHOR: update
    pub fn update(&mut self, message: Message) -> Option<Action> {
        match message {
            Message::SetupChanged(setup) => {
                self.setup = setup;
                None
            }
            Message::PunchlineChanged(punchline) => {
                self.punchline = punchline;
                None
            }
            Message::Submit => {
                if !self.setup.is_empty() && !self.punchline.is_empty() {
                    Some(Action::AddJoke {
                        setup: self.setup.clone(),
                        punchline: self.punchline.clone(),
                    })
                } else {
                    None
                }
            }
            Message::Cancel => Some(Action::Cancel),
        }
    }
    // ANCHOR_END: update

    // ANCHOR: view
    pub fn view(&self) -> Element<'_, Message> {
        column![
            text("Add a New Joke").size(24),
            text_input("Setup...", &self.setup)
                .on_input(Message::SetupChanged)
                .padding(10),
            text_input("Punchline...", &self.punchline)
                .on_input(Message::PunchlineChanged)
                .padding(10),
            row![
                button("Cancel").on_press(Message::Cancel),
                button("Submit").on_press(Message::Submit),
            ]
            .spacing(10)
        ]
        .spacing(15)
        .padding(20)
        .into()
    }
    // ANCHOR_END: view
}
// ANCHOR_END: all
