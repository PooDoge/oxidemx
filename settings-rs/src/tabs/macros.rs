//! "Macros" tab — list / inspect / delete the macros stored in
//! `~/.config/juhradial/macros/*.json`. Recording new macros lives
//! in the daemon (it captures evdev events on a separate code
//! path); this tab is the management surface — see what's stored,
//! tweak names / triggers via JSON, delete the ones you don't
//! want any more. A future iteration adds in-place name editing
//! and a "Record" launcher that talks to the daemon over D-Bus.

use crate::widgets::section_header;
use crate::{style, Message, State};
use iced::widget::{button, column, container, row, rule, text, Space};
use iced::{Alignment, Element, Length};
use serde::Deserialize;
use std::path::PathBuf;

/// Subset of the daemon's `MacroConfig` we care about for the
/// listing UI. Read-only here — `serde::Deserialize` only.
#[derive(Debug, Clone, Deserialize)]
pub struct MacroSummary {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub assigned_trigger: Option<String>,
    #[serde(default)]
    pub actions: Vec<serde_json::Value>,
}

/// Walk `~/.config/juhradial/macros/*.json` and parse each into
/// `MacroSummary`. Returns sorted by name. Errors are logged and
/// turned into an empty list — the tab handles that gracefully.
pub fn list() -> Vec<MacroSummary> {
    let dir = match macros_dir() {
        Some(d) => d,
        None => return Vec::new(),
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<MacroSummary> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension()?.to_str()? != "json" {
                return None;
            }
            let bytes = std::fs::read(&path).ok()?;
            serde_json::from_slice::<MacroSummary>(&bytes).ok()
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Macros directory: `~/.config/juhradial/macros/`. Mirrors the
/// daemon's `macros_dir()` so we read/write the same files.
pub fn macros_dir() -> Option<PathBuf> {
    juhradial_shared::config::default_config_path()
        .and_then(|p| p.parent().map(|p| p.join("macros")))
}

pub fn delete_macro(id: &str) -> Result<(), String> {
    let dir = macros_dir().ok_or_else(|| "no config dir".to_string())?;
    let path = dir.join(format!("{id}.json"));
    if !path.exists() {
        return Err(format!("not found: {}", path.display()));
    }
    std::fs::remove_file(&path).map_err(|e| format!("rm {}: {e}", path.display()))
}

// ============================================================================
// View
// ============================================================================

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let macros = &state.macros;

    // "Record" button intentionally has no on_press until the
    // daemon's StartMacroRecording bridge is wired into a UI flow.
    // iced disables the button visually when on_press is missing,
    // so users see it's a planned feature, not a working one.
    let header = row![
        section_header("Macros"),
        Space::new().width(Length::Fixed(12.0)),
        container(text("RECORDING — COMING SOON").size(9))
            .padding([2, 6])
            .style(style::chip(pal)),
        Space::new().width(Length::Fill),
        button(text("Record").size(11)).style(style::btn_secondary(pal)),
        button(text("Refresh").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::RefreshMacros),
        button(text("Open folder").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::OpenMacrosFolder),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    let intro = text(
        "Macros are stored as JSON files in ~/.config/juhradial/macros/. \
         Recording new macros happens in the daemon's record mode; this \
         page is the management surface — see what's stored, delete the \
         ones you don't want, and edit names / triggers in the JSON \
         directly until the in-app editor lands.",
    )
    .size(12)
    .style(style::text_dim(pal));

    let list_section: Element<Message> = if macros.is_empty() {
        container(
            column![
                text("No macros configured.")
                    .size(13)
                    .style(style::text_dim(pal)),
                text("Use the daemon's record mode to capture one, or drop \
                      a hand-edited .json into the macros folder.")
                .size(11)
                .style(style::text_faint(pal)),
            ]
            .spacing(6),
        )
        .padding(20)
        .style(style::card_quiet(pal))
        .into()
    } else {
        let mut col = column![].spacing(8);
        for m in macros {
            col = col.push(macro_card(state, m));
        }
        col.into()
    };

    column![
        header,
        intro,
        rule::horizontal(1).style(style::rule_style(pal)),
        Space::new().height(Length::Fixed(8.0)),
        list_section,
    ]
    .spacing(10)
    .into()
}

fn macro_card<'a>(state: &'a State, m: &'a MacroSummary) -> Element<'a, Message> {
    let pal = &state.palette;
    let trigger_text = m
        .assigned_trigger
        .as_deref()
        .unwrap_or("(no trigger)")
        .to_string();
    let action_count = m.actions.len();

    container(
        row![
            container(text("M").size(11).style(style::text_dim(pal)))
                .padding([4, 8])
                .style(style::chip(pal)),
            column![
                text(m.name.clone()).size(14),
                text(if m.description.is_empty() {
                    "(no description)".to_string()
                } else {
                    m.description.clone()
                })
                .size(11)
                .style(style::text_dim(pal)),
                row![
                    text(format!("{action_count} action{}", if action_count == 1 { "" } else { "s" }))
                        .size(11)
                        .style(style::text_faint(pal)),
                    text(" · ").size(11).style(style::text_faint(pal)),
                    text(trigger_text).size(11).style(style::text_faint(pal)),
                ]
                .spacing(0),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            button(text("Delete").size(11))
                .style(style::btn_danger(pal))
                .on_press(Message::DeleteMacro(m.id.clone())),
        ]
        .align_y(Alignment::Center)
        .spacing(12),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}
