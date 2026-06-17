//! Footer: activity line (streaming status dot + "Esc to stop"),
//! the multi-line input, and the accent send button — laid over
//! the painted footer arc.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

use super::icons::icon;
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

    let palette_open = state.ai_palette.is_some();
    let editor = iced::widget::text_editor(&state.ai_editor)
        .placeholder("Ask, describe an automation, or type / for commands…")
        .size(13)
        .padding(10)
        .height(Length::Fixed(44.0))
        .on_action(Message::AiEditorAction)
        .key_binding(move |key_press| {
            use iced::keyboard::key::Named;
            use iced::keyboard::Key;
            use iced::widget::text_editor::Binding;
            // While the slash palette is open, the arrow/Enter/Esc keys
            // drive it instead of the editor.
            if palette_open {
                match &key_press.key {
                    Key::Named(Named::ArrowUp) => {
                        return Some(Binding::Custom(Message::AiPaletteNav(-1)))
                    }
                    Key::Named(Named::ArrowDown) => {
                        return Some(Binding::Custom(Message::AiPaletteNav(1)))
                    }
                    Key::Named(Named::Enter) if !key_press.modifiers.shift() => {
                        return Some(Binding::Custom(Message::AiPaletteRun))
                    }
                    Key::Named(Named::Escape) => {
                        return Some(Binding::Custom(Message::AiPaletteClose))
                    }
                    _ => {}
                }
            }
            if matches!(key_press.key, Key::Named(Named::Enter)) && !key_press.modifiers.shift() {
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
        button(iced::widget::center(icon(
            "stop",
            16.0,
            kit.fade(kit.crust, 1.0),
        )))
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
        button(iced::widget::center(icon(
            "send",
            18.0,
            kit.fade(kit.crust, 1.0),
        )))
        .width(Length::Fixed(40.0))
        .height(Length::Fixed(40.0))
        .padding(0)
        .style(move |_, _status| button::Style {
            background: Some(iced::Background::Color(kit.fade(kit.accent, 1.0))),
            border: iced::border::Border {
                radius: super::tokens::R_CARD.into(),
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

    // Attach button (opens the native picker; drag-drop also works).
    let attach_btn = button(icon("attach", 16.0, kit.fade(kit.subtext0, 1.0)))
        .height(Length::Fixed(40.0))
        .padding([0, 8])
        .style(|_, _| button::Style::default())
        .on_press(Message::AiAttachPick);

    let mut col = column![activity];
    // Staged-attachment chip (drag-drop or picker), with a clear ✕.
    if let Some(path) = &state.ai_attachment {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let is_img = matches!(
            path.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .as_deref(),
            Some("png" | "jpg" | "jpeg" | "gif" | "webp")
        );
        let mut chip = row![].spacing(6).align_y(Alignment::Center);
        if is_img {
            // Small thumbnail preview for image attachments.
            chip = chip.push(
                iced::widget::image(iced::widget::image::Handle::from_path(path))
                    .width(Length::Fixed(28.0))
                    .height(Length::Fixed(28.0)),
            );
        } else {
            chip = chip.push(icon("doc", 16.0, kit.fade(kit.subtext0, 1.0)));
        }
        chip = chip.push(text(name).size(11).color(kit.fade(kit.text, 1.0)));
        chip = chip.push(
            button(icon("close", 11.0, kit.fade(kit.subtext0, 1.0)))
                .padding([0, 4])
                .style(|_, _| button::Style::default())
                .on_press(Message::AiAttachClear),
        );
        col = col.push(container(chip).padding([3, 8]).style(move |_| {
            iced::widget::container::Style {
                background: Some(iced::Background::Color(kit.fade(kit.surface0, 0.9))),
                border: iced::border::Border {
                    color: kit.fade(kit.surface2, 1.0),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }
        }));
    }
    col = col.push(
        row![editor, attach_btn, action_btn]
            .spacing(8)
            .align_y(Alignment::End),
    );

    container(col.spacing(6))
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
