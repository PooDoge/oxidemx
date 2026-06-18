//! Skills management view (header ✱ button / palette): search, the
//! discovered-skill rows (enable toggle / name / source badge /
//! description), and a footnote. Skills are discovered natively from
//! the Claude + Antigravity roots (see `agent::skills`); enabled skills
//! form the candidate pool the agent draws on.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length};

use super::Kit;
use crate::app::Message;
use crate::radial::RadialState;

pub fn view<'a>(state: &'a RadialState, kit: &Kit) -> Element<'a, Message> {
    let kit = *kit;

    let query = state.ai_skills_query.trim().to_lowercase();
    let visible: Vec<&crate::agent::skills::Skill> = state
        .ai_skills
        .iter()
        .filter(|s| {
            query.is_empty()
                || s.name.to_lowercase().contains(&query)
                || s.description.to_lowercase().contains(&query)
        })
        .collect();

    let enabled_n = state.ai_skills_enabled.len();
    let header = row![
        text_input("Search skills…", &state.ai_skills_query)
            .size(12)
            .padding(7)
            .on_input(Message::AiSkillsSearch)
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
        text(format!("{} found · {enabled_n} on", state.ai_skills.len()))
            .size(10.5)
            .color(kit.fade(kit.subtext0, 1.0)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut rows = column![].spacing(8);
    if visible.is_empty() {
        rows = rows.push(
            text(if state.ai_skills.is_empty() {
                "No skills found. Drop SKILL.md folders in ~/.claude/skills or \
                 ~/.gemini/antigravity/skills (or add a root in skills.json)."
            } else {
                "No skills match the search."
            })
            .size(12)
            .color(kit.fade(kit.subtext0, 1.0)),
        );
    }
    for s in visible {
        let on = state.ai_skills_enabled.contains(&s.name);
        let name = s.name.clone();
        let toggle = button(
            text(if on { "● on" } else { "○ off" })
                .size(11)
                .color(kit.fade(if on { kit.green } else { kit.overlay0 }, 1.0)),
        )
        .padding([3, 8])
        .style(move |_, _| button::Style {
            background: Some(iced::Background::Color(kit.fade(
                if on { kit.green } else { kit.surface1 },
                if on { 0.22 } else { 0.6 },
            ))),
            border: iced::border::Border::default().rounded(8.0),
            text_color: kit.fade(kit.text, 1.0),
            ..Default::default()
        })
        .on_press(Message::AiSkillEnable(name, !on));

        let desc = if s.description.is_empty() {
            "(no description)".to_string()
        } else {
            s.description.clone()
        };
        let meta = row![text(s.source).size(10).color(kit.fade(kit.accent, 1.0)),].spacing(8);

        rows = rows.push(
            container(
                row![
                    column![
                        row![
                            text(s.name.clone())
                                .size(12.5)
                                .color(kit.fade(kit.text, 1.0)),
                            Space::new().width(Length::Fixed(8.0)),
                            meta,
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                        text(desc).size(11).color(kit.fade(kit.subtext0, 1.0)),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                    toggle,
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
            .style(super::catalog::surface_style(kit, super::Surface::Row)),
        );
    }

    let footnote = text(
        "Enabled skills become the agent's candidate pool — it loads a skill's full \
         instructions on demand when relevant. Sources: Claude (~/.claude/skills), \
         Antigravity (~/.gemini/antigravity/skills), project ./.claude/skills.",
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
    ]
    .spacing(8)
    .into()
}
