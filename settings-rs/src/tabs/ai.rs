//! "AI" tab — agent runtime configuration: Gemini transport
//! backend, model id, API key, and the execute_command allowlist.
//!
//! The key panel moved here from the Settings tab's "AI Assistant"
//! section (P1a); it keeps the same write-only contract — the
//! stored key is never loaded back into the UI, and saving writes
//! `~/.config/oxidemx/gemini.key` with 0600 perms.

use crate::{Message, State};
use iced::widget::{button, checkbox, column, container, pick_list, row, rule, text, text_input, toggler, Space};
use iced::{Alignment, Element, Length};
use oxidemx_shared::config::AiProvider;
use oxidemx_widgets::style;
use oxidemx_widgets::widgets::section_header;

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    let body = column![
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
    ]
    .spacing(16);

    // The local-server endpoint only applies to the MistralRs provider, so
    // it's shown contextually rather than cluttering every provider's view.
    let body = if state.config.overlay.ai.provider == AiProvider::MistralRs {
        body.push(section_block(state, "Local server endpoint", endpoint_panel(state)))
    } else {
        body
    };

    body.push(section_block(state, "API key", key_panel(state)))
        .push(section_block(state, "Hybrid routing", routing_panel(state)))
        .push(section_block(state, "Agent routing", agentd_panel(state)))
        .push(section_block(state, "Local Models", local_models_panel(state)))
        .push(section_block(state, "Command allowlist", allowlist_editor(state)))
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
         their API keys (below). Ollama and mistral.rs run local models \
         (no key) — pick mistral.rs to talk to a `mistralrs-server` over \
         its OpenAI-compatible API and set its URL in the endpoint field \
         that appears. Claude Code uses your `claude` CLI / subscription \
         (no key) but is chat-only — the agent tools (run command, \
         memory, …) don't apply on that path. Switching providers resets \
         the model to that provider's default.",
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
// Local server endpoint — MistralRs (OpenAI-compatible) only
// ============================================================================

