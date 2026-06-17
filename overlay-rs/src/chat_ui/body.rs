//! Conversation body: bubbles, agent cards, in-flight stream, the
//! pending multiple-choice question, and the full thread list view
//! (reached from the "…" chip when more threads exist than fit the
//! strip).

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Color, Element, Length};

use super::icons::icon;
use super::Kit;
use crate::app::Message;
use crate::radial::RadialState;

/// Scrollable id for the conversation history — the update loop
/// snaps it to the newest message.
pub const CHAT_SCROLL_ID: &str = "ai-chat-history";

/// 1 px horizontal divider in `color`.
pub fn hairline<'a>(kit: Kit, color: Color) -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(kit.fade(color, 1.0))),
            ..Default::default()
        })
        .into()
}

/// Bubble max width as a share of the chat window width, so bubbles
/// grow when the window is widened — clamped to a readable band
/// (narrow windows keep the old 360 px; very wide windows cap at
/// 760 px so prose lines don't get uncomfortably long).
fn bubble_max_width(state: &RadialState) -> f32 {
    (state.win_size.0 * 0.72).clamp(360.0, 760.0)
}

pub fn conversation<'a>(state: &'a RadialState, kit: &Kit) -> Element<'a, Message> {
    let kit = *kit;
    let mut list = column![].spacing(10);

    if state.chat().history.is_empty() {
        let suggestions =
            "Ask, automate, or configure:\n• 'Dim the screen to 40% tonight at 22:00'\n• 'Change slice colors to green'\n• 'Run the research-digest flow on this URL'\n• 'Remember I prefer warm light after sunset'";
        list = list.push(
            text(suggestions)
                .size(13)
                .color(kit.fade(kit.subtext0, 1.0)),
        );
    } else {
        for (i, msg) in state.chat().history.iter().enumerate() {
            if let Some(card) = &msg.card {
                let expanded = state.ai_card_expanded.contains(&i);
                list = list.push(super::cards::view(card, &kit, i, expanded));
                continue;
            }
            list = list.push(bubble_row(state, kit, i, msg));
        }

        // In-flight streamed reply for this thread — rendered as
        // markdown live (parsed each delta into `ai_stream_md`), so
        // formatting appears as it streams instead of reflowing at
        // completion. Falls back to plain text if parsing yielded
        // nothing yet (a trailing ▌ cursor is baked into the source).
        if let Some((idx, partial)) = &state.ai_stream {
            if *idx == state.ai_active && !partial.is_empty() {
                let content: Element<'a, Message> = if state.ai_stream_md.is_empty() {
                    text(format!("{partial}\u{258c}"))
                        .size(13)
                        .color(kit.fade(kit.text, 1.0))
                        .into()
                } else {
                    iced::widget::markdown::view(&state.ai_stream_md, iced::Theme::CatppuccinMocha)
                        .map(|url| Message::AiLinkClicked(url.to_string()))
                };
                list = list.push(row![
                    container(content)
                        .padding(iced::Padding {
                            top: 9.0,
                            right: 13.0,
                            bottom: 9.0,
                            left: 13.0,
                        })
                        .max_width(bubble_max_width(state))
                        .style(bubble_style(kit, false)),
                    Space::new().width(Length::Fill),
                ]);
            }
        }
    }

    if let Some(pending) = &state.ai_pending_question {
        list = list.push(approval_view(pending, kit));
    }

    // While the puck is armed the chat is render-only: a scrollable
    // here would capture wheel events before the caps canvas could
    // route them to page cycling ("wheel anywhere cycles" contract).
    // The conversation becomes scrollable on activation.
    if state.ai_handoff.is_armed() {
        return container(list)
            .padding(iced::Padding::default().right(12.0))
            .height(Length::Fill)
            .clip(true)
            .into();
    }
    let scroller = scrollable(container(list).padding(iced::Padding::default().right(12.0)))
        .id(CHAT_SCROLL_ID)
        .on_scroll(Message::AiChatScrolled)
        .style(kit.scrollable_style())
        .height(Length::Fill);

    // Overlay layers float above the scroller via a Stack: a
    // "jump to latest" pill (bottom) when scrolled up, and a transient
    // confirmation toast (top) e.g. after copying.
    let mut layers: Vec<Element<'a, Message>> = vec![scroller.into()];

    if !state.ai_chat_at_bottom && !state.chat().history.is_empty() {
        let pill = container(
            button(text("↓ Latest").size(12).color(kit.fade(kit.crust, 1.0)))
                .padding([5, 12])
                .style(move |_, _| button::Style {
                    background: Some(iced::Background::Color(kit.fade(kit.accent, 0.95))),
                    border: iced::border::Border::default().rounded(14.0),
                    text_color: kit.fade(kit.crust, 1.0),
                    ..Default::default()
                })
                .on_press(Message::AiScrollToBottom),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::End)
        .padding(10);
        layers.push(pill.into());
    }

    if let Some((label, _)) = &state.ai_toast {
        let toast = container(
            container(text(label).size(12).color(kit.fade(kit.crust, 1.0)))
                .padding([5, 12])
                .style(move |_| iced::widget::container::Style {
                    background: Some(iced::Background::Color(kit.fade(kit.green, 0.96))),
                    border: iced::border::Border::default().rounded(14.0),
                    text_color: Some(kit.fade(kit.crust, 1.0)),
                    ..Default::default()
                }),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Start)
        .padding(10);
        layers.push(toast.into());
    }

    if layers.len() == 1 {
        layers.pop().unwrap()
    } else {
        iced::widget::Stack::with_children(layers).into()
    }
}

