//! Dock, bubble, and peek views for the activity-bubbles system (spec
//! §1 — visible GUI layer). All colours resolved through `Kit`; no
//! hardcoded hex anywhere in this file.

#![allow(dead_code)]

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length};

use oxidemx_widgets::controls as widgets;
use oxidemx_widgets::icons::icon;
use oxidemx_widgets::kit::Kit;
use oxidemx_widgets::tokens;

use crate::activity::{ActivityState, BubbleState, ClusterStatus, RunCluster};
use crate::app::Message;

/// Entry point: returns `None` when there are no clusters for the active
/// conversation (dock is hidden when the active thread has no runs).
pub fn dock_view<'a>(act: &'a ActivityState, kit: &Kit, active_conv: &str) -> Option<Element<'a, Message>> {
    let kit = *kit;
    let (active, recent) = {
        let (a, r) = act.partition();
        let a: Vec<&RunCluster> = a.into_iter().filter(|c| c.conversation_id == active_conv).collect();
        let r: Vec<&RunCluster> = r.into_iter().filter(|c| c.conversation_id == active_conv).collect();
        (a, r)
    };
    if active.is_empty() && recent.is_empty() {
        return None;
    }

    match &act.expanded {
        None => Some(collapsed_dock(active, recent, kit)),
        Some(run_id) => {
            // Only render the expanded panel when the run belongs to the active
            // conversation. If the user switched conversations while a panel was
            // open, fall through to the normal collapsed dock for the new thread.
            let belongs = act
                .cluster(run_id)
                .map(|c| c.conversation_id.as_str() == active_conv)
                .unwrap_or(false);
            if belongs {
                let run_id = run_id.clone();
                Some(expanded_panel(act, kit, &run_id))
            } else {
                Some(collapsed_dock(active, recent, kit))
            }
        }
    }
}

// ── Collapsed: a row of cluster orbs ─────────────────────────────────────────

fn collapsed_dock<'a>(
    active: Vec<&'a RunCluster>,
    recent: Vec<&'a RunCluster>,
    kit: Kit,
) -> Element<'a, Message> {
    let orbs: Vec<Element<'_, Message>> = active
        .iter()
        .map(|c| cluster_orb(kit, c))
        .collect();

    let mut dock_row = row(orbs).spacing(8.0).align_y(Alignment::Center);

    // Recent-run tray: small status dots for runs finished >6s ago (cap 5).
    // Each dot is a button that expands the run's panel.
    if !recent.is_empty() {
        let tray_dots: Vec<Element<'_, Message>> = recent
            .iter()
            .map(|c| {
                let dot_color = match c.status {
                    ClusterStatus::Finished => kit.green,
                    _ => kit.red, // Failed or Cancelled
                };
                let run_id = c.run_id.clone();
                button(
                    container(Space::new())
                        .width(Length::Fixed(8.0))
                        .height(Length::Fixed(8.0))
                        .style(move |_| container::Style {
                            background: Some(Background::Color(kit.fade(dot_color, 0.85))),
                            border: Border::default().rounded(4.0),
                            ..Default::default()
                        }),
                )
                .padding(3)
                .style(|_, _| button::Style::default())
                .on_press(Message::ActivityExpand(run_id))
                .into()
            })
            .collect();

        dock_row = dock_row.push(
            row(tray_dots).spacing(2.0).align_y(Alignment::Center),
        );
    }

    container(dock_row)
    .padding([6, 8])
    .style(move |_| container::Style {
        background: Some(Background::Color(kit.fade(kit.surface0, 0.88))),
        border: Border::default()
            .rounded(32.0)
            .color(kit.fade(kit.hairline, 1.0))
            .width(1.0),
        ..Default::default()
    })
    .into()
}

