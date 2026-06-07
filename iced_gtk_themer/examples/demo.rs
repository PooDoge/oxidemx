use iced::widget::{button, column, row, text, container, mouse_area, Space, scrollable, text_input, slider, toggler, checkbox, radio, pick_list, progress_bar};
use iced::{Alignment, Element, Length, window, Size, Task, Event};
use iced_gtk_themer::prelude::*;
use std::time::Instant;

pub fn main() -> iced::Result {
    iced::application(Demo::default, Demo::update, Demo::view)
        .window(window::Settings {
            decorations: false,
            transparent: true,
            size: Size::new(600.0, 400.0),
            min_size: Some(Size::new(550.0, 350.0)),
            ..Default::default()
        })
        .theme(Demo::theme)
        .style(|_state: &Demo, theme: &iced::Theme| iced::theme::Style {
            background_color: iced::Color::TRANSPARENT,
            text_color: theme.palette().text,
            icon_color: theme.palette().text,
        })
        .subscription(Demo::subscription)
        .run()
}

struct Demo {
    gtk_theme: Option<GtkTheme>,
    is_focused: bool,
    is_maximized: bool,
    last_header_click: Option<Instant>,
    button_layout: iced_gtk_themer::header_bar::ButtonLayout,
    active_tab: usize,
    active_carousel_page: usize,
    banner_visible: bool,
    slider_value: f32,
    toggled: bool,
    checkbox_val: bool,
    text_val: String,
    selected_option: Option<String>,
    system_themes: Vec<String>,
    selected_theme: Option<String>,
    segmented_selection: usize,
    tabs: Vec<iced_gtk_themer::view_switcher::ViewSwitcherItem<usize>>,
    nav_tabs: Vec<iced_gtk_themer::nav_bar::NavBarItem<usize>>,
}

#[derive(Debug, Clone)]
enum Message {
    CloseWindow,
    MaximizeWindow,
    MinimizeWindow,
    HeaderClicked,
    Resize(window::Direction),
    WindowFocused,
    WindowUnfocused,
    TabSelected(usize),
    SelectNavTab(usize),
    SelectCarouselPage(usize),
    DismissBanner,
    SliderChanged(f32),
    Toggled(bool),
    CheckboxToggled(bool),
    TextChanged(String),
    OptionSelected(String),
    ThemeChanged(String),
    SegmentSelected(usize),
}

impl Default for Demo {
    fn default() -> Self {
        let themes = GtkTheme::system_themes();
        let default_theme = if themes.contains(&"adw-gtk3-dark".to_string()) {
            "adw-gtk3-dark".to_string()
        } else if themes.contains(&"adw-gtk3".to_string()) {
            "adw-gtk3".to_string()
        } else {
            themes.first().cloned().unwrap_or_default()
        };

        Self {
            gtk_theme: GtkTheme::load(&default_theme),
            is_focused: true,
            is_maximized: false,
            last_header_click: None,
            button_layout: iced_gtk_themer::header_bar::get_system_button_layout(),
            active_tab: 0,
            slider_value: 50.0,
            toggled: true,
            checkbox_val: true,
            text_val: String::new(),
            selected_option: None,
            system_themes: themes,
            selected_theme: Some(default_theme),
            segmented_selection: 0,
            tabs: vec![
                iced_gtk_themer::view_switcher::ViewSwitcherItem::new(0, "Basic", "⚙"),
                iced_gtk_themer::view_switcher::ViewSwitcherItem::new(1, "Inputs", "📝"),
                iced_gtk_themer::view_switcher::ViewSwitcherItem::new(2, "Indicators", "◴"),
                iced_gtk_themer::view_switcher::ViewSwitcherItem::new(3, "Settings", "🖽"),
            ],
            nav_tabs: vec![
                iced_gtk_themer::nav_bar::NavBarItem::new(0, "Basic").with_icon("⚙"),
                iced_gtk_themer::nav_bar::NavBarItem::new(1, "Inputs").with_icon("📝"),
                iced_gtk_themer::nav_bar::NavBarItem::new(2, "Indicators").with_icon("◴"),
                iced_gtk_themer::nav_bar::NavBarItem::new(3, "Settings").with_icon("🖽"),
            ],
            active_carousel_page: 0,
            banner_visible: true,
        }
    }
}

