//! Agent-feature cards in the conversation: Command executed
//! (green), Task scheduled (accent), Memory saved (mauve). All
//! left-accent-ruled with a header row (icon / title / mono meta)
//! and action chips, per `ai-chat.jsx`.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

use super::Kit;
use crate::ai_client::AgentCardData;
use crate::app::Message;

pub fn view<'a>(card: &'a AgentCardData, kit: &Kit) -> Element<'a, Message> {
    let kit = *kit;
    let (tone, glyph, title, meta) = match card {
        AgentCardData::Command {
            command, exit_code, ..
        } => {
            let head = command.split_whitespace().next().unwrap_or("sh");
            (
                kit.green,
                "$",
                "Command executed",
                format!("{head} · exit {exit_code}"),
            )
        }
        AgentCardData::Task { .. } => (
            kit.accent,
            "🕓",
            "Task scheduled",
            "systemd user timer".into(),
        ),
        AgentCardData::Memory { retention, .. } => (
            kit.mauve,
            "✱",
            "Memory saved",
            format!("retention: {retention}"),
        ),
        AgentCardData::Flow {
            flow_id,
            success,
            steps,
            ..
        } => {
            let done = steps.iter().filter(|s| s.status == "done").count();
            (
                if *success { kit.accent } else { kit.red },
                "⛓",
                if *success { "Flow completed" } else { "Flow run" },
                format!("{flow_id} · {done}/{} steps", steps.len()),
            )
        }
    };

    let header = row![
        text(glyph).size(15).color(kit.fade(tone, 1.0)),
        text(title)
            .size(11.5)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..Default::default()
            })
            .color(kit.fade(kit.text, 1.0)),
        Space::new().width(Length::Fill),
        text(meta)
            .size(10.5)
            .font(iced::Font::MONOSPACE)
            .color(kit.fade(kit.subtext0, 1.0)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let chip = move |label: String, msg: Message| {
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

    let (body, chips): (Element<'_, Message>, Vec<Element<'_, Message>>) = match card {
        AgentCardData::Command {
            command, stdout, ..
        } => {
            let mono = format!("$ {command}\n{stdout}");
            (
                text(mono)
                    .size(11)
                    .font(iced::Font::MONOSPACE)
                    .color(kit.fade(kit.subtext1, 1.0))
                    .into(),
                vec![],
            )
        }
        AgentCardData::Task {
            name,
            unit,
            schedule,
            next_run,
            enabled,
        } => {
            let sub = match next_run {
                Some(n) => format!("{schedule} · next run {n}"),
                None => schedule.clone(),
            };
            let switch = mini_switch(kit, *enabled, Message::AiTaskToggle(unit.clone(), !enabled));
            let body = row![
                column![
                    text(name).size(12.5).color(kit.fade(kit.text, 1.0)),
                    text(sub).size(11).color(kit.fade(kit.subtext0, 1.0)),
                ]
                .spacing(2),
                Space::new().width(Length::Fill),
                switch,
            ]
            .align_y(Alignment::Center)
            .into();
            let chips = vec![
                chip("Edit".into(), Message::AiTaskEdit(name.clone())).into(),
                chip("Run now".into(), Message::AiTaskRun(unit.clone())).into(),
            ];
            (body, chips)
        }
        AgentCardData::Memory { id, text: mem, .. } => {
            let body = text(format!("“{mem}”"))
                .size(12.5)
                .font(iced::Font {
                    style: iced::font::Style::Italic,
                    ..Default::default()
                })
                .color(kit.fade(kit.subtext1, 1.0))
                .into();
            let chips = vec![
                chip("View all".into(), Message::AiToggleMemories).into(),
                chip("Forget".into(), Message::AiMemoryDelete(id.clone())).into(),
            ];
            (body, chips)
        }
        AgentCardData::Flow {
            flow_id,
            steps,
            artifacts,
            ..
        } => {
            let mut col = column![].spacing(2);
            for s in steps {
                let (g, c) = match s.status.as_str() {
                    "done" => ("●", kit.green),
                    "running" => ("◐", kit.accent),
                    "failed" => ("✗", kit.red),
                    "skipped" => ("⊘", kit.overlay0),
                    _ => ("○", kit.overlay0),
                };
                col = col.push(
                    row![
                        text(g).size(11).color(kit.fade(c, 1.0)),
                        text(s.step.clone()).size(11.5).color(kit.fade(kit.subtext1, 1.0)),
                    ]
                    .spacing(7)
                    .align_y(Alignment::Center),
                );
            }
            if !artifacts.is_empty() {
                col = col.push(
                    text(format!("artifacts: {}", artifacts.join(", ")))
                        .size(10)
                        .font(iced::Font::MONOSPACE)
                        .color(kit.fade(kit.subtext0, 1.0)),
                );
            }
            let chips = vec![chip(
                "Watch in Mission Control".into(),
                Message::AiWatchFlow(flow_id.clone()),
            )
            .into()];
            (col.into(), chips)
        }
    };

    let mut inner = column![
        container(header).padding(iced::Padding {
            top: 8.0,
            right: 12.0,
            bottom: 8.0,
            left: 12.0,
        }),
        super::body::hairline(kit, kit.surface0),
        container(body).padding(iced::Padding {
            top: 9.0,
            right: 12.0,
            bottom: 9.0,
            left: 12.0,
        }),
    ];
    if !chips.is_empty() {
        inner = inner.push(
            container(iced::widget::Row::with_children(chips).spacing(6)).padding(iced::Padding {
                top: 0.0,
                right: 12.0,
                bottom: 9.0,
                left: 12.0,
            }),
        );
    }

    // Left accent rule: a 2.5 px tone bar beside the card body
    // (iced borders are uniform, so the rule is its own element).
    let rule = container(Space::new())
        .width(Length::Fixed(2.5))
        .height(Length::Fill)
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(kit.fade(tone, 1.0))),
            ..Default::default()
        });

    let framed = container(row![rule, inner]).style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(kit.fade(kit.mantle, 0.96))),
        border: iced::border::Border {
            color: kit.fade(kit.surface1, 1.0),
            width: 1.0,
            radius: 12.0.into(),
        },
        ..Default::default()
    });

    // 92% width, left-aligned like an assistant message.
    row![
        container(framed).width(Length::FillPortion(92)),
        Space::new().width(Length::FillPortion(8)),
    ]
    .into()
}

/// Small pill switch (Task card enable toggle).
fn mini_switch<'a>(kit: Kit, on: bool, msg: Message) -> Element<'a, Message> {
    let knob = container(Space::new())
        .width(Length::Fixed(14.0))
        .height(Length::Fixed(14.0))
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(
                kit.fade(if on { kit.crust } else { kit.subtext0 }, 1.0),
            )),
            border: iced::border::Border {
                radius: 7.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
    let inner = if on {
        row![Space::new().width(Length::Fill), knob]
    } else {
        row![knob, Space::new().width(Length::Fill)]
    };
    button(
        container(inner.align_y(Alignment::Center))
            .width(Length::Fixed(32.0))
            .height(Length::Fixed(18.0))
            .padding(2)
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(
                    kit.fade(if on { kit.accent } else { kit.surface2 }, 1.0),
                )),
                border: iced::border::Border {
                    radius: 9.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
    )
    .padding(0)
    .style(|_, _| button::Style::default())
    .on_press(msg)
    .into()
}
