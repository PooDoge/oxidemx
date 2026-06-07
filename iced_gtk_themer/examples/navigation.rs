use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length, window, Size, Task};
use iced_gtk_themer::prelude::*;

pub fn main() -> iced::Result {
    iced::application(NavigationApp::default, NavigationApp::update, NavigationApp::view)
        .window(window::Settings {
            size: Size::new(800.0, 600.0),
            ..Default::default()
        })
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    NavSelected(usize),
    TabSelected(usize),
    HeaderClicked,
    CloseWindow,
    MaximizeWindow,
    MinimizeWindow,
}

struct NavigationApp {
    gtk: GtkTheme,
    active_nav: usize,
    active_tab: usize,
    nav_items: Vec<iced_gtk_themer::nav_bar::NavBarItem<usize>>,
    tab_items: Vec<iced_gtk_themer::view_switcher::ViewSwitcherItem<usize>>,
    button_layout: iced_gtk_themer::header_bar::ButtonLayout,
}

impl Default for NavigationApp {
    fn default() -> Self {
        Self {
            gtk: GtkTheme::load("adwaita").unwrap(),
            active_nav: 0,
            active_tab: 0,
            nav_items: vec![
                iced_gtk_themer::nav_bar::NavBarItem::new(0, "Dashboard").with_icon("◴"),
                iced_gtk_themer::nav_bar::NavBarItem::new(1, "Files").with_icon("📝"),
                iced_gtk_themer::nav_bar::NavBarItem::new(2, "Settings").with_icon("⚙"),
            ],
            tab_items: vec![
                iced_gtk_themer::view_switcher::ViewSwitcherItem::new(0, "General", "⚙"),
                iced_gtk_themer::view_switcher::ViewSwitcherItem::new(1, "Appearance", "🖽"),
            ],
            button_layout: iced_gtk_themer::header_bar::ButtonLayout::default(),
        }
    }
}

impl NavigationApp {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NavSelected(i) => self.active_nav = i,
            Message::TabSelected(i) => self.active_tab = i,
            Message::HeaderClicked => {},
            Message::CloseWindow | Message::MaximizeWindow | Message::MinimizeWindow => {},
        }
        Task::none()
    }

    fn view(&self) -> Element<Message> {
        let header = HeaderBar::view(
            "Navigation Example",
            &self.gtk.assets,
            &self.button_layout,
            Message::CloseWindow,
            Message::MaximizeWindow,
            Message::MinimizeWindow,
            Message::HeaderClicked,
        );

        let nav = nav_bar(&self.gtk, &self.nav_items, self.active_nav, Message::NavSelected);

        let content: Element<Message> = match self.active_nav {
            0 => column![text("Dashboard Content").size(24)].padding(20).into(),
            1 => column![text("Files Content").size(24)].padding(20).into(),
            2 => {
                let tabs = view_switcher(
                    &self.gtk,
                    &self.tab_items,
                    self.active_tab,
                    Message::TabSelected,
                    iced_gtk_themer::view_switcher::ViewSwitcherPolicy::Wide,
                );

                let tab_content: Element<Message> = match self.active_tab {
                    0 => text("General Settings").into(),
                    1 => text("Appearance Settings").into(),
                    _ => Space::new().width(Length::Fill).height(Length::Fill).into(),
                };

                column![
                    tabs,
                    container(tab_content).padding(20)
                ].into()
            }
            _ => Space::new().width(Length::Fill).height(Length::Fill).into(),
        };

        let body = row![
            nav,
            container(scrollable(content))
                .card(&self.gtk)
                .width(Length::Fill)
                .height(Length::Fill)
        ];

        let main_window = container(column![header, body])
            .width(Length::Fill)
            .height(Length::Fill);

        main_window.into()
    }
}

impl NavigationApp {
    fn theme(&self) -> iced::Theme {
        iced::Theme::Light
    }
}