impl Demo {
    fn subscription(&self) -> iced::Subscription<Message> {
        iced::event::listen_with(|event, _status, _id| match event {
            Event::Window(window::Event::Focused) => Some(Message::WindowFocused),
            Event::Window(window::Event::Unfocused) => Some(Message::WindowUnfocused),
            _ => None,
        })
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CloseWindow => {
                std::process::exit(0);
            }
            Message::MaximizeWindow => {
                self.is_maximized = !self.is_maximized;
                window::oldest().and_then(|id| window::toggle_maximize(id))
            }
            Message::MinimizeWindow => {
                window::oldest().and_then(|id| window::minimize(id, true))
            }
            Message::HeaderClicked => {
                let now = Instant::now();
                let is_double = self.last_header_click.map_or(false, |last| now.duration_since(last).as_millis() < 500);
                self.last_header_click = Some(now);
                if is_double {
                    self.is_maximized = !self.is_maximized;
                    return window::oldest().and_then(|id| window::toggle_maximize(id));
                } else {
                    return window::oldest().and_then(window::drag);
                }
            }
            Message::Resize(dir) => {
                window::oldest().and_then(move |id| window::drag_resize(id, dir))
            }
            Message::WindowFocused => {
                self.is_focused = true;
                Task::none()
            }
            Message::WindowUnfocused => {
                self.is_focused = false;
                Task::none()
            }
            Message::TabSelected(idx) => {
                self.active_tab = idx;
                Task::none()
            }
            Message::SelectNavTab(i) => {
                self.active_tab = i;
                Task::none()
            }
            Message::SelectCarouselPage(i) => {
                self.active_carousel_page = i;
                Task::none()
            }
            Message::DismissBanner => {
                self.banner_visible = false;
                Task::none()
            }
            Message::SliderChanged(val) => {
                self.slider_value = val;
                Task::none()
            }
            Message::Toggled(b) => {
                self.toggled = b;
                Task::none()
            }
            Message::CheckboxToggled(b) => {
                self.checkbox_val = b;
                Task::none()
            }
            Message::TextChanged(t) => {
                self.text_val = t;
                Task::none()
            }
            Message::OptionSelected(o) => {
                self.selected_option = Some(o);
                Task::none()
            }
            Message::ThemeChanged(t) => {
                self.gtk_theme = GtkTheme::load(&t);
                self.selected_theme = Some(t);
                Task::none()
            }
            Message::SegmentSelected(idx) => {
                self.segmented_selection = idx;
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<Message> {
        if let Some(gtk) = &self.gtk_theme {
            let header = HeaderBar::view(
                "GTK Widget Showcase",
                &gtk.assets,
                &self.button_layout,
                Message::CloseWindow,
                Message::MaximizeWindow,
                Message::MinimizeWindow,
                Message::HeaderClicked,
            );

            let tabs = view_switcher(
                gtk,
                &self.tabs,
                self.active_tab,
                Message::TabSelected,
                iced_gtk_themer::view_switcher::ViewSwitcherPolicy::Wide,
            );

            let tab_content: Element<Message> = match self.active_tab {
                0 => column![
                    button("Primary Button").primary(gtk),
                    button("Secondary Button").secondary(gtk),
                    button("Destructive Button").destructive(gtk),
                    button("Success Button").primary(gtk),
                    text("A simple label"),
                ].spacing(20).into(),
                1 => column![
                    text_input("Enter text...", &self.text_val).on_input(Message::TextChanged).gtk_style(gtk),
                    checkbox(self.checkbox_val).label("Check me out").on_toggle(Message::CheckboxToggled).gtk_style(gtk),
                    radio("Option A", "A", self.selected_option.as_deref(), |s| Message::OptionSelected(s.to_string())).gtk_style(gtk),
                    radio("Option B", "B", self.selected_option.as_deref(), |s| Message::OptionSelected(s.to_string())).gtk_style(gtk),
                    pick_list(vec!["Item 1".to_string(), "Item 2".to_string()], self.selected_option.clone(), Message::OptionSelected).gtk_style(gtk),
                ].spacing(20).into(),
                2 => {
                    let banner_widget: Element<Message> = if self.banner_visible {
                        banner(text("Welcome to the Indicators tab! This is a banner.")).into()
                    } else {
                        iced::widget::space().into()
                    };

                    let spinner_widget = spinner();
                    let avatar_widget = avatar("User Avatar");

                    let pages = vec![
                        container(text("Carousel Page 1").size(24)).center_x(Length::Fill).center_y(Length::Fill).into(),
                        container(text("Carousel Page 2").size(24)).center_x(Length::Fill).center_y(Length::Fill).into(),
                        container(text("Carousel Page 3").size(24)).center_x(Length::Fill).center_y(Length::Fill).into(),
                    ];
                    let carousel_widget = carousel(gtk, pages, self.active_carousel_page, Message::SelectCarouselPage);

                    column![
                        banner_widget,
                        row![
                            column![text("Spinner"), spinner_widget].spacing(10).align_x(Alignment::Center),
                            column![text("Avatar"), avatar_widget].spacing(10).align_x(Alignment::Center),
                        ].spacing(40),
                        text("Carousel Component").size(18),
                        carousel_widget
                    ].spacing(20).into()
                },
                3 => {
                    let appearance_group = preferences_group(
                        gtk,
                        Some("Appearance"),
                        vec![
                            action_row(
                                "Dark Mode",
                                Some("Enable darker colors across the app"),
                                Some(toggler(self.toggled).on_toggle(Message::Toggled).into()),
                            ),
                            action_row(
                                "Theme",
                                Some("Select the active GTK theme"),
                                Some(pick_list(self.system_themes.clone(), self.selected_theme.clone(), Message::ThemeChanged).gtk_style(gtk).into()),
                            ),
                        ],
                    );

                    let controls_group = preferences_group(
                        gtk,
                        Some("Controls"),
                        vec![
                            action_row(
                                "Layout Style",
                                Some("Segmented button demo"),
                                Some(segmented_button(
                                    gtk,
                                    vec![
                                        ("Left".into(), Message::SegmentSelected(0)),
                                        ("Center".into(), Message::SegmentSelected(1)),
                                        ("Right".into(), Message::SegmentSelected(2)),
                                    ],
                                    self.segmented_selection,
                                ).into()),
                            ),
                            spin_row(
                                gtk,
                                "Volume",
                                Some("Adjust the system volume"),
                                self.slider_value as f64,
                                0.0,
                                100.0,
                                1.0,
                                |v| Message::SliderChanged(v as f32),
                            ),
                        ],
                    );
                    
                    let advanced_group = boxed_list(gtk)
                        .push(action_row("Boxed Item 1", None, None::<Element<Message>>))
                        .push(action_row("Boxed Item 2", None, None::<Element<Message>>))
                        .push(button_row(gtk, "Click Me!", None, Message::HeaderClicked, None));

                    clamp(
                        column![appearance_group, controls_group, advanced_group].spacing(24),
                        500.0,
                    ).into()
                },
                _ => iced::widget::space().into(),
            };

            let nav = nav_bar(gtk, &self.nav_tabs, self.active_tab, Message::TabSelected);

            let content = column![
                header,
                row![
                    nav,
                    container(column![tabs, scrollable(tab_content).height(Length::Fill)].spacing(20).padding(20))
                        .card(gtk)
                        .width(Length::Fill)
                        .height(Length::Fill)
                ].height(Length::Fill)
            ];

            let main_window = container(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |theme: &iced::Theme| {
                    let palette = theme.palette();
                    let mut style = container::background(palette.background);
                    if !self.is_maximized {
                        style.border = iced::Border {
                            color: iced::Color::TRANSPARENT,
                            width: 0.0,
                            radius: 12.0.into(),
                        };
                        if self.is_focused {
                            style.shadow = iced::Shadow {
                                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                                offset: iced::Vector::new(0.0, 8.0),
                                blur_radius: 20.0,
                            };
                        } else {
                            style.shadow = iced::Shadow {
                                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.2),
                                offset: iced::Vector::new(0.0, 4.0),
                                blur_radius: 10.0,
                            };
                        }
                    }
                    style
                });

            let resizer = |dir: window::Direction, w: Length, h: Length| {
                mouse_area(iced::widget::space().width(w).height(h))
                    .interaction(match dir {
                        window::Direction::North | window::Direction::South => iced::mouse::Interaction::ResizingVertically,
                        window::Direction::East | window::Direction::West => iced::mouse::Interaction::ResizingHorizontally,
                        window::Direction::NorthWest | window::Direction::SouthEast => iced::mouse::Interaction::ResizingDiagonallyUp,
                        window::Direction::NorthEast | window::Direction::SouthWest => iced::mouse::Interaction::ResizingDiagonallyDown,
                    })
                    .on_press(Message::Resize(dir))
            };

            let outer_layout = column![
                row![resizer(window::Direction::NorthWest, Length::Fixed(5.0), Length::Fixed(5.0)), resizer(window::Direction::North, Length::Fill, Length::Fixed(5.0)), resizer(window::Direction::NorthEast, Length::Fixed(5.0), Length::Fixed(5.0))],
                row![resizer(window::Direction::West, Length::Fixed(5.0), Length::Fill), main_window, resizer(window::Direction::East, Length::Fixed(5.0), Length::Fill)],
                row![resizer(window::Direction::SouthWest, Length::Fixed(5.0), Length::Fixed(5.0)), resizer(window::Direction::South, Length::Fill, Length::Fixed(5.0)), resizer(window::Direction::SouthEast, Length::Fixed(5.0), Length::Fixed(5.0))],
            ];

            if self.is_maximized {
                container(outer_layout)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            } else {
                container(outer_layout)
                    .padding(20)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
        } else {
            container(text("No GTK theme found!"))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        }
    }

    fn theme(&self) -> iced::Theme {
        if let Some(gtk) = &self.gtk_theme {
            gtk.theme.clone()
        } else {
            iced::Theme::Light
        }
    }
}
