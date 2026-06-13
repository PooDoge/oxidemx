//! "Agents" tab — the conductor's management surface (spec §4.3).
//!
//! Browses the multi-agent system: flows (with live validation), the
//! roster, the tool registry, and configured MCP servers — and
//! launches a flow into Mission Control. Read-mostly for v1; authoring
//! (the flow/roster wizard) is the P5 surface. Listing + validation
//! reuse the conductor's own `loader`/`validate` (single source of
//! truth — the exact same parse the CLI and Mission Control use).

use std::path::PathBuf;

use iced::widget::{button, column, container, row, rule, text, text_input, Space};
use iced::{Alignment, Element, Length};
use oxidemx_conductor::loader::{default_agents_root, default_flows_root, list_flows, load_flow, load_roster};
use oxidemx_conductor::{validate, KNOWN_TOOLS};
use oxidemx_widgets::style;
use oxidemx_widgets::widgets::section_header;

use crate::{Message, State};

/// One flow row: identity + validation result.
#[derive(Debug, Clone)]
pub struct FlowRow {
    pub id: String,
    pub name: String,
    pub step_order: Vec<String>,
    pub valid: bool,
    pub errors: Vec<String>,
}

/// One roster agent row.
#[derive(Debug, Clone)]
pub struct AgentRow {
    pub id: String,
    pub name: String,
    pub model: String,
    pub tools: Vec<String>,
    pub capabilities: Vec<String>,
}

/// All Agents-tab data, scanned from disk.
#[derive(Debug, Clone, Default)]
pub struct AgentsData {
    pub flows: Vec<FlowRow>,
    pub agents: Vec<AgentRow>,
    pub mcp_path: Option<PathBuf>,
    pub mcp_exists: bool,
    pub mcp_server_count: usize,
}

impl AgentsData {
    /// Scan `~/.config/oxidemx/{flows,agents}` + the MCP config.
    pub fn load() -> Self {
        let flows_root = default_flows_root();
        let agents_root = default_agents_root();

        let mut flows = Vec::new();
        for id in list_flows(&flows_root) {
            match load_flow(&flows_root, &agents_root, &id) {
                Ok((doc, roster)) => {
                    let name = doc.name().to_string();
                    match validate(&doc, &roster, KNOWN_TOOLS) {
                        Ok(plan) => flows.push(FlowRow {
                            id,
                            name,
                            step_order: plan.topo.clone(),
                            valid: true,
                            errors: Vec::new(),
                        }),
                        Err(errs) => flows.push(FlowRow {
                            id,
                            name,
                            step_order: doc.manifest.steps.iter().map(|s| s.id.clone()).collect(),
                            valid: false,
                            errors: errs.iter().map(|e| e.to_string()).collect(),
                        }),
                    }
                }
                Err(e) => flows.push(FlowRow {
                    id: id.clone(),
                    name: id,
                    step_order: Vec::new(),
                    valid: false,
                    errors: vec![e.to_string()],
                }),
            }
        }

        let roster = load_roster(&agents_root).unwrap_or_default();
        let mut agents: Vec<AgentRow> = roster
            .ids()
            .filter_map(|id| roster.get(id))
            .map(|def| AgentRow {
                id: def.decl.id.clone(),
                name: def.name().to_string(),
                model: def.decl.model.clone().unwrap_or_else(|| "(flow default)".into()),
                tools: def.decl.tools.clone(),
                capabilities: def.decl.capabilities.clone(),
            })
            .collect();
        agents.sort_by(|a, b| a.id.cmp(&b.id));

        let mcp_path = oxidemx_agent::toolkit::mcp::default_config_path();
        let (mcp_exists, mcp_server_count) = mcp_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| (true, s.matches("[[mcp.servers]]").count()))
            .unwrap_or((false, 0));

        AgentsData {
            flows,
            agents,
            mcp_path,
            mcp_exists,
            mcp_server_count,
        }
    }
}

/// Spawn Mission Control, pre-selecting `flow_id` (read via
/// `OXIDEMX_MC_FLOW`). Fire-and-forget; a missing binary is silently
/// ignored (the tab still works for browsing).
pub fn launch_mission_control(flow_id: &str) {
    let _ = std::process::Command::new("oxidemx-mission-control")
        .env("OXIDEMX_MC_FLOW", flow_id)
        .spawn();
}

