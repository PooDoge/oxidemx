use iced::{Element, Length, Size};
use iced::widget::{container, responsive, row, stack, Rule, opaque, mouse_area};
use crate::GtkTheme;

/// A split view that shows a sidebar next to content on wide screens,
/// and overlays the sidebar on narrow screens. Mimics AdwOverlaySplitView.
pub fn overlay_split_view<'a, Message: Clone + 'a>(
    sidebar: impl Fn() -> Element<'a, Message> + 'a,
    content: impl Fn() -> Element<'a, Message> + 'a,
    show_sidebar: bool,
    on_close_sidebar: Message,
) -> Element<'a, Message> {
    let sidebar_width = 300.0;
    let breakpoint = 700.0;

    responsive(move |size: Size| {
        if size.width >= breakpoint {
            // Desktop mode: side-by-side
            let split = row![
                container(sidebar()).width(Length::Fixed(sidebar_width)).height(Length::Fill),
                Rule::vertical(1),
                container(content()).width(Length::Fill).height(Length::Fill)
            ]
            .width(Length::Fill)
            .height(Length::Fill);
            
            split.into()
        } else {
            // Mobile mode: overlay
            let mut s = stack![
                container(content()).width(Length::Fill).height(Length::Fill)
            ];

            if show_sidebar {
                let dimmer = mouse_area(
                    opaque(
                        container(iced::widget::space().width(Length::Fill).height(Length::Fill))
                            .style(|_theme| {
                                let mut color = iced::Color::BLACK;
                                color.a = 0.3;
                                container::background(color)
                            })
                    )
                )
                .on_press(on_close_sidebar.clone());

                let sidebar_overlay = container(sidebar())
                    .width(Length::Fixed(sidebar_width))
                    .height(Length::Fill)
                    .style(|theme: &iced::Theme| {
                        container::background(theme.palette().background)
                    });

                s = s.push(dimmer);
                s = s.push(sidebar_overlay);
            }

            s.width(Length::Fill).height(Length::Fill).into()
        }
    })
    .into()
}