fn cluster_orb<'a>(kit: Kit, c: &'a RunCluster) -> Element<'a, Message> {
    let is_running = matches!(c.status, ClusterStatus::Running);
    let orb_color = if is_running { kit.accent } else { kit.green };
    let unread: u32 = c.bubbles.iter().map(|b| b.unread).sum();

    let orb_body = container(
        iced::widget::center(icon("agents", 20.0, kit.fade(kit.crust, 1.0))),
    )
    .width(Length::Fixed(44.0))
    .height(Length::Fixed(44.0))
    .style(move |_| container::Style {
        background: Some(Background::Color(kit.fade(orb_color, 1.0))),
        border: Border::default()
            .rounded(22.0)
            .color(kit.fade(orb_color, 0.6))
            .width(2.0),
        // Shadow alpha breathes when running: 0.25–0.55 driven by kit.pulse.
        // (bob/spin/appear + reduce-motion deferred — see spec §6)
        shadow: if is_running {
            iced::Shadow {
                color: kit.fade(orb_color, 0.25 + 0.30 * kit.pulse),
                offset: iced::Vector::ZERO,
                blur_radius: 10.0,
            }
        } else {
            iced::Shadow::default()
        },
        ..Default::default()
    });

    let mut stack_children: Vec<Element<'_, Message>> = vec![orb_body.into()];

    if unread > 0 {
        let badge_label = if unread > 99 { "99+".to_string() } else { unread.to_string() };
        let badge = container(
            text(badge_label)
                .size(tokens::T_MICRO)
                .font(iced::Font { weight: iced::font::Weight::Bold, ..Default::default() })
                .color(kit.fade(kit.crust, 1.0)),
        )
        .padding([1.0, 4.0])
        .style(move |_| container::Style {
            background: Some(Background::Color(kit.fade(kit.red, 1.0))),
            border: Border::default().rounded(8.0),
            ..Default::default()
        });

        // Float badge top-right of the orb
        let badged = container(badge)
            .width(Length::Fixed(44.0))
            .height(Length::Fixed(44.0))
            .align_x(iced::Alignment::End)
            .align_y(iced::Alignment::Start);
        stack_children.push(badged.into());
    }

    button(
        iced::widget::Stack::with_children(stack_children)
            .width(Length::Fixed(44.0))
            .height(Length::Fixed(44.0)),
    )
    .padding(0)
    .style(|_, _| button::Style::default())
    .on_press(Message::ActivityExpand(c.run_id.clone()))
    .into()
}

// ── Expanded panel ────────────────────────────────────────────────────────────

