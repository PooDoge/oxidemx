//! Memories management view (header brain button): search field,
//! count + size, memory rows (pin toggle / text / scope tag /
//! retention / age / delete), retention footnote.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length};

use super::Kit;
use crate::app::Message;
use crate::radial::RadialState;

pub fn view<'a>(state: &'a RadialState, kit: &Kit) -> Element<'a, Message> {
    let kit = *kit;

    let query = state.ai_memories_query.trim().to_lowercase();
    let visible: Vec<&crate::agent::memory::MemoryEntry> = state
        .ai_memories
        .iter()
        .filter(|m| {
            query.is_empty()
                || m.text.to_lowercase().contains(&query)
                || m.scope.to_lowercase().contains(&query)
        })
        .collect();

    let size_kb = (state.ai_memories_bytes as f64 / 1024.0).ceil() as u64;
    let header = row![
        text_input("Search memories…", &state.ai_memories_query)
            .size(12)
            .padding(7)
            .on_input(Message::AiMemorySearch)
            .style(move |theme, status| {
                let mut s = iced::widget::text_input::default(theme, status);
                s.background = iced::Background::Color(kit.fade(kit.crust, 1.0));
                s.border.color = kit.fade(kit.surface2, 1.0);
                s.border.radius = 10.0.into();
                s.value = kit.fade(kit.text, 1.0);
                s.placeholder = kit.fade(kit.subtext0, 1.0);
                s
            })
            .width(Length::Fill),
        text(format!("{} stored · {size_kb} KB", state.ai_memories.len()))
            .size(10.5)
            .color(kit.fade(kit.subtext0, 1.0)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut rows = column![].spacing(8);
    if visible.is_empty() {
        rows = rows.push(
            text(if state.ai_memories.is_empty() {
                "No memories yet — ask the assistant to remember something."
            } else {
                "No memories match the search."
            })
            .size(12)
            .color(kit.fade(kit.subtext0, 1.0)),
        );
    }
    for m in visible {
        let id_pin = m.id.clone();
        let pinned = m.pinned;
        let pin_btn = button(super::icons::icon(
            "pin",
            13.0,
            kit.fade(if pinned { kit.yellow } else { kit.overlay0 }, 1.0),
        ))
        .padding([2, 4])
        .style(|_, _| button::Style::default())
        .on_press(Message::AiMemoryPin(id_pin, !pinned));

        let retention = if m.pinned {
            "Until changed".to_string()
        } else {
            "Auto · 90d".to_string()
        };
        let meta = row![
            text(m.scope.clone())
                .size(10)
                .color(kit.fade(kit.accent, 1.0)),
            text(retention).size(10).color(kit.fade(kit.subtext0, 1.0)),
            text(format!("· {}", crate::app::rel_time(m.created_at)))
                .size(10)
                .color(kit.fade(kit.subtext0, 1.0)),
        ]
        .spacing(8);

        let del = button(super::icons::icon(
            "trash",
            14.0,
            kit.fade(kit.subtext0, 1.0),
        ))
        .padding([2, 4])
        .style(|_, _| button::Style::default())
        .on_press(Message::AiMemoryDelete(m.id.clone()));

        rows = rows.push(
            container(
                row![
                    pin_btn,
                    column![
                        text(m.text.clone())
                            .size(12.5)
                            .color(kit.fade(kit.text, 1.0)),
                        meta,
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                    del,
                ]
                .spacing(10)
                .align_y(Alignment::Start),
            )
            .padding(iced::Padding {
                top: 9.0,
                right: 12.0,
                bottom: 9.0,
                left: 12.0,
            })
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(kit.fade(kit.mantle, 0.96))),
                border: iced::border::Border {
                    color: kit.fade(kit.surface1, 1.0),
                    width: 1.0,
                    radius: 10.0.into(),
                },
                ..Default::default()
            }),
        );
    }

    let footnote = text(
        "Auto-retention: unpinned memories expire after 90 days unused. \
         Pinned memories persist until you change or delete them.",
    )
    .size(10.5)
    .color(kit.fade(kit.subtext0, 1.0));

    column![
        header,
        scrollable(container(rows).padding(iced::Padding::default().right(12.0)))
            .style(kit.scrollable_style())
            .height(Length::Fill),
        container(footnote).padding(iced::Padding {
            top: 4.0,
            right: 2.0,
            bottom: 6.0,
            left: 2.0,
        }),
        Space::new().height(Length::Fixed(0.0)),
    ]
    .spacing(8)
    .into()
}