/// The pending-question / command-approval prompt. Command approvals
/// (question contains a `…` command) render as the rich yellow approval
/// card from the design (preview + guardrail + Run/Always/Deny);
/// anything else renders as a styled question with its option buttons.
fn approval_view<'a>(
    pending: &'a crate::ai_client::PendingQuestion,
    kit: Kit,
) -> Element<'a, Message> {
    // Pull the command out of "Run `<cmd>`?" when present.
    let cmd = pending.question.split('`').nth(1).filter(|s| !s.is_empty());

    if let Some(cmd) = cmd {
        let bang = container(
            text("!")
                .size(11)
                .font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                })
                .color(kit.fade(kit.crust, 1.0)),
        )
        .width(Length::Fixed(17.0))
        .height(Length::Fixed(17.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(kit.fade(kit.yellow, 1.0))),
            border: iced::border::Border::default().rounded(9.0),
            ..Default::default()
        });

        let header = row![
            bang,
            text("Command approval")
                .size(12.0)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..Default::default()
                })
                .color(kit.fade(kit.text, 1.0)),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let preview = container(
            text(format!("$ {cmd}"))
                .size(11.0)
                .font(iced::Font::MONOSPACE)
                .color(kit.fade(kit.text, 1.0)),
        )
        .width(Length::Fill)
        .padding([7, 10])
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(kit.fade(kit.crust, 1.0))),
            border: iced::border::Border {
                color: kit.fade(kit.surface1, 1.0),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        });

        let guard = row![
            icon("shield", 11.0, kit.fade(kit.green, 1.0)),
            text("not on allowlist")
                .size(9.5)
                .color(kit.fade(kit.subtext0, 1.0)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // Map the options to styled actions; "Run it" is the primary,
        // right-aligned. Others (Always allow, Don't run) sit left.
        let mut actions = row![].spacing(6).align_y(Alignment::Center);
        let mut primary: Option<Element<'a, Message>> = None;
        for opt in &pending.options {
            let o = opt.clone();
            if opt == "Run it" {
                primary = Some(
                    button(
                        text("Run it")
                            .size(10.5)
                            .font(iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..Default::default()
                            })
                            .color(kit.fade(kit.crust, 1.0)),
                    )
                    .padding([4, 11])
                    .style(move |_, _| button::Style {
                        background: Some(iced::Background::Color(kit.fade(kit.yellow, 1.0))),
                        border: iced::border::Border::default().rounded(6.0),
                        text_color: kit.fade(kit.crust, 1.0),
                        ..Default::default()
                    })
                    .on_press(Message::AiChooseOption(o))
                    .into(),
                );
            } else {
                let label = if opt.starts_with("Always allow") {
                    "Always allow".to_string()
                } else {
                    "Deny".to_string()
                };
                let is_deny = !opt.starts_with("Always allow");
                actions = actions.push(
                    button(text(label).size(10.5).color(kit.fade(kit.subtext1, 1.0)))
                        .padding([4, 11])
                        .style(move |_, status| {
                            let hov = matches!(status, button::Status::Hovered);
                            button::Style {
                                border: iced::border::Border {
                                    color: if is_deny && hov {
                                        kit.fade(kit.red, 0.5)
                                    } else {
                                        kit.fade(kit.surface2, 1.0)
                                    },
                                    width: 1.0,
                                    radius: 6.0.into(),
                                },
                                text_color: if is_deny && hov {
                                    kit.fade(kit.red, 1.0)
                                } else {
                                    kit.fade(kit.subtext1, 1.0)
                                },
                                ..Default::default()
                            }
                        })
                        .on_press(Message::AiChooseOption(o)),
                );
            }
        }
        actions = actions.push(Space::new().width(Length::Fill));
        if let Some(p) = primary {
            actions = actions.push(p);
        }

        let card = column![header, preview, guard, actions].spacing(8);
        return container(card)
            .padding(12)
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(kit.fade(kit.yellow, 0.07))),
                border: iced::border::Border {
                    color: kit.fade(kit.yellow, 0.32),
                    width: 1.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            })
            .into();
    }

    // Generic multiple-choice question.
    let mut q = column![text(&pending.question)
        .size(12)
        .font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..Default::default()
        })
        .color(kit.fade(kit.accent, 1.0)),]
    .spacing(6);
    for opt in &pending.options {
        let opt_clone = opt.clone();
        q = q.push(
            button(
                text(opt)
                    .size(11)
                    .color(kit.fade(kit.text, 1.0))
                    .align_x(iced::alignment::Horizontal::Center),
            )
            .width(Length::Fill)
            .padding(6)
            .style(move |_, _status| button::Style {
                background: Some(iced::Background::Color(kit.fade(kit.accent, 0.25))),
                border: iced::border::Border {
                    color: kit.fade(kit.accent, 0.9),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                text_color: kit.fade(kit.text, 1.0),
                ..Default::default()
            })
            .on_press(Message::AiChooseOption(opt_clone)),
        );
    }
    container(q)
        .padding(10)
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(kit.fade(kit.surface0, 0.8))),
            border: iced::border::Border {
                color: kit.fade(kit.accent, 0.7),
                width: 1.0,
                radius: 10.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn bubble_style(
    kit: Kit,
    is_user: bool,
) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(if is_user {
            kit.fade(kit.accent, 0.11)
        } else {
            kit.fade(kit.surface0, 1.0)
        })),
        border: iced::border::Border {
            color: if is_user {
                kit.fade(kit.accent, 0.23)
            } else {
                kit.fade(kit.text, 0.05)
            },
            width: 1.0,
            // Speech-tail asymmetry per the design: the corner
            // nearest the sender is tight.
            radius: if is_user {
                iced::border::Radius::default()
                    .top_left(14.0)
                    .top_right(14.0)
                    .bottom_left(14.0)
                    .bottom_right(4.0)
            } else {
                iced::border::Radius::default()
                    .top_left(14.0)
                    .top_right(14.0)
                    .bottom_left(4.0)
                    .bottom_right(14.0)
            },
        },
        ..Default::default()
    }
}