fn expanded_panel<'a>(
    act: &'a ActivityState,
    kit: Kit,
    run_id: &str,
) -> Element<'a, Message> {
    let Some(cluster) = act.cluster(run_id) else {
        // Run disappeared while expanded — collapse gracefully.
        // Space must fill the layer so the button has actual hit area.
        return button(Space::new().width(Length::Fill).height(Length::Fill))
            .padding(0)
            .style(|_, _| button::Style::default())
            .on_press(Message::ActivityCollapse)
            .into();
    };

    let is_running = matches!(cluster.status, ClusterStatus::Running);
    let running_count = cluster.running_count();

    // Header status label
    let status_label: String = if is_running {
        format!("{running_count} running")
    } else {
        "all done".to_string()
    };

    let header = container(
        row![
            widgets::status_dot(kit, if is_running { kit.accent } else { kit.green }, is_running),
            text(status_label)
                .size(tokens::T_LABEL)
                .color(kit.fade(kit.subtext0, 1.0)),
            text(cluster.flow_id.clone())
                .size(tokens::T_LABEL)
                .font(iced::Font { weight: iced::font::Weight::Semibold, ..Default::default() })
                .color(kit.fade(kit.text, 1.0)),
            Space::new().width(Length::Fill),
            widgets::ghost_icon_button(
                kit,
                "close",
                14.0,
                kit.fade(kit.subtext0, 1.0),
                Message::ActivityCollapse,
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .width(Length::Fixed(286.0))
    .style(move |_| container::Style {
        background: Some(Background::Color(kit.fade(kit.surface0, 0.95))),
        border: Border {
            radius: iced::border::Radius::default().top_left(12.0).top_right(12.0),
            color: kit.fade(kit.hairline, 1.0),
            width: 1.0,
        },
        ..Default::default()
    });

    // Bubble column
    let bubbles_col: Element<'_, Message> = {
        let items: Vec<Element<'_, Message>> = cluster
            .bubbles
            .iter()
            .map(|b| {
                let peek_open = act
                    .peek
                    .as_ref()
                    .is_some_and(|(r, s)| r == &cluster.run_id && s == &b.step);
                bubble_view(kit, &cluster.run_id, b, peek_open)
            })
            .collect();
        column(items).spacing(6).into()
    };

    let mut panel_col = column![header, bubbles_col].spacing(0).width(Length::Fixed(286.0));

    // Peek popover if open for a step in this cluster
    if let Some((pk_run, pk_step)) = &act.peek {
        if pk_run == run_id {
            if let Some(b) = cluster.bubbles.iter().find(|b| &b.step == pk_step) {
                panel_col = panel_col.push(peek_view(kit, cluster, b));
            }
        }
    }

    // Transparent backdrop: fills the whole stack layer, catches clicks to collapse
    let backdrop = button(Space::new().width(Length::Fill).height(Length::Fill))
        .padding(0)
        .style(|_, _| button::Style { background: None, ..Default::default() })
        .on_press(Message::ActivityCollapse);

    // Align panel_col to the bottom-right corner within the Fill×Fill Stack layer.
    // The outer container in view.rs owns all padding ([0,18,96,0]); no padding here.
    let aligned_panel = iced::widget::container(panel_col)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::Alignment::End)
        .align_y(iced::Alignment::End)
        .into();

    iced::widget::Stack::with_children(vec![
        backdrop.into(),
        aligned_panel,
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// ── Bubble view (52px orb) ────────────────────────────────────────────────────

fn bubble_view<'a>(
    kit: Kit,
    run_id: &'a str,
    b: &'a crate::activity::AgentBubble,
    peek_open: bool,
) -> Element<'a, Message> {
    let tone_color = b.tone.color(&kit);
    let alpha_fill = if matches!(b.state, BubbleState::Working) {
        0.25 + 0.15 * kit.pulse
    } else {
        0.18
    };
    let is_live = matches!(b.state, BubbleState::Working);
    let icon_name = b.tone.icon();

    // Badge label / color
    let badge_el: Option<Element<'_, Message>> = match b.state {
        BubbleState::Done => Some(
            container(icon("check", 9.0, kit.fade(kit.crust, 1.0)))
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(kit.fade(kit.green, 1.0))),
                    border: Border::default().rounded(7.0),
                    ..Default::default()
                })
                .into(),
        ),
        BubbleState::Failed => Some(
            container(
                text("!")
                    .size(9.0)
                    .font(iced::Font { weight: iced::font::Weight::Bold, ..Default::default() })
                    .color(kit.fade(kit.crust, 1.0)),
            )
            .width(Length::Fixed(14.0))
            .height(Length::Fixed(14.0))
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .style(move |_| container::Style {
                background: Some(Background::Color(kit.fade(kit.red, 1.0))),
                border: Border::default().rounded(7.0),
                ..Default::default()
            })
            .into(),
        ),
        BubbleState::Working if b.unread > 0 => {
            let label = if b.unread > 99 { "99+".to_string() } else { b.unread.to_string() };
            Some(
                container(
                    text(label)
                        .size(9.0)
                        .font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..Default::default()
                        })
                        .color(kit.fade(kit.crust, 1.0)),
                )
                .padding([1.0, 3.0])
                .style(move |_| container::Style {
                    background: Some(Background::Color(kit.fade(kit.accent, 1.0))),
                    border: Border::default().rounded(7.0),
                    ..Default::default()
                })
                .into(),
            )
        }
        _ => None,
    };

    // Dismiss button (terminal states)
    let is_terminal = b.is_terminal();
    let run_id_s = run_id.to_string();
    let step_s = b.step.clone();

    // Border alpha breathes when working: 0.55–1.0 driven by kit.pulse.
    // (bob/spin/appear + reduce-motion deferred — see spec §6)
    let border_alpha = if is_live {
        0.55 + 0.45 * kit.pulse
    } else if peek_open {
        1.0
    } else {
        0.5
    };
    let orb = container(iced::widget::center(icon(icon_name, 22.0, kit.fade(tone_color, 1.0))))
        .width(Length::Fixed(52.0))
        .height(Length::Fixed(52.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(kit.fade(tone_color, alpha_fill))),
            border: Border::default()
                .rounded(26.0)
                .color(kit.fade(tone_color, border_alpha))
                .width(if peek_open { 2.0 } else { 1.5 }),
            shadow: if is_live {
                iced::Shadow {
                    color: kit.fade(tone_color, 0.35),
                    offset: iced::Vector::ZERO,
                    blur_radius: 8.0,
                }
            } else {
                iced::Shadow::default()
            },
            ..Default::default()
        });

    let mut orb_stack: Vec<Element<'_, Message>> = vec![orb.into()];
    if let Some(badge) = badge_el {
        let badge_wrap = container(badge)
            .width(Length::Fixed(52.0))
            .height(Length::Fixed(52.0))
            .align_x(iced::Alignment::End)
            .align_y(iced::Alignment::Start);
        orb_stack.push(badge_wrap.into());
    }

    let orb_with_badge = iced::widget::Stack::with_children(orb_stack)
        .width(Length::Fixed(52.0))
        .height(Length::Fixed(52.0));

    // Agent name label
    let agent_label = if b.agent.is_empty() { &b.step } else { &b.agent };
    let label_el = text(agent_label)
        .size(10.0)
        .color(kit.fade(kit.subtext0, 1.0));

    let mut row_children: Vec<Element<'_, Message>> = vec![
        button(orb_with_badge)
            .padding(0)
            .style(|_, _| button::Style::default())
            .on_press(Message::BubblePeekToggle(run_id_s.clone(), step_s.clone()))
            .into(),
        column![label_el].spacing(0).into(),
    ];

    if is_terminal {
        row_children.push(Space::new().width(Length::Fill).into());
        row_children.push(
            widgets::ghost_icon_button(
                kit,
                "close",
                12.0,
                kit.fade(kit.subtext0, 0.7),
                Message::BubbleDismiss(run_id_s, step_s),
            )
            .into(),
        );
    }

    container(
        row(row_children).spacing(8).align_y(Alignment::Center),
    )
    .padding([4, 12])
    .width(Length::Fixed(286.0))
    .style(move |_| container::Style {
        background: Some(Background::Color(kit.fade(kit.surface0, 0.85))),
        border: Border::default()
            .color(kit.fade(kit.hairline, 1.0))
            .width(1.0),
        ..Default::default()
    })
    .into()
}

