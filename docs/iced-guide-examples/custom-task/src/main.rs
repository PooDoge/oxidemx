// ANCHOR: all
use iced::widget::{button, column, text};
use iced::{Center, Element, Task};
use serde::Deserialize;

pub fn main() -> iced::Result {
    iced::application(App::default, App::update, App::view)
        .title("Custom Task Example")
        .run()
}

#[derive(Default)]
struct App {
    ip: Option<String>,
    is_loading: bool,
    error: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    FetchIp,
    IpFetched(Result<String, Error>),
}

#[derive(Debug, Clone)]
enum Error {
    Network(String),
}

impl App {
// ANCHOR: update_function
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::FetchIp => {
                self.is_loading = true;
                self.error = None;
                // ANCHOR: return_custom_task
                Task::perform(fetch_ip(), Message::IpFetched)
                // ANCHOR_END: return_custom_task
            }
            Message::IpFetched(Ok(ip)) => {
                self.is_loading = false;
                self.ip = Some(ip);
                Task::none()
            }
            Message::IpFetched(Err(Error::Network(e))) => {
                self.is_loading = false;
                self.error = Some(e);
                Task::none()
            }
        }
    }
// ANCHOR_END: update_function

    fn view(&self) -> Element<'_, Message> {
        let content = if self.is_loading {
            text("Fetching IP address...")
        } else if let Some(error) = &self.error {
            text(format!("Error: {}", error))
        } else if let Some(ip) = &self.ip {
            text(format!("Your IP is: {}", ip))
        } else {
            text("Click the button to fetch your IP address")
        };

        column![
            content,
            button("Fetch IP").on_press(Message::FetchIp)
        ]
        .padding(20)
        .spacing(20)
        .align_x(Center)
        .into()
    }
}

// ANCHOR: fetch_ip
#[derive(Deserialize)]
struct IpResponse {
    ip: String,
}

async fn fetch_ip() -> Result<String, Error> {
    let response = reqwest::get("https://api.ipify.org?format=json")
        .await
        .map_err(|e| Error::Network(e.to_string()))?;

    let ip_response: IpResponse = response
        .json()
        .await
        .map_err(|e| Error::Network(e.to_string()))?;

    Ok(ip_response.ip)
}
// ANCHOR_END: fetch_ip
// ANCHOR_END: all