/// Render AI markdown with a copy button on each code block. Non-code
/// items render in runs via `markdown::view` (which already
/// syntax-highlights); each `CodeBlock` renders via `markdown::code_block`
/// with a ⧉ button overlaid top-right that copies the raw code.
fn render_ai_markdown<'a>(
    items: &'a [iced::widget::markdown::Item],
    kit: Kit,
    img_cache: &std::collections::HashMap<String, crate::radial::ImgState>,
) -> Element<'a, Message> {
    use iced::widget::markdown;
    let special = items.iter().any(|it| {
        matches!(
            it,
            markdown::Item::CodeBlock { .. } | markdown::Item::Image { .. }
        )
    });
    // Fast path: no code blocks or images → the plain view (most replies).
    if !special {
        return markdown::view(items, iced::Theme::CatppuccinMocha)
            .map(|url| Message::AiLinkClicked(url.to_string()));
    }

    let mut col = column![].spacing(8);
    let mut run: Vec<&'a markdown::Item> = Vec::new();
    let flush = |col: iced::widget::Column<'a, Message>, run: &mut Vec<&'a markdown::Item>| {
        if run.is_empty() {
            col
        } else {
            col.push(
                markdown::view(run.drain(..), iced::Theme::CatppuccinMocha)
                    .map(|url| Message::AiLinkClicked(url.to_string())),
            )
        }
    };
    for item in items {
        match item {
            markdown::Item::CodeBlock { code, lines, .. } => {
                col = flush(col, &mut run);
                let block = markdown::code_block(
                    markdown::Settings::from(iced::Theme::CatppuccinMocha),
                    lines,
                    |url| Message::AiLinkClicked(url.to_string()),
                );
                let copy = button(icon("copy", 13.0, kit.fade(kit.subtext0, 1.0)))
                    .padding([2, 5])
                    .style(move |_, _| button::Style {
                        background: Some(iced::Background::Color(kit.fade(kit.surface1, 0.85))),
                        border: iced::border::Border::default().rounded(6.0),
                        text_color: kit.fade(kit.text, 1.0),
                        ..Default::default()
                    })
                    .on_press(Message::AiCopyText(code.clone()));
                col = col.push(iced::widget::stack![
                    block,
                    container(copy)
                        .width(Length::Fill)
                        .align_x(Alignment::End)
                        .padding(6),
                ]);
            }
            markdown::Item::Image { url, .. } => {
                col = flush(col, &mut run);
                col = col.push(render_md_image(&url.to_string(), kit, img_cache));
            }
            other => run.push(other),
        }
    }
    flush(col, &mut run).into()
}

