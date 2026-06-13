//! "AI" tab — agent runtime configuration: Gemini transport
//! backend, model id, API key, and the execute_command allowlist.
//!
//! The key panel moved here from the Settings tab's "AI Assistant"
//! section (P1a); it keeps the same write-only contract — the
//! stored key is never loaded back into the UI, and saving writes
//! `~/.config/oxidemx/gemini.key` with 0600 perms.

use crate::{Message, State};
use iced::widget::{button, column, container, pick_list, row, rule, text, text_input};
use iced::{Alignment, Element, Length};
use oxidemx_shared::config::AiProvider;
use oxidemx_widgets::style;
use oxidemx_widgets::widgets::section_header;

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    column![
        section_header("AI"),
        text(
            "The agent runtime behind the radial menu's AI page: which \
             provider it talks to, which model, the API key, and which \
             shell commands the agent may run without asking. Edits \
             autosave; the overlay picks them up via inotify within \
             ~150 ms.",
        )
        .size(12)
        .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
        section_block(state, "Provider", provider_picker(state)),
        section_block(state, "Model", model_picker(state)),
        section_block(state, "API key", key_panel(state)),
        section_block(state, "Command allowlist", allowlist_editor(state)),
    ]
    .spacing(16)
    .into()
}

// ============================================================================
// Provider
// ============================================================================

fn provider_picker(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let current = state.config.overlay.ai.provider;

    let intro = text(
        "Which LLM serves the chat. Gemini, OpenAI and Anthropic use \
         their API keys (below). Ollama runs local models (no key). \
         Claude Code uses your `claude` CLI / subscription (no key) but \
         is chat-only — the agent tools (run command, memory, …) don't \
         apply on that path. Switching providers resets the model to \
         that provider's default.",
    )
    .size(11)
    .style(style::text_dim(pal));

    let picker = pick_list(AiProvider::ALL, Some(current), Message::AiProviderChanged)
        .text_size(12)
        .padding(6)
        .style(style::pick_list_style(pal));

    column![intro, picker].spacing(8).into()
}

// ============================================================================
// Model
// ============================================================================

fn model_picker(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let provider = state.config.overlay.ai.provider;
    let current = state.config.overlay.ai.model.clone();

    let intro = text(
        "Pick a model for the selected provider, or type any id in the \
         field. Claude Code leaves this blank to use your subscription \
         default.",
    )
    .size(11)
    .style(style::text_dim(pal));

    let known: Vec<String> = provider
        .model_suggestions()
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let selected = known.iter().find(|m| **m == current).cloned();
    let quick = pick_list(known, selected, Message::AiModelChanged)
        .placeholder("custom…")
        .text_size(12)
        .padding(6)
        .style(style::pick_list_style(pal));

    let custom = text_input("model id", &current)
        .on_input(Message::AiModelChanged)
        .padding(6)
        .size(12)
        .width(Length::Fill);

    column![intro, row![quick, custom].spacing(8).align_y(Alignment::Center)]
        .spacing(8)
        .into()
}

// ============================================================================
// API key — per selected provider (write-only contract)
// ============================================================================

fn key_panel(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let provider = state.config.overlay.ai.provider;

    // Keyless providers (Ollama, Claude Code) just show a note.
    let Some(stem) = provider.key_file_stem() else {
        return text(format!(
            "{} needs no API key.",
            provider.label()
        ))
        .size(11)
        .style(style::text_dim(pal))
        .into();
    };

    let env = provider.key_env().unwrap_or("");
    let intro = text(format!(
        "API key for {}. Stored outside config.json at \
         ~/.config/oxidemx/{stem}.key (0600) — never included in config \
         exports. The {env} environment variable overrides it.",
        provider.label()
    ))
    .size(11)
    .style(style::text_dim(pal));

    let status_line: Element<Message> = if state.ai_key_present {
        text("Key configured ✓ — paste a new one below to replace it.")
            .size(11)
            .style(style::text_accent(pal))
            .into()
    } else {
        text("No key configured — the AI page will answer with an error until one is set.")
            .size(11)
            .style(style::text_faint(pal))
            .into()
    };

    let input = text_input("Paste API key…", &state.ai_key_draft)
        .secure(true)
        .on_input(Message::AiKeyDraftChanged)
        .on_submit(Message::AiKeySave)
        .padding(6)
        .size(12)
        .width(Length::Fill);

    let save_btn = button(text("Save").size(11))
        .style(style::btn_primary(pal))
        .on_press(Message::AiKeySave);

    let mut form = row![input, save_btn].align_y(Alignment::Center).spacing(8);
    if state.ai_key_present {
        form = form.push(
            button(text("Remove").size(11))
                .style(style::btn_danger(pal))
                .on_press(Message::AiKeyRemove),
        );
    }

    column![intro, status_line, form].spacing(8).into()
}

// ============================================================================
// Command allowlist
// ============================================================================

fn allowlist_editor(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let entries = &state.config.overlay.ai.command_allowlist;

    let intro = text(
        "Commands the agent's execute_command tool may run without a \
         confirmation chip. Entries match the leading whole tokens of a \
         command — \"systemctl --user\" allows \"systemctl --user status \
         foo\" but not \"systemctl enable foo\". A trailing * is \
         cosmetic; matching is prefix-shaped either way. Compound \
         commands (&&, |, ;) need every part allowlisted.",
    )
    .size(11)
    .style(style::text_dim(pal));

    let mut list = column![].spacing(6);
    if entries.is_empty() {
        list = list.push(
            text("Empty — every command will ask for confirmation.")
                .size(11)
                .style(style::text_faint(pal)),
        );
    } else {
        for (i, entry) in entries.iter().enumerate() {
            list = list.push(
                row![
                    text(entry.clone()).size(12).width(Length::Fill),
                    button(text("Remove").size(10))
                        .style(style::btn_danger(pal))
                        .on_press(Message::AiAllowlistRemove(i)),
                ]
                .align_y(Alignment::Center)
                .spacing(8),
            );
        }
    }

    let add_form = row![
        text_input("e.g. \"git status\" or \"flatpak list\"", &state.ai_allowlist_draft)
            .on_input(Message::AiAllowlistDraftChanged)
            .on_submit(Message::AiAllowlistAdd)
            .padding(6)
            .size(12)
            .width(Length::Fill),
        button(text("Add").size(11))
            .style(style::btn_primary(pal))
            .on_press(Message::AiAllowlistAdd),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    column![intro, list, add_form].spacing(10).into()
}

// ============================================================================
// Shared card chrome (same shape as settings_page's section_block)
// ============================================================================

fn section_block<'a>(
    state: &'a State,
    title: &str,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let pal = &state.palette;
    container(
        column![
            text(title.to_string()).size(16),
            rule::horizontal(1).style(style::rule_style(pal)),
            body,
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}
