// ANCHOR: all
use iced::{Element, Task, Theme};
use iced::widget::{button, column, text};

mod list_item;
mod new_joke;

use list_item::ListItem;

// ANCHOR: view_enum
#[derive(Debug, Clone)]
pub enum View {
    List,
    AddJoke,
}
// ANCHOR_END: view_enum

// ANCHOR: app_state
#[derive(Debug, Clone)]
pub struct AppState {
    current_view: View,
    jokes: Vec<(String, String)>,
    new_joke: new_joke::NewJoke,
}
// ANCHOR_END: app_state

#[derive(Debug, Clone)]
pub enum Message {
    GoTo(View),
    NewJokeMsg(new_joke::Message),
}

impl AppState {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                current_view: View::List,
                jokes: vec![
                    (
                        "Why do programmers prefer dark mode?".to_string(),
                        "Because light attracts bugs.".to_string(),
                    ),
                ],
                new_joke: new_joke::NewJoke::new(),
            },
            Task::none()
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::GoTo(view) => {
                self.current_view = view;
            }
            Message::NewJokeMsg(msg) => {
                if let Some(action) = self.new_joke.update(msg) {
                    match action {
                        new_joke::Action::AddJoke { setup, punchline } => {
                            self.jokes.push((setup, punchline));
                            self.new_joke = new_joke::NewJoke::new(); // reset
                            self.current_view = View::List;
                        }
                        new_joke::Action::Cancel => {
                            self.new_joke = new_joke::NewJoke::new(); // reset
                            self.current_view = View::List;
                        }
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        match self.current_view {
            View::List => {
                let mut jokes_col = column![].spacing(10);
                for (setup, punchline) in &self.jokes {
                    let formatted = format!("Q: {}\nA: {}", setup, punchline);
                    jokes_col = jokes_col.push(
                        Element::from(ListItem::from(formatted).padding(15.0))
                    );
                }

                column![
                    text("Jokes").size(30),
                    jokes_col,
                    button("Add New Joke").on_press(Message::GoTo(View::AddJoke)),
                ]
                .spacing(20)
                .padding(20)
                .into()
            }
            View::AddJoke => {
                self.new_joke
                    .view()
                    .map(Message::NewJokeMsg)
            }
        }
    }
}

pub fn main() -> iced::Result {
    iced::application(AppState::new, AppState::update, AppState::view)
        .title("Iced App Structure Example")
        .theme(|_state: &AppState| Theme::Dark)
        .run()
}
// ANCHOR_END: all
