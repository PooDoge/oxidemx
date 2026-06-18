//! Scheduled-tasks view (header clock button): one row per
//! `oxidemx-task-*` systemd user timer with its schedule, next
//! run, enable switch, and run/delete actions.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length};

use super::Kit;
use crate::app::Message;
use crate::radial::RadialState;

pub fn view<'a>(state: &'a RadialState, kit: &Kit) -> Element<'a, Message> {
    let kit = *kit;

    let mut rows = column![].spacing(8);
    if state.ai_tasks.is_empty() {
        rows = rows.push(
            text("No scheduled tasks yet — ask the assistant to schedule something.")
                .size(12)
                .color(kit.fade(kit.subtext0, 1.0)),
        );
    }
    for t in &state.ai_tasks {
        let sub = match &t.next_run {
            Some(n) => format!("{} · next run {n}", t.schedule),
            None => t.schedule.clone(),
        };
        let unit_toggle = t.unit.clone();
        let unit_run = t.unit.clone();
        let unit_del = t.unit.clone();
        let enabled = t.enabled;

        let small_btn = |label: &'static str, msg: Message| {
            button(text(label).size(10.5).color(kit.fade(kit.subtext1, 1.0)))
                .padding([3, 9])
                .style(move |_, _status| button::Style {
                    border: iced::border::Border {
                        color: kit.fade(kit.surface2, 1.0),
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                })
                .on_press(msg)
        };

        rows = rows.push(
            container(
                row![
                    column![
                        text(t.name.clone())
                            .size(12.5)
                            .color(kit.fade(kit.text, 1.0)),
                        text(sub).size(11).color(kit.fade(kit.subtext0, 1.0)),
                        row![
                            small_btn("Run now", Message::AiTaskRun(unit_run)),
                            small_btn("Delete", Message::AiTaskDelete(unit_del)),
                        ]
                        .spacing(6),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                    toggle(kit, enabled, Message::AiTaskToggle(unit_toggle, !enabled)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(iced::Padding {
                top: 9.0,
                right: 12.0,
                bottom: 9.0,
                left: 12.0,
            })
            .style(super::catalog::surface_style(kit, super::Surface::Row)),
        );
    }

    column![
        text("Scheduled tasks")
            .size(12)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..Default::default()
            })
            .color(kit.fade(kit.text, 1.0)),
        scrollable(container(rows).padding(iced::Padding::default().right(12.0)))
            .style(kit.scrollable_style())
            .height(Length::Fill),
        Space::new().height(Length::Fixed(0.0)),
    ]
    .spacing(8)
    .into()
}

fn toggle<'a>(kit: Kit, on: bool, msg: Message) -> Element<'a, Message> {
    super::widgets::switch(kit, on, msg)
}
