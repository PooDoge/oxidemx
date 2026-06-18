//! Slash-command palette: type `/` in the prompt to fuzzy-pick a
//! built-in action, a flow to run, or an enabled skill. Rendered as a
//! floating card above the input (see `mod.rs` view), driven by the
//! editor text (`mod.rs`/`update.rs`).

use iced::widget::{button, column, container, row, text, Space};
use iced::{Element, Length};

use super::Kit;
use crate::app::Message;
use crate::radial::RadialState;

/// What a palette row does when chosen.
#[derive(Clone, Debug)]
pub enum PaletteKind {
    NewChat,
    CommandCenter,
    Agents,
    Mcp,
    Memories,
    Tasks,
    Skills,
    ModelToggle,
    /// Run a flow — carries the flow's display name (the prompt is
    /// "Run the <name> flow").
    Flow(String),
    /// Toggle a skill's enabled state — carries the skill name.
    Skill(String),
    /// Run a prompt-template command — carries the file path + the
    /// argument string (text typed after the command name).
    Command(std::path::PathBuf, String),
}

/// One palette row.
#[derive(Clone, Debug)]
pub struct PaletteItem {
    pub icon: &'static str,
    pub label: String,
    pub hint: String,
    pub kind: PaletteKind,
}

impl PaletteItem {
    fn new(
        icon: &'static str,
        label: impl Into<String>,
        hint: impl Into<String>,
        kind: PaletteKind,
    ) -> Self {
        Self {
            icon,
            label: label.into(),
            hint: hint.into(),
            kind,
        }
    }
}

/// Build the (filtered) command list from the query, the user's flows
/// (`(id, name)`), their enabled skills, and their prompt-template
/// commands (`(name, description, path)`). For commands the first query
/// word selects the command and the rest is passed as its arguments.
pub fn build(
    query: &str,
    flows: &[(String, String)],
    skills: &[String],
    commands: &[(String, String, std::path::PathBuf)],
) -> Vec<PaletteItem> {
    let q = query.trim();
    let q_lower = q.to_lowercase();
    let first = q.split_whitespace().next().unwrap_or("").to_lowercase();
    let args = q
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest.trim().to_string())
        .unwrap_or_default();

    let mut items = vec![
        PaletteItem::new(
            "＋",
            "New chat",
            "Start a fresh conversation",
            PaletteKind::NewChat,
        ),
        PaletteItem::new(
            "▦",
            "Command Center",
            "Open Mission Control",
            PaletteKind::CommandCenter,
        ),
        PaletteItem::new(
            "⚙",
            "Configure agents & skills",
            "Agents tab",
            PaletteKind::Agents,
        ),
        PaletteItem::new("✱", "Skills", "Browse & enable skills", PaletteKind::Skills),
        PaletteItem::new("🔌", "MCP servers", "Configure MCP", PaletteKind::Mcp),
        PaletteItem::new(
            "✦",
            "Memories",
            "Browse saved memories",
            PaletteKind::Memories,
        ),
        PaletteItem::new("◔", "Tasks", "Scheduled flows", PaletteKind::Tasks),
        PaletteItem::new(
            "⇄",
            "Switch model",
            "Toggle Flash / Pro",
            PaletteKind::ModelToggle,
        ),
    ];
    for (id, name) in flows {
        items.push(PaletteItem::new(
            "▶",
            format!("Run flow: {name}"),
            id.clone(),
            PaletteKind::Flow(name.clone()),
        ));
    }
    for name in skills {
        items.push(PaletteItem::new(
            "✱",
            format!("Skill: {name}"),
            "enabled — toggle off",
            PaletteKind::Skill(name.clone()),
        ));
    }
    for (name, desc, path) in commands {
        items.push(PaletteItem::new(
            "⌘",
            format!("/{name}"),
            desc.clone(),
            PaletteKind::Command(path.clone(), args.clone()),
        ));
    }

    if q.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|it| match &it.kind {
            // Commands match on the first word (the rest is arguments).
            PaletteKind::Command(..) => {
                first.is_empty() || it.label.to_lowercase().contains(&first)
            }
            _ => {
                it.label.to_lowercase().contains(&q_lower)
                    || it.hint.to_lowercase().contains(&q_lower)
            }
        })
        .collect()
}

/// The floating palette card (already filtered items live in
/// `state.ai_palette`). `selected` highlights the active row.
pub fn overlay<'a>(state: &'a RadialState, kit: Kit) -> Element<'a, Message> {
    let (selected, items) = match &state.ai_palette {
        Some(p) => (p.0, &p.1),
        None => return Space::new().into(),
    };

    let mut list = column![].spacing(1);
    if items.is_empty() {
        list = list.push(
            text("No matching commands")
                .size(12)
                .color(kit.fade(kit.subtext0, 1.0)),
        );
    }
    for (i, it) in items.iter().enumerate() {
        let active = i == selected;
        let label = it.label.clone();
        let hint = it.hint.clone();
        let icon = it.icon;
        let rowel = row![
            text(icon).size(13).color(kit.fade(kit.accent, 1.0)),
            text(label).size(12.5).color(kit.fade(kit.text, 1.0)),
            Space::new().width(Length::Fill),
            text(hint).size(10.5).color(kit.fade(kit.subtext0, 1.0)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        list = list.push(
            button(rowel)
                .width(Length::Fill)
                .padding([6, 10])
                .style(move |_, status| {
                    let hovered = active || matches!(status, button::Status::Hovered);
                    // Spec: selected row = tone@16% fill + tone@40% ring.
                    button::Style {
                        background: hovered
                            .then(|| iced::Background::Color(kit.fade(kit.accent, 0.16))),
                        border: iced::border::Border {
                            color: if hovered {
                                kit.fade(kit.accent, 0.4)
                            } else {
                                iced::Color::TRANSPARENT
                            },
                            width: 1.0,
                            radius: 7.0.into(),
                        },
                        text_color: kit.fade(kit.text, 1.0),
                        ..Default::default()
                    }
                })
                .on_press(Message::AiPaletteSelect(i)),
        );
    }

    let card = container(column![list].spacing(2))
        .padding(5)
        .max_width(420.0)
        .style(move |_| iced::widget::container::Style {
            // Spec: sheet = mantle@97% fill, surface1 border (+ a drop
            // shadow since it floats — the one place the palette elevates).
            background: Some(iced::Background::Color(kit.fade(kit.mantle, 0.97))),
            border: iced::border::Border {
                color: kit.fade(kit.surface1, 1.0),
                width: 1.0,
                radius: 10.0.into(),
            },
            shadow: iced::Shadow {
                color: kit.fade(kit.crust, 0.5),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            },
            ..Default::default()
        });

    // Float it above the input: fill the chat, align bottom-left, pad up
    // past the footer.
    container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::Alignment::Start)
        .align_y(iced::Alignment::End)
        .padding(iced::Padding {
            left: crate::chat_shell::EDGE_PAD + 12.0,
            right: crate::chat_shell::EDGE_PAD + 12.0,
            bottom: crate::chat_shell::EDGE_PAD + crate::chat_shell::FOOTER_H,
            top: 0.0,
        })
        .into()
}
