//! Footer: activity line (streaming status dot + "Esc to stop"),
//! the multi-line input, and the accent send button — laid over
//! the painted footer arc.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

use super::Kit;
use crate::app::Message;
use crate::chat_shell::{EDGE_PAD, FOOTER_H};
use crate::radial::RadialState;

pub fn view<'a>(state: &'a RadialState, kit: &Kit) -> Element<'a, Message> {
    let kit = *kit;

    // Activity line — present while a turn is in flight.
    let activity: Element<'a, Message> = if state.ai_loading {
        row![
            container(Space::new())
                .width(Length::Fixed(5.0))
                .height(Length::Fixed(5.0))
                .style(move |_| iced::widget::container::Style {
                    // Breathes with the shared status pulse so the
                    // "working" state is alive, not a static dot.
                    background: Some(iced::Background::Color(
                        kit.fade(kit.accent, 0.45 + 0.55 * kit.pulse),
                    )),
                    border: iced::border::Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            text(
                state
                    .ai_activity
                    .clone()
                    .unwrap_or_else(|| "Working…".to_string())
            )
            .size(10.5)
            .color(kit.fade(kit.subtext0, 1.0)),
            Space::new().width(Length::Fill),
            button(
                text("■ Esc to stop")
                    .size(10.5)
                    .color(kit.fade(kit.subtext0, 1.0))
            )
            .padding(0)
            .style(|_, _| button::Style::default())
            .on_press(Message::AiStopRequest),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    } else {
        Space::new().height(Length::Fixed(16.0)).into()
    };

    let editor = iced::widget::text_editor(&state.ai_editor)
        .placeholder("Ask, or describe an automation…")
        .size(13)
        .padding(10)
        .height(Length::Fixed(44.0))
        .on_action(Message::AiEditorAction)
        .key_binding(|key_press| {
            use iced::widget::text_editor::Binding;
            if matches!(
                key_press.key,
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter)
            ) && !key_press.modifiers.shift()
            {
                return Some(Binding::Custom(Message::AiSubmitPrompt));
            }
            Binding::from_key_press(key_press)
        })
        .style(move |theme, status| {
            let mut s = iced::widget::text_editor::default(theme, status);
            s.background = iced::Background::Color(kit.fade(kit.crust, 1.0));
            s.value = kit.fade(kit.text, 1.0);
            s.placeholder = kit.fade(kit.subtext0, 1.0);
            s.border.color = kit.fade(kit.surface2, 1.0);
            s.border.radius = 12.0.into();
            s
        });

    // Send flips to Stop while a turn is in flight.
    let action_btn = if state.ai_loading {
        button(iced::widget::center(
            text("■")
                .size(18)
                .line_height(1.0)
                .color(kit.fade(kit.crust, 1.0)),
        ))
        .width(Length::Fixed(40.0))
        .height(Length::Fixed(40.0))
        .padding(0)
        .style(move |_, _status| button::Style {
            background: Some(iced::Background::Color(kit.fade(kit.red, 1.0))),
            border: iced::border::Border {
                radius: 12.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .on_press(Message::AiStopRequest)
    } else {
        button(iced::widget::center(
            text("➤")
                .size(22)
                .line_height(1.0)
                .color(kit.fade(kit.crust, 1.0)),
        ))
        .width(Length::Fixed(40.0))
        .height(Length::Fixed(40.0))
        .padding(0)
        .style(move |_, _status| button::Style {
            background: Some(iced::Background::Color(kit.fade(kit.accent, 1.0))),
            border: iced::border::Border {
                radius: 12.0.into(),
                ..Default::default()
            },
            shadow: iced::Shadow {
                color: kit.fade(kit.accent, 0.33),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        })
        .on_press(Message::AiSubmitPrompt)
    };

    container(
        column![
            activity,
            row![editor, action_btn].spacing(8).align_y(Alignment::End),
        ]
        .spacing(6),
    )
    .width(Length::Fill)
    .height(Length::Fixed(EDGE_PAD + FOOTER_H))
    .padding(iced::Padding {
        top: 10.0,
        right: EDGE_PAD + 10.0,
        bottom: EDGE_PAD + 10.0,
        left: EDGE_PAD + 10.0,
    })
    .into()
}