/// Render an image referenced in an AI reply. Local paths load directly;
/// remote URLs use the fetch cache (loading / ready / failed→link).
fn render_md_image<'a>(
    url: &str,
    kit: Kit,
    img_cache: &std::collections::HashMap<String, crate::radial::ImgState>,
) -> Element<'a, Message> {
    use crate::radial::ImgState;
    let show = |handle: iced::widget::image::Handle| -> Element<'a, Message> {
        container(iced::widget::image(handle).width(Length::Fill))
            .max_width(360.0)
            .into()
    };
    // Local file path → load directly.
    if url.starts_with('/') || url.starts_with("file://") {
        let path = url.strip_prefix("file://").unwrap_or(url);
        return show(iced::widget::image::Handle::from_path(path));
    }
    match img_cache.get(url) {
        Some(ImgState::Ready(h)) => show(h.clone()),
        Some(ImgState::Loading) => text("🖼 loading image…")
            .size(11)
            .color(kit.fade(kit.subtext0, 1.0))
            .into(),
        _ => {
            // Failed or not-yet-requested → a clickable open-in-browser chip.
            let owned = url.to_string();
            button(
                text("🖼 Open image ↗")
                    .size(12)
                    .color(kit.fade(kit.accent, 1.0)),
            )
            .padding([3, 8])
            .style(|_, _| button::Style::default())
            .on_press(Message::AiLinkClicked(owned))
            .into()
        }
    }
}