/// Sanitize a user-typed flow id to a safe directory slug
/// (lowercase, alnum + dashes).
fn slugify(raw: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Scaffold a runnable starter flow at `<flows_root>/<id>/flow.md`. The
/// template uses the shipped roster agents (so it validates + runs
/// immediately) and is meant to be hand-refined afterward. Errors on a
/// bad id or an existing flow (won't clobber).
pub fn scaffold_flow(raw_id: &str) -> Result<String, String> {
    let id = slugify(raw_id);
    if id.is_empty() {
        return Err("enter a flow name (letters, numbers, dashes)".into());
    }
    let dir = default_flows_root().join(&id);
    let flow_md = dir.join("flow.md");
    if flow_md.exists() {
        return Err(format!("flow `{id}` already exists"));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let template = format!(
        r#"---
[flow]
id = "{id}"
name = "{id}"
description = "Describe what this flow does."
version = 1

[inputs]
topic = {{ type = "string", default = "the AutoAgents changelog" }}

[defaults]
model = "gemini-2.5-flash"
executor = "react"
approval = "allowlist"
max_turns = 8

[[step]]
id = "research"
agent = "web-researcher"
task = "Gather what you can about {{{{input.topic}}}}."
output = "debug/research.md"

[[step]]
id = "answer"
agent = "writer"
needs = ["research"]
task = "Write a clear, grounded summary about {{{{input.topic}}}}."
context = ["@artifact@"]
output = "ANSWER.md"

[delivery]
root = "ANSWER.md"
title = "{id}"
---

# {id}

A starter flow. Edit this file to shape the pipeline: add `[[step]]`
tables, wire them with `needs`, grant tools via the roster agents, and
use `kind = "reflect"` / `kind = "route"` for review loops and
branches. Validate + run it from the Agents tab.
"#
    );
    std::fs::write(&flow_md, template).map_err(|e| format!("cannot write flow.md: {e}"))?;
    Ok(id)
}

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let a = &state.agents;

    let header = row![
        section_header("Agents"),
        Space::new().width(Length::Fill),
        button(text("Refresh").size(12))
            .padding([4, 10])
            .on_press(Message::AgentsRefresh)
            .style(style::btn_secondary(pal)),
    ]
    .align_y(Alignment::Center);

    let intro = text(
        "Multi-agent flows run by the conductor. Author flows + roster \
         agents as markdown under ~/.config/oxidemx/{flows,agents}; this \
         tab lists them with live validation and launches them into \
         Mission Control. The same loader/validator the CLI uses.",
    )
    .size(12)
    .style(style::text_dim(pal));

    let empty_hint: Element<Message> = if a.flows.is_empty() && a.agents.is_empty() {
        text(
            "Nothing found yet. Copy the starter pack from the repo's \
             oxidemx-conductor/{flows,agents}/ into ~/.config/oxidemx/.",
        )
        .size(11)
        .style(style::text_faint(pal))
        .into()
    } else {
        Space::new().height(Length::Fixed(0.0)).into()
    };

    column![
        header,
        intro,
        new_flow_bar(state),
        rule::horizontal(1).style(style::rule_style(pal)),
        flows_card(state),
        roster_card(state),
        tools_card(state),
        mcp_card(state),
        empty_hint,
    ]
    .spacing(16)
    .into()
}

fn new_flow_bar(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let input = text_input("new-flow-name", &state.agents_new_flow_draft)
        .on_input(Message::AgentsNewFlowDraft)
        .on_submit(Message::AgentsCreateFlow)
        .padding(6)
        .size(12)
        .width(Length::Fixed(220.0));
    let create = button(text("New flow").size(12))
        .padding([6, 12])
        .on_press(Message::AgentsCreateFlow)
        .style(style::btn_secondary(pal));
    let mut bar = row![input, create].spacing(8).align_y(Alignment::Center);
    if !state.agents_new_flow_status.is_empty() {
        bar = bar.push(
            text(state.agents_new_flow_status.clone())
                .size(11)
                .style(style::text_dim(pal)),
        );
    }
    bar.into()
}

fn card<'a>(state: &'a State, title: &str, body: Element<'a, Message>) -> Element<'a, Message> {
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

fn flows_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let mut col = column![].spacing(12);
    if state.agents.flows.is_empty() {
        col = col.push(text("No flows.").size(11).style(style::text_faint(pal)));
    }
    for f in &state.agents.flows {
        let badge = if f.valid {
            text("✓ valid").size(11).style(text_color(pal.success))
        } else {
            text(format!("✗ {} error(s)", f.errors.len()))
                .size(11)
                .style(text_color(pal.danger))
        };
        let head = row![
            text(f.name.clone()).size(13).style(style::text_accent(pal)),
            Space::new().width(Length::Fill),
            badge,
            button(text("Open in Mission Control").size(11))
                .padding([3, 9])
                .on_press(Message::AgentsRunFlow(f.id.clone()))
                .style(style::btn_primary(pal)),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let order = if f.step_order.is_empty() {
            "—".to_string()
        } else {
            format!("{} steps:  {}", f.step_order.len(), f.step_order.join(" → "))
        };
        let mut block = column![head, text(order).size(11).style(style::text_dim(pal))].spacing(4);
        for e in &f.errors {
            block = block.push(text(format!("• {e}")).size(10).style(text_color(pal.danger)));
        }
        col = col.push(
            container(block)
                .padding([8, 10])
                .width(Length::Fill)
                .style(style::card_quiet(pal)),
        );
    }
    card(state, "Flows", col.into())
}

fn roster_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let mut col = column![].spacing(10);
    if state.agents.agents.is_empty() {
        col = col.push(text("No roster agents.").size(11).style(style::text_faint(pal)));
    }
    for ag in &state.agents.agents {
        let tools = if ag.tools.is_empty() {
            "no tools".to_string()
        } else {
            ag.tools.join(", ")
        };
        let caps = if ag.capabilities.is_empty() {
            String::new()
        } else {
            format!("  ·  {}", ag.capabilities.join(", "))
        };
        let block = column![
            row![
                text(ag.name.clone()).size(13).style(style::text_accent(pal)),
                Space::new().width(Length::Fill),
                text(ag.model.clone()).size(10).style(style::text_faint(pal)),
            ]
            .align_y(Alignment::Center),
            text(format!("tools: {tools}{caps}")).size(11).style(style::text_dim(pal)),
        ]
        .spacing(3);
        col = col.push(
            container(block)
                .padding([7, 10])
                .width(Length::Fill)
                .style(style::card_quiet(pal)),
        );
    }
    card(state, "Roster", col.into())
}

fn tools_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let mut col = column![text(
        "Tools a roster agent may be granted (via its `tools = [...]`). \
         The AutoAgents Toolkit + our execute_command."
    )
    .size(11)
    .style(style::text_dim(pal))]
    .spacing(6);
    let mut line = String::new();
    for (i, t) in KNOWN_TOOLS.iter().enumerate() {
        let note = if *t == "brave_search" {
            " (needs key)"
        } else if oxidemx_agent::toolkit::is_mutating(t) {
            " (mutating)"
        } else {
            ""
        };
        line.push_str(&format!("{t}{note}"));
        if i + 1 < KNOWN_TOOLS.len() {
            line.push_str("   ");
        }
    }
    col = col.push(text(line).size(11).style(style::text_dim(pal)));
    card(state, "Tool registry", col.into())
}

