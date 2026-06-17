//! Conversation body: bubbles, agent cards, in-flight stream, the
//! pending multiple-choice question, and the full thread list view
//! (reached from the "…" chip when more threads exist than fit the
//! strip).

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Color, Element, Length};

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
                list = list.push(super::cards::view(card, &kit));
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
                    iced::widget::markdown::view(
                        &state.ai_stream_md,
                        iced::Theme::CatppuccinMocha,
                    )
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
        list = list.push(
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
                }),
        );
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

    // When scrolled up away from the latest message, float a
    // "jump to latest" pill over the bottom — clicking snaps back and
    // re-enables follow-along auto-scroll.
    if state.ai_chat_at_bottom || state.chat().history.is_empty() {
        scroller.into()
    } else {
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
        iced::widget::stack![scroller, pill].into()
    }
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

fn bubble_row<'a>(
    state: &'a RadialState,
    kit: Kit,
    i: usize,
    msg: &'a crate::radial::ChatMessage,
) -> Element<'a, Message> {
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
        text(&msg.text)
            .size(13)
            .color(kit.fade(kit.text, 1.0))
            .into()
    } else {
        iced::widget::markdown::view(&msg.md, iced::Theme::CatppuccinMocha)
            .map(|url| Message::AiLinkClicked(url.to_string()))
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
        let copy = button(text("⧉").size(14).color(kit.fade(kit.subtext0, 1.0)))
            .padding([2, 4])
            .style(|_, _| button::Style::default())
            .on_press(Message::AiCopyText(msg.text.clone()));
        let select_glyph = if selecting { "✓" } else { "⌶" };
        let select_msg = if selecting {
            Message::AiSelectExit
        } else {
            Message::AiBubbleSelect(i)
        };
        let select = button(text(select_glyph).size(14).color(kit.fade(kit.subtext0, 1.0)))
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

    // Right-click context menu, attached under the bubble.
    let mut stack = column![inner].spacing(4);
    if state.ai_context_menu == Some(i) {
        stack = stack.push(bubble_context_menu(kit, i, msg, selecting));
    }

    // One mouse_area over the whole row+menu: hover stays active when
    // the pointer moves from the bubble onto its buttons (the old bug
    // — the area wrapped only the bubble, so reaching for the copy
    // button left the area and hid it). Left-click dismisses an open
    // menu; right-click opens it.
    iced::widget::mouse_area(stack)
        .on_enter(Message::AiBubbleHover(Some(i)))
        .on_exit(Message::AiBubbleHover(None))
        .on_press(Message::AiBubbleMenu(None))
        .on_right_press(Message::AiBubbleMenu(Some(i)))
        .into()
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
        button(text(label.to_string()).size(12).color(kit.fade(kit.text, 1.0)))
            .width(Length::Fill)
            .padding([5, 10])
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered);
                button::Style {
                    background: hovered
                        .then(|| iced::Background::Color(kit.fade(kit.accent, 0.25))),
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

    let card = container(menu)
        .padding(4)
        .max_width(190.0)
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(kit.fade(kit.surface0, 0.98))),
            border: iced::border::Border {
                color: kit.fade(kit.surface2, 1.0),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
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
    let mut list = column![].spacing(6);
    let saved = state
        .ai_threads
        .iter()
        .filter(|t| !t.history.is_empty())
        .count();
    if saved == 0 {
        list = list.push(
            text("No previous chats yet.")
                .size(12)
                .color(kit.fade(kit.subtext0, 1.0)),
        );
    }
    for (idx, thread) in state.ai_threads.iter().enumerate().rev() {
        if thread.history.is_empty() {
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

        let small_btn = |label: &'static str, msg: Message| {
            button(text(label).size(14).color(kit.fade(kit.subtext0, 1.0)))
                .padding([4, 6])
                .style(|_, _| button::Style::default())
                .on_press(msg)
        };
        let rename_btn = if renaming {
            small_btn("✓", Message::AiRenameCommit)
        } else {
            small_btn("✎", Message::AiRenameStart(idx))
        };

        list = list.push(
            row![
                open_btn,
                rename_btn,
                small_btn("✕", Message::AiDeleteThread(idx)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
    }
    scrollable(container(list).padding(iced::Padding::default().right(12.0)))
        .style(kit.scrollable_style())
        .height(Length::Fill)
        .into()
}