fn bubble_row<'a>(
    state: &'a RadialState,
    kit: Kit,
    i: usize,
    msg: &'a crate::radial::ChatMessage,
) -> Element<'a, Message> {
    // Failed-turn bubble: red-tinted, with a Retry that re-runs the
    // last user prompt.
    if msg.is_error {
        let card = container(
            column![
                row![
                    text("⚠").size(13).color(kit.fade(kit.red, 1.0)),
                    text(format!("Something went wrong: {}", msg.text))
                        .size(12.5)
                        .color(kit.fade(kit.text, 1.0)),
                ]
                .spacing(8),
                button(
                    row![
                        icon("retry", 13.0, kit.fade(kit.crust, 1.0)),
                        text("Retry").size(12).color(kit.fade(kit.crust, 1.0)),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([4, 12])
                .style(move |_, _| button::Style {
                    background: Some(iced::Background::Color(kit.fade(kit.red, 0.9))),
                    border: iced::border::Border::default().rounded(10.0),
                    text_color: kit.fade(kit.crust, 1.0),
                    ..Default::default()
                })
                .on_press(Message::AiRetryLast),
            ]
            .spacing(8),
        )
        .padding(iced::Padding {
            top: 9.0,
            right: 13.0,
            bottom: 9.0,
            left: 13.0,
        })
        .max_width(bubble_max_width(state))
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(kit.fade(kit.red, 0.12))),
            border: iced::border::Border {
                color: kit.fade(kit.red, 0.6),
                width: 1.0,
                radius: 12.0.into(),
            },
            ..Default::default()
        });
        return row![card, Space::new().width(Length::Fill)].into();
    }

    let selecting = matches!(&state.ai_select, Some((idx, _)) if *idx == i);

    // Bubble body: a read-only text_editor while selecting (so the
    // pointer can highlight + Ctrl+C — markdown::view isn't
    // selectable); rendered markdown for AI replies; plain text for
    // user prompts.
    let content: Element<'a, Message> = if selecting {
        let ed = state.ai_select.as_ref().map(|(_, c)| c).unwrap();
        iced::widget::text_editor(ed)
            .size(13)
            .padding(0)
            .on_action(Message::AiSelectAction)
            .style(move |_theme, _status| iced::widget::text_editor::Style {
                background: iced::Background::Color(Color::TRANSPARENT),
                border: iced::border::Border::default(),
                placeholder: kit.fade(kit.subtext0, 1.0),
                value: kit.fade(kit.text, 1.0),
                selection: kit.fade(kit.accent, 0.45),
            })
            .into()
    } else if msg.is_user || msg.md.is_empty() {
        let txt = text(&msg.text).size(13).color(kit.fade(kit.text, 1.0));
        // Attached-image thumbnail above the user's text.
        if let Some(path) = &msg.image_path {
            column![
                container(
                    iced::widget::image(iced::widget::image::Handle::from_path(path))
                        .width(Length::Fill)
                )
                .max_width(220.0)
                .style(move |_| iced::widget::container::Style {
                    border: iced::border::Border::default().rounded(8.0),
                    ..Default::default()
                }),
                txt,
            ]
            .spacing(6)
            .into()
        } else {
            txt.into()
        }
    } else {
        render_ai_markdown(&msg.md, kit, &state.ai_image_cache)
    };
    let bubble = container(content)
        .padding(iced::Padding {
            top: 9.0,
            right: 13.0,
            bottom: 9.0,
            left: 13.0,
        })
        .max_width(bubble_max_width(state))
        .style(bubble_style(kit, msg.is_user));

    // Hovering the row (or an open menu) reveals the action buttons; a
    // fixed-width placeholder otherwise keeps the layout from shifting.
    let show_actions = state.ai_hover_msg == Some(i) || state.ai_context_menu == Some(i);
    let actions: Element<'a, Message> = if show_actions {
        let copy = button(icon("copy", 14.0, kit.fade(kit.subtext0, 1.0)))
            .padding([2, 4])
            .style(|_, _| button::Style::default())
            .on_press(Message::AiCopyText(msg.text.clone()));
        let (select_icon, select_color, select_msg) = if selecting {
            ("check", kit.fade(kit.accent, 1.0), Message::AiSelectExit)
        } else {
            (
                "select",
                kit.fade(kit.subtext0, 1.0),
                Message::AiBubbleSelect(i),
            )
        };
        let select = button(icon(select_icon, 14.0, select_color))
            .padding([2, 4])
            .style(|_, _| button::Style::default())
            .on_press(select_msg);
        row![copy, select].spacing(2).into()
    } else {
        Space::new().width(Length::Fixed(48.0)).into()
    };

    let inner = if msg.is_user {
        row![Space::new().width(Length::Fill), actions, bubble]
    } else {
        row![bubble, actions, Space::new().width(Length::Fill)]
    }
    .spacing(4)
    .align_y(Alignment::Center);

    // Per-bubble meta line: time · (model · tokens · cost for AI / "you").
    let meta_line = bubble_meta(state, kit, msg);

    // bubble + meta + (optional) context menu, stacked.
    let mut col = column![inner].spacing(2);
    col = col.push(meta_line);
    if state.ai_context_menu == Some(i) {
        col = col.push(bubble_context_menu(kit, i, msg, selecting));
    }

    // AI replies get a sparkle avatar to their left; user messages don't.
    let body: Element<'a, Message> = if msg.is_user {
        col.into()
    } else {
        let avatar = container(icon("sparkle", 13.0, kit.fade(kit.accent, 1.0)))
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(24.0))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(kit.fade(kit.surface0, 1.0))),
                border: iced::border::Border {
                    color: kit.fade(kit.surface2, 1.0),
                    width: 1.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            });
        row![avatar, col]
            .spacing(8)
            .align_y(Alignment::Start)
            .into()
    };

    // One mouse_area over the whole row+menu: hover stays active when the
    // pointer moves from the bubble onto its buttons. Left-click dismisses
    // an open menu; right-click opens it.
    iced::widget::mouse_area(body)
        .on_enter(Message::AiBubbleHover(Some(i)))
        .on_exit(Message::AiBubbleHover(None))
        .on_press(Message::AiBubbleMenu(None))
        .on_right_press(Message::AiBubbleMenu(Some(i)))
        .into()
}