// ── Peek view (286px popover) ─────────────────────────────────────────────────

fn peek_view<'a>(
    kit: Kit,
    cluster: &'a RunCluster,
    b: &'a crate::activity::AgentBubble,
) -> Element<'a, Message> {
    let tone_color = b.tone.color(&kit);
    let icon_name = b.tone.icon();

    // Elapsed time from bubble start or cluster start estimate
    let elapsed_secs = b
        .started_at
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    let elapsed_str = if elapsed_secs >= 3600 {
        format!("{}h {}m", elapsed_secs / 3600, (elapsed_secs % 3600) / 60)
    } else if elapsed_secs >= 60 {
        format!("{}m {}s", elapsed_secs / 60, elapsed_secs % 60)
    } else {
        format!("{elapsed_secs}s")
    };

    let state_label = match b.state {
        BubbleState::Pending => "pending",
        BubbleState::Working => "working",
        BubbleState::Done => "done",
        BubbleState::Failed => "failed",
        BubbleState::Skipped => "skipped",
    };

    // Header row: icon + name + state + elapsed
    let peek_header = row![
        container(iced::widget::center(icon(icon_name, 16.0, kit.fade(tone_color, 1.0))))
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(kit.fade(tone_color, 0.2))),
                border: Border::default().rounded(14.0),
                ..Default::default()
            }),
        column![
            text(if b.agent.is_empty() { &b.step } else { &b.agent })
                .size(tokens::T_LABEL)
                .font(iced::Font { weight: iced::font::Weight::Semibold, ..Default::default() })
                .color(kit.fade(kit.text, 1.0)),
            text(format!("{state_label} · {elapsed_str}"))
                .size(tokens::T_MICRO)
                .color(kit.fade(kit.subtext0, 1.0)),
        ]
        .spacing(1),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Progress bar — 286px panel minus 2×12px padding = 262px track width.
    // FillPortion only works proportionally inside a Row/Column with siblings;
    // using it as a sole child just fills 100%. Use a Row with fill-bar + Space
    // so the fill fraction is semantically correct.
    let progress = cluster.progress();
    let track_width = 262.0_f32;
    let fill_px = (track_width * progress.clamp(0.0, 1.0)).max(0.0);
    let fill_bar = container(Space::new())
        .width(Length::Fixed(fill_px))
        .height(Length::Fixed(4.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(kit.fade(kit.accent, 1.0))),
            border: Border::default().rounded(2.0),
            ..Default::default()
        });
    let progress_bar = container(
        row![fill_bar, Space::new().width(Length::Fill)]
            .spacing(0),
    )
    .width(Length::Fill)
    .height(Length::Fixed(4.0))
    .style(move |_| container::Style {
        background: Some(Background::Color(kit.fade(kit.surface2, 1.0))),
        border: Border::default().rounded(2.0),
        ..Default::default()
    });

    // Log tail: last 6 entries, monospace
    let log_tail: Vec<Element<'_, Message>> = b
        .logs
        .iter()
        .rev()
        .take(6)
        .rev()
        .map(|l| {
            text(l)
                .size(10.0)
                .font(iced::Font {
                    family: iced::font::Family::Monospace,
                    ..Default::default()
                })
                .color(kit.fade(kit.subtext0, 0.85))
                .into()
        })
        .collect();

    let log_col = column(log_tail).spacing(2);
    let log_scroll = scrollable(log_col)
        .height(Length::Fixed(80.0))
        .style(kit.scrollable_style());

    // Action chips based on state
    let run_id = cluster.run_id.clone();
    let flow_id = cluster.flow_id.clone();
    let step = b.step.clone();

    let mut chip_row: Vec<Element<'_, Message>> = Vec::new();
    match b.state {
        BubbleState::Working => {
            chip_row.push(
                widgets::ghost_icon_button(
                    kit,
                    "stop",
                    13.0,
                    kit.fade(kit.red, 1.0),
                    Message::RunCancel(run_id.clone()),
                )
                .into(),
            );
        }
        BubbleState::Done => {
            chip_row.push(
                widgets::ghost_icon_button(
                    kit,
                    "memory",
                    13.0,
                    kit.fade(kit.subtext0, 1.0),
                    Message::RunTranscript(run_id.clone()),
                )
                .into(),
            );
            if let Some(artifact) = &b.artifact {
                let artifact = artifact.clone();
                chip_row.push(
                    widgets::ghost_icon_button(
                        kit,
                        "doc",
                        13.0,
                        kit.fade(kit.accent, 1.0),
                        Message::RunOpenArtifact(artifact),
                    )
                    .into(),
                );
            }
            chip_row.push(
                widgets::ghost_icon_button(
                    kit,
                    "close",
                    13.0,
                    kit.fade(kit.subtext0, 0.7),
                    Message::BubbleDismiss(run_id, step),
                )
                .into(),
            );
        }
        BubbleState::Failed => {
            chip_row.push(
                widgets::ghost_icon_button(
                    kit,
                    "retry",
                    13.0,
                    kit.fade(kit.peach, 1.0),
                    Message::RunRetry(run_id.clone(), flow_id),
                )
                .into(),
            );
            chip_row.push(
                widgets::ghost_icon_button(
                    kit,
                    "close",
                    13.0,
                    kit.fade(kit.subtext0, 0.7),
                    Message::BubbleDismiss(run_id, step),
                )
                .into(),
            );
        }
        _ => {}
    }

    let chips = row(chip_row).spacing(4).align_y(Alignment::Center);

    container(
        column![
            peek_header,
            progress_bar,
            log_scroll,
            chips,
        ]
        .spacing(8),
    )
    .width(Length::Fixed(286.0))
    .padding([10, 12])
    .style(move |_| container::Style {
        background: Some(Background::Color(kit.fade(kit.surface0, 0.96))),
        border: Border {
            radius: iced::border::Radius::default().bottom_left(12.0).bottom_right(12.0),
            color: kit.fade(kit.hairline, 1.0),
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: kit.fade(Color::BLACK, 0.3),
            offset: iced::Vector { x: 0.0, y: 4.0 },
            blur_radius: 12.0,
        },
        ..Default::default()
    })
    .into()
}