fn mcp_card(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let a = &state.agents;
    let path = a
        .mcp_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.config/oxidemx/mcp.toml".into());
    let status: Element<Message> = if a.mcp_exists {
        text(format!("{} server(s) configured in {path}", a.mcp_server_count))
            .size(11)
            .style(style::text_dim(pal))
            .into()
    } else {
        column![
            text("No MCP config. Add servers to expose their tools to agents.")
                .size(11)
                .style(style::text_dim(pal)),
            text(format!("Create {path} (see mcp.toml.example)."))
                .size(10)
                .style(style::text_faint(pal)),
        ]
        .spacing(3)
        .into()
    };
    card(state, "MCP servers", status)
}

fn text_color(c: iced::Color) -> impl Fn(&iced::Theme) -> iced::widget::text::Style {
    move |_| iced::widget::text::Style { color: Some(c) }
}

#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn slugify_makes_safe_dir_names() {
        assert_eq!(slugify("Research Digest"), "research-digest");
        assert_eq!(slugify("  My  Cool Flow!! "), "my-cool-flow");
        assert_eq!(slugify("already-ok"), "already-ok");
        assert_eq!(slugify("a/b\\c"), "a-b-c");
        assert_eq!(slugify("   "), "");
        assert_eq!(slugify("---"), "");
    }
}