/// The per-bubble meta line — monospace micro text under the bubble.
/// AI: `time · model · N tok · $cost`; user: `time · you` (right-aligned).
fn bubble_meta<'a>(
    state: &RadialState,
    kit: Kit,
    msg: &crate::radial::ChatMessage,
) -> Element<'a, Message> {
    let when = if msg.created_at == 0 {
        String::new()
    } else {
        format!("{} · ", crate::app::rel_time(msg.created_at))
    };
    let label = if msg.is_user {
        format!("{when}you")
    } else {
        let model = state.chat().model.clone();
        let model_short = model
            .strip_prefix("gemini-")
            .map(|m| format!("gemini {m}"))
            .unwrap_or(model);
        let (p, c) = msg.tokens;
        if p + c > 0 {
            let cost = (p as f64 / 1e6) * 0.075 + (c as f64 / 1e6) * 0.30;
            let tot = p + c;
            let tok = if tot >= 1000 {
                format!("{:.1}k", tot as f64 / 1000.0)
            } else {
                tot.to_string()
            };
            format!("{when}{model_short} · {tok} tok · ${cost:.4}")
        } else {
            format!("{when}{model_short}")
        }
    };
    let txt = text(label)
        .size(10.0)
        .font(iced::Font::MONOSPACE)
        .color(kit.fade(kit.overlay0, 1.0));
    if msg.is_user {
        row![Space::new().width(Length::Fill), txt].into()
    } else {
        row![txt].into()
    }
}

/// The right-click context menu for a bubble: copy, toggle selection,
/// and paste-into-input. Rendered as a small card under the bubble
/// (iced has no native cursor-anchored menu; attaching it to the
/// bubble is robust and needs no absolute positioning).
fn bubble_context_menu<'a>(
    kit: Kit,
    i: usize,
    msg: &'a crate::radial::ChatMessage,
    selecting: bool,
) -> Element<'a, Message> {
    let item = |label: &str, m: Message| {
        button(
            text(label.to_string())
                .size(12)
                .color(kit.fade(kit.text, 1.0)),
        )
        .width(Length::Fill)
        .padding([5, 10])
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: hovered.then(|| iced::Background::Color(kit.fade(kit.accent, 0.25))),
                border: iced::border::Border::default().rounded(6.0),
                text_color: kit.fade(kit.text, 1.0),
                ..Default::default()
            }
        })
        .on_press(m)
    };

    let mut menu = column![item("Copy message", Message::AiCopyText(msg.text.clone()))].spacing(1);
    menu = menu.push(if selecting {
        item("Stop selecting", Message::AiSelectExit)
    } else {
        item("Select text", Message::AiBubbleSelect(i))
    });
    menu = menu.push(item("Paste into input", Message::AiPasteToInput));

    let card = container(menu).padding(4).max_width(190.0).style(move |_| {
        iced::widget::container::Style {
            background: Some(iced::Background::Color(kit.fade(kit.surface0, 0.98))),
            border: iced::border::Border {
                color: kit.fade(kit.surface2, 1.0),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        }
    });

    if msg.is_user {
        row![Space::new().width(Length::Fill), card]
    } else {
        row![card, Space::new().width(Length::Fill)]
    }
    .into()
}

