//! "Macros" tab — list / inspect / delete the macros stored in
//! `~/.config/juhradial/macros/*.json`. Recording new macros lives
//! in the daemon (it captures evdev events on a separate code
//! path); this tab is the management surface — see what's stored,
//! tweak names / triggers via JSON, delete the ones you don't
//! want any more. A future iteration adds in-place name editing
//! and a "Record" launcher that talks to the daemon over D-Bus.

use juhradial_widgets::widgets::section_header;
use crate::{MacroEditField, Message, RecordingState, State};
use juhradial_widgets::style;
use iced::widget::{button, column, container, row, rule, text, text_input, Space};
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

/// Read the raw JSON of a macro file. Returned as
/// `serde_json::Value` so the UI can mutate just the user-editable
/// fields and write the result back unchanged.
pub fn read_raw(id: &str) -> Result<serde_json::Value, String> {
    let dir = macros_dir().ok_or_else(|| "no config dir".to_string())?;
    let path = dir.join(format!("{id}.json"));
    let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// Write the raw JSON of a macro file. Used by the in-place
/// editor; the daemon's SaveMacro D-Bus method takes the same
/// shape but requires a parsed `MacroConfig` round-trip and
/// re-validates triggers, which we don't want to do for a simple
/// rename. Atomic temp-file + rename mirrors the daemon's writer.
pub fn write_raw(id: &str, value: &serde_json::Value) -> Result<(), String> {
    let dir = macros_dir().ok_or_else(|| "no config dir".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let path = dir.join(format!("{id}.json"));
    let json = serde_json::to_string_pretty(value).map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
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

    // Record button label depends on the flow's state.
    let (record_label, record_chip) = match &state.recording {
        RecordingState::Idle => ("● Record", None),
        RecordingState::Recording => ("■ Stop", Some("RECORDING")),
        RecordingState::Naming { .. } => ("● Record", Some("NAMING")),
    };
    let record_btn_disabled = matches!(state.recording, RecordingState::Naming { .. });
    let mut record_btn = button(text(record_label.to_string()).size(11))
        .style(style::btn_secondary(pal));
    if !record_btn_disabled {
        record_btn = record_btn.on_press(Message::ToggleMacroRecord);
    }

    let mut header_row = row![section_header("Macros")];
    if let Some(chip_label) = record_chip {
        header_row = header_row
            .push(Space::new().width(Length::Fixed(12.0)))
            .push(
                container(text(chip_label.to_string()).size(9))
                    .padding([2, 6])
                    .style(style::chip(pal)),
            );
    }
    header_row = header_row
        .push(Space::new().width(Length::Fill))
        .push(record_btn)
        .push(
            button(text("Import…").size(11))
                .style(style::btn_secondary(pal))
                .on_press(Message::ImportMacro),
        )
        .push(
            button(text("Refresh").size(11))
                .style(style::btn_secondary(pal))
                .on_press(Message::RefreshMacros),
        )
        .push(
            button(text("Open folder").size(11))
                .style(style::btn_secondary(pal))
                .on_press(Message::OpenMacrosFolder),
        );
    let header = header_row.align_y(Alignment::Center).spacing(8);

    let intro = text(
        "Click Record, then trigger your shortcut sequence on the keyboard \
         or mouse. Click Stop and name the macro to save it. Stored as \
         JSON in ~/.config/juhradial/macros/ — open the folder for \
         hand-edits, or delete the ones you don't want.",
    )
    .size(12)
    .style(style::text_dim(pal));

    // Naming form sits above the list when present.
    let naming_section: Option<Element<Message>> = match &state.recording {
        RecordingState::Naming { name, .. } => Some(
            container(
                column![
                    text("Name your macro").size(14),
                    text("Choose something memorable — the name doubles as \
                          the file slug. Triggers can be assigned later via \
                          the JSON file.")
                    .size(11)
                    .style(style::text_dim(pal)),
                    text_input("e.g. \"open editor\"", name)
                        .on_input(Message::EditRecordedName)
                        .on_submit(Message::SaveRecordedMacro)
                        .padding(8)
                        .size(13)
                        .width(Length::Fill),
                    row![
                        button(text("Save").size(11))
                            .style(style::btn_secondary(pal))
                            .on_press(Message::SaveRecordedMacro),
                        button(text("Discard").size(11))
                            .style(style::btn_danger(pal))
                            .on_press(Message::DiscardRecordedMacro),
                    ]
                    .spacing(8),
                ]
                .spacing(10),
            )
            .padding(14)
            .style(style::card(pal))
            .into(),
        ),
        _ => None,
    };

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

    let mut col = column![header, intro, rule::horizontal(1).style(style::rule_style(pal))]
        .spacing(10);
    if let Some(naming) = naming_section {
        col = col.push(naming);
    }
    col.push(Space::new().height(Length::Fixed(8.0)))
        .push(list_section)
        .into()
}

fn macro_card<'a>(state: &'a State, m: &'a MacroSummary) -> Element<'a, Message> {
    let pal = &state.palette;
    let editing = state
        .macro_edit
        .as_ref()
        .filter(|d| d.id == m.id);

    if let Some(draft) = editing {
        return edit_card(state, draft, m);
    }

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
            button(text("Export").size(11))
                .style(style::btn_secondary(pal))
                .on_press(Message::ExportMacro(m.id.clone())),
            button(text("Edit").size(11))
                .style(style::btn_secondary(pal))
                .on_press(Message::StartEditMacro(m.id.clone())),
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

fn edit_card<'a>(
    state: &'a State,
    draft: &'a crate::MacroEditDraft,
    m: &'a MacroSummary,
) -> Element<'a, Message> {
    let pal = &state.palette;
    let id_for_name = draft.id.clone();
    let id_for_desc = draft.id.clone();
    let id_for_trig = draft.id.clone();
    let id_for_save = draft.id.clone();

    container(
        column![
            row![
                container(text("M").size(11).style(style::text_dim(pal)))
                    .padding([4, 8])
                    .style(style::chip(pal)),
                text(format!("Editing \"{}\"", m.name)).size(14),
                Space::new().width(Length::Fill),
                button(text("Cancel").size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::CancelMacroEdit),
                button(text("Save").size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::CommitMacroEdit(id_for_save)),
            ]
            .align_y(Alignment::Center)
            .spacing(8),
            text_input("Name", &draft.name)
                .on_input(move |v| Message::EditMacroField {
                    id: id_for_name.clone(),
                    field: MacroEditField::Name,
                    value: v,
                })
                .padding(6)
                .size(12),
            text_input("Description", &draft.description)
                .on_input(move |v| Message::EditMacroField {
                    id: id_for_desc.clone(),
                    field: MacroEditField::Description,
                    value: v,
                })
                .padding(6)
                .size(12),
            text_input(
                "Assigned trigger (e.g. \"button:0x53\" — leave blank for none)",
                &draft.trigger,
            )
            .on_input(move |v| Message::EditMacroField {
                id: id_for_trig.clone(),
                field: MacroEditField::Trigger,
                value: v,
            })
            .padding(6)
            .size(12),
            text(
                "Trigger format follows the daemon's macro::triggers parser. \
                 Recorded actions stay unchanged — open the JSON file to \
                 edit those by hand.",
            )
            .size(10)
            .style(style::text_faint(pal)),
        ]
        .spacing(8),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}