fn endpoint_panel(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let current = &state.config.overlay.ai.local_endpoint;

    let intro = text(
        "Base URL of your mistral.rs server (a `mistralrs-server --port …` \
         instance). Must include the `/v1/` path and end with a trailing \
         slash — the OpenAI client joins `chat/completions` onto it. \
         Default: http://localhost:1234/v1/ .",
    )
    .size(11)
    .style(style::text_dim(pal));

    let input = text_input("http://localhost:1234/v1/", current)
        .on_input(Message::AiLocalEndpointChanged)
        .padding(6)
        .size(12)
        .width(Length::Fill);

    // Flag the trailing-slash trap inline so a bad URL fails loudly here
    // rather than silently 404ing at request time.
    let hint: Element<Message> = if current.ends_with('/') {
        text("Endpoint looks well-formed ✓")
            .size(11)
            .style(style::text_accent(pal))
            .into()
    } else {
        text("⚠ No trailing slash — the runtime appends one, but add it here to be explicit.")
            .size(11)
            .style(style::text_faint(pal))
            .into()
    };

    column![intro, input, hint].spacing(8).into()
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
    // Some backends (mistral.rs) have a key slot but don't require one —
    // a local server may or may not be authed.
    let optional = !provider.needs_key();
    let intro = text(format!(
        "API key for {}{}. Stored outside config.json at \
         ~/.config/oxidemx/{stem}.key (0600) — never included in config \
         exports. The {env} environment variable overrides it.",
        provider.label(),
        if optional { " (optional)" } else { "" },
    ))
    .size(11)
    .style(style::text_dim(pal));

    let status_line: Element<Message> = if state.ai_key_present {
        text("Key configured ✓ — paste a new one below to replace it.")
            .size(11)
            .style(style::text_accent(pal))
            .into()
    } else if optional {
        text("No key set — fine for an unauthenticated local server; add one if yours requires it.")
            .size(11)
            .style(style::text_faint(pal))
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
// Agent routing (agentd D-Bus path)
// ============================================================================

fn agentd_panel(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    let intro = text(
        "When on, overlay chat is routed through the agentd daemon over D-Bus. \
         Required for flow delivery (activity bubbles, auto-delivery). Turn off \
         only to force the legacy in-process path for debugging.",
    )
    .size(11)
    .style(style::text_dim(pal));

    // Route chat through agentd (required for flows: bubbles + auto-delivery).
    let agentd_row = row![
        text("Run agent through agentd (required for flows)").size(13),
        Space::new().width(Length::Fill),
        toggler(state.config.overlay.ai.use_agentd)
            .on_toggle(Message::AiUseAgentdToggled)
            .style(style::toggler_style(pal)),
    ]
    .align_y(Alignment::Center);

    // HTTP/SSE transport — remote access via tailnet or local HTTP.
    let http_row = row![
        text("Enable HTTP/SSE transport (remote access)").size(13),
        Space::new().width(Length::Fill),
        toggler(state.config.http.enabled)
            .on_toggle(Message::HttpTransportToggled)
            .style(style::toggler_style(pal)),
    ]
    .align_y(Alignment::Center);

    column![intro, agentd_row, http_row].spacing(8).into()
}

// ============================================================================
// Hybrid routing (Phase 4)
// ============================================================================

fn routing_panel(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let ai = &state.config.overlay.ai;

    let intro = text(
        "When on, a fast local model classifies each turn: simple questions it \
         answers itself (no cloud call — private, fast, free); anything needing \
         tools or deep reasoning escalates to the provider selected above. Off = \
         every turn uses that provider.",
    )
    .size(11)
    .style(style::text_dim(pal));

    let toggle = checkbox(ai.routing_enabled)
        .label("Enable hybrid routing")
        .on_toggle(Message::AiRoutingToggled)
        .size(16)
        .text_size(12);

    if !ai.routing_enabled {
        return column![intro, toggle].spacing(8).into();
    }

    // Fast/local provider — local options only.
    const LOCAL: [AiProvider; 2] = [AiProvider::MistralRs, AiProvider::Ollama];
    let provider_pick = pick_list(LOCAL, Some(ai.fast_provider), Message::AiFastProviderChanged)
        .text_size(12)
        .padding(6)
        .style(style::pick_list_style(pal));

    let known: Vec<String> = ai
        .fast_provider
        .model_suggestions()
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let selected = known.iter().find(|m| **m == ai.fast_model).cloned();
    let model_quick = pick_list(known, selected, Message::AiFastModelChanged)
        .placeholder("custom…")
        .text_size(12)
        .padding(6)
        .style(style::pick_list_style(pal));
    let model_custom = text_input("model id", &ai.fast_model)
        .on_input(Message::AiFastModelChanged)
        .padding(6)
        .size(12)
        .width(Length::Fill);

    let note = text(format!(
        "Fast model talks to {} at the endpoint configured above.",
        ai.fast_provider.label()
    ))
    .size(11)
    .style(style::text_faint(pal));

    column![
        intro,
        toggle,
        row![text("Fast provider:").size(12), provider_pick]
            .spacing(8)
            .align_y(Alignment::Center),
        row![model_quick, model_custom]
            .spacing(8)
            .align_y(Alignment::Center),
        note,
    ]
    .spacing(8)
    .into()
}

// ============================================================================
// Local Models — download directory + idle timeout
// ============================================================================

fn local_models_panel(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let lm = &state.config.overlay.local_models;

    let intro = text(
        "Where mistral.rs stores downloaded GGUF models, and how long \
         an idle model engine is kept loaded before being released. \
         Changes take effect when the local-model service next \
         starts or after a daemon reload.",
    )
    .size(11)
    .style(style::text_dim(pal));

    // Download directory row: text input + folder-picker button.
    let dir_input = text_input(
        "~/.local/share/oxidemx/models",
        &lm.download_dir.display().to_string(),
    )
    .on_input(Message::AiModelDirChanged)
    .padding(6)
    .size(12)
    .width(Length::Fill);

    let pick_btn = button(text("Choose folder…").size(11))
        .style(style::btn_primary(pal))
        .on_press(Message::AiModelDirPick);

    let dir_row = row![dir_input, pick_btn]
        .spacing(8)
        .align_y(Alignment::Center);

    // Idle timeout row.
    let timeout_label = text("Idle timeout (seconds):").size(12);
    let timeout_input = text_input(
        "600",
        &lm.idle_timeout_secs.to_string(),
    )
    .on_input(Message::AiIdleTimeoutChanged)
    .padding(6)
    .size(12)
    .width(Length::Fixed(120.0));

    let timeout_row = row![timeout_label, timeout_input]
        .spacing(8)
        .align_y(Alignment::Center);

    column![intro, dir_row, timeout_row].spacing(8).into()
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