/// Full thread list (rename / delete / open) — functionality kept
/// from the pre-redesign toolbar, reached via the strip's "…" chip.
pub fn threads_list<'a>(state: &'a RadialState, kit: &Kit) -> Element<'a, Message> {
    let kit = *kit;

    let query = state.ai_threads_query.trim().to_lowercase();
    let matches = |t: &crate::radial::ChatThread| -> bool {
        query.is_empty()
            || t.title.to_lowercase().contains(&query)
            || t.history
                .iter()
                .any(|m| m.text.to_lowercase().contains(&query))
    };

    let search = text_input("Search chats…", &state.ai_threads_query)
        .size(12)
        .padding(7)
        .on_input(Message::AiThreadsSearch)
        .style(move |theme, status| {
            let mut s = iced::widget::text_input::default(theme, status);
            s.background = iced::Background::Color(kit.fade(kit.crust, 1.0));
            s.border.color = kit.fade(kit.surface2, 1.0);
            s.border.radius = 10.0.into();
            s.value = kit.fade(kit.text, 1.0);
            s.placeholder = kit.fade(kit.subtext0, 1.0);
            s
        })
        .width(Length::Fill);

    let mut list = column![].spacing(6);
    let saved = state
        .ai_threads
        .iter()
        .filter(|t| !t.history.is_empty() && matches(t))
        .count();
    if saved == 0 {
        list = list.push(
            text(if query.is_empty() {
                "No previous chats yet."
            } else {
                "No chats match the search."
            })
            .size(12)
            .color(kit.fade(kit.subtext0, 1.0)),
        );
    }
    for (idx, thread) in state.ai_threads.iter().enumerate().rev() {
        if thread.history.is_empty() || !matches(thread) {
            continue;
        }
        let is_active = idx == state.ai_active;
        let renaming = matches!(state.ai_renaming, Some((r, _)) if r == idx);

        let title_el: Element<'a, Message> = if renaming {
            let draft = state
                .ai_renaming
                .as_ref()
                .map(|(_, d)| d.as_str())
                .unwrap_or("");
            text_input("Thread name…", draft)
                .size(12)
                .padding(4)
                .on_input(Message::AiRenameInput)
                .on_submit(Message::AiRenameCommit)
                .into()
        } else {
            let title = if thread.title.is_empty() {
                "Untitled chat".to_string()
            } else {
                thread.title.clone()
            };
            text(title).size(12).color(kit.fade(kit.text, 1.0)).into()
        };

        let meta = format!(
            "{} · {} messages · {}",
            thread.mode.label(),
            thread.history.len(),
            crate::app::rel_time(thread.updated_at),
        );

        let open_btn = button(
            column![
                title_el,
                text(meta).size(10).color(kit.fade(kit.subtext0, 0.9))
            ]
            .spacing(2),
        )
        .width(Length::Fill)
        .padding(8)
        .style(move |_, _status| button::Style {
            background: Some(iced::Background::Color(if is_active {
                kit.fade(kit.accent, 0.15)
            } else {
                kit.fade(kit.surface0, 0.8)
            })),
            border: iced::border::Border {
                color: if is_active {
                    kit.fade(kit.accent, 0.6)
                } else {
                    kit.fade(kit.text, 0.06)
                },
                width: 1.0,
                radius: 9.0.into(),
            },
            text_color: kit.fade(kit.text, 1.0),
            ..Default::default()
        })
        .on_press(Message::AiSelectThread(idx));

        let small_btn = |name: &'static str, msg: Message| {
            button(icon(name, 14.0, kit.fade(kit.subtext0, 1.0)))
                .padding([4, 6])
                .style(|_, _| button::Style::default())
                .on_press(msg)
        };
        let rename_btn = if renaming {
            small_btn("check", Message::AiRenameCommit)
        } else {
            small_btn("rename", Message::AiRenameStart(idx))
        };

        list = list.push(
            row![
                open_btn,
                rename_btn,
                small_btn("export", Message::AiExportThread(idx)),
                small_btn("trash", Message::AiDeleteThread(idx)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
    }
    column![
        search,
        scrollable(container(list).padding(iced::Padding::default().right(12.0)))
            .style(kit.scrollable_style())
            .height(Length::Fill),
    ]
    .spacing(8)
    .into()
}
