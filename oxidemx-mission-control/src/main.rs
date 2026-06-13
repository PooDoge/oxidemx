//! OxideMX Mission Control — the live multi-agent flow monitor (P4,
//! spec §4.2). A standalone iced window that embeds the conductor as a
//! library: pick a flow, run it (mock or a real provider), and watch
//! the run-layer event stream (§11) render live — per-step status, an
//! event console, and the artifacts produced.
//!
//! The conductor's `EventSink` trait + an `async_channel` is the whole
//! bridge: a run is driven *inside* an iced `Task::stream` (so it runs
//! on iced's tokio executor — no manual spawn, no agentd) while its
//! events flow back as `Message::Event`. Cancellation is the run's
//! `CancellationToken`, held in app state.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use futures_util::{Stream, StreamExt};
use iced::widget::{button, column, container, row, rule, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length, Task};

use oxidemx_conductor::event::{EventSink, RunEvent};
use oxidemx_conductor::loader::{default_agents_root, default_flows_root, list_flows, load_flow};
use oxidemx_conductor::mock::MockProvider;
use oxidemx_conductor::plan::FlowPlan;
use oxidemx_conductor::roster::Roster;
use oxidemx_conductor::supervisor::{
    resolve_inputs, run_flow, ConfigFactory, FixedFactory, ProviderFactory, RunOptions,
};
use oxidemx_conductor::{validate, KNOWN_TOOLS};
use oxidemx_shared::config::AiProvider;
use oxidemx_widgets::palette::Palette;
use oxidemx_widgets::style;
use oxidemx_widgets::widgets::section_header;
use tokio_util::sync::CancellationToken;

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("OxideMX Mission Control")
        .window(iced::window::Settings {
            size: iced::Size::new(1000.0, 700.0),
            min_size: Some(iced::Size::new(760.0, 520.0)),
            ..Default::default()
        })
        .theme(theme)
        .run()
}

// ── status model ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

impl Status {
    fn glyph(self) -> &'static str {
        match self {
            Status::Pending => "○",
            Status::Running => "◐",
            Status::Done => "●",
            Status::Failed => "✗",
            Status::Skipped => "⊘",
        }
    }
    fn color(self, p: &Palette) -> iced::Color {
        match self {
            Status::Pending => p.overlay0,
            Status::Running => p.accent,
            Status::Done => p.success,
            Status::Failed => p.danger,
            Status::Skipped => p.hairline_strong,
        }
    }
}

#[derive(Debug, Clone)]
struct StepView {
    id: String,
    kind: String,
    status: Status,
    detail: String,
}

// ── app state ────────────────────────────────────────────────────────

struct App {
    palette: Palette,
    flows_root: PathBuf,
    agents_root: PathBuf,
    flows: Vec<String>,
    selected: Option<String>,
    plan: Option<FlowPlan>,
    roster: Option<Roster>,
    validation: Vec<String>,
    inputs: Vec<(String, String)>,
    use_mock: bool,
    gemini_key: bool,

    run_id: Option<String>,
    running: bool,
    cancel: Option<CancellationToken>,
    steps: Vec<StepView>,
    artifacts: Vec<String>,
    console: Vec<String>,
    outcome: Option<Result<usize, String>>,
}

#[derive(Debug, Clone)]
enum Message {
    SelectFlow(String),
    SetInput(usize, String),
    ToggleMock,
    Refresh,
    Run,
    Cancel,
    Event(RunEvent),
}

fn boot() -> (App, Task<Message>) {
    let palette = active_palette();
    let flows_root = default_flows_root();
    let agents_root = default_agents_root();
    let flows = list_flows(&flows_root);
    let gemini_key = oxidemx_agent::keys::provider_key(AiProvider::Gemini).is_some();
    let app = App {
        palette,
        flows_root,
        agents_root,
        flows,
        selected: None,
        plan: None,
        roster: None,
        validation: Vec::new(),
        inputs: Vec::new(),
        use_mock: !gemini_key, // default to mock when no real key
        gemini_key,
        run_id: None,
        running: false,
        cancel: None,
        steps: Vec::new(),
        artifacts: Vec::new(),
        console: Vec::new(),
        outcome: None,
    };
    (app, Task::none())
}

/// Resolve the palette from the user's configured theme, falling back
/// to the design-system default.
fn active_palette() -> Palette {
    oxidemx_shared::config::default_config_path()
        .and_then(|p| oxidemx_shared::AppConfig::load_from(&p).ok())
        .map(|c| Palette::resolve(&c.theme))
        .unwrap_or_else(|| Palette::resolve_named("OxideMX MX"))
}

fn theme(state: &App) -> iced::Theme {
    let p = &state.palette;
    iced::Theme::custom(
        "OxideMX Mission Control".to_string(),
        iced::theme::Palette {
            background: p.base,
            text: p.text,
            primary: p.accent,
            success: p.success,
            warning: p.warning,
            danger: p.danger,
        },
    )
}

// ── update ───────────────────────────────────────────────────────────

fn update(state: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::Refresh => {
            state.flows = list_flows(&state.flows_root);
            Task::none()
        }
        Message::SelectFlow(id) => {
            select_flow(state, &id);
            Task::none()
        }
        Message::SetInput(i, v) => {
            if let Some(slot) = state.inputs.get_mut(i) {
                slot.1 = v;
            }
            Task::none()
        }
        Message::ToggleMock => {
            // Only allow leaving mock if a real key exists.
            if state.use_mock && !state.gemini_key {
                state
                    .console
                    .push("no Gemini key — add one in Settings → AI to run real flows".into());
            } else {
                state.use_mock = !state.use_mock;
            }
            Task::none()
        }
        Message::Cancel => {
            if let Some(c) = &state.cancel {
                c.cancel();
            }
            Task::none()
        }
        Message::Run => start_run(state),
        Message::Event(ev) => {
            apply_event(state, ev);
            Task::none()
        }
    }
}

fn select_flow(state: &mut App, id: &str) {
    state.selected = Some(id.to_string());
    state.validation.clear();
    state.inputs.clear();
    state.plan = None;
    state.roster = None;
    // Clear any prior run view.
    state.steps.clear();
    state.artifacts.clear();
    state.console.clear();
    state.outcome = None;
    state.run_id = None;

    match load_flow(&state.flows_root, &state.agents_root, id) {
        Ok((doc, roster)) => {
            // Seed the input form from the declared inputs + defaults.
            state.inputs = doc
                .manifest
                .inputs
                .iter()
                .map(|(k, spec)| (k.clone(), spec.default.clone().unwrap_or_default()))
                .collect();
            match validate(&doc, &roster, KNOWN_TOOLS) {
                Ok(plan) => {
                    state.plan = Some(plan);
                    state.roster = Some(roster);
                }
                Err(errors) => {
                    state.validation = errors.iter().map(|e| e.to_string()).collect();
                }
            }
        }
        Err(e) => state.validation = vec![e.to_string()],
    }
}

fn start_run(state: &mut App) -> Task<Message> {
    let (Some(plan), Some(roster)) = (state.plan.clone(), state.roster.clone()) else {
        return Task::none();
    };
    if state.running {
        return Task::none();
    }

    let provided: BTreeMap<String, String> = state
        .inputs
        .iter()
        .filter(|(_, v)| !v.trim().is_empty())
        .cloned()
        .collect();
    let inputs = match resolve_inputs(&plan, &provided) {
        Ok(i) => i,
        Err(missing) => {
            state
                .console
                .push(format!("✗ missing required input(s): {}", missing.join(", ")));
            return Task::none();
        }
    };

    let factory: Arc<dyn ProviderFactory> = if state.use_mock {
        Arc::new(FixedFactory(MockProvider::echoing()))
    } else {
        match oxidemx_agent::keys::provider_key(AiProvider::Gemini) {
            Some(key) => Arc::new(ConfigFactory {
                provider: AiProvider::Gemini,
                api_key: key,
            }),
            None => {
                state.use_mock = true;
                state.console.push("no Gemini key — running with the mock provider".into());
                Arc::new(FixedFactory(MockProvider::echoing()))
            }
        }
    };

    let flow_id = plan.doc.manifest.flow.id.clone();
    let run_id = format!("{flow_id}-{}", now_millis());
    let workdir = runs_root().join(&run_id);
    let _ = std::fs::create_dir_all(&workdir);

    let approval = oxidemx_conductor::approval::ApprovalPolicy::parse(
        &plan.doc.manifest.defaults.approval,
    );
    let cancel = CancellationToken::new();

    // Reset the run view.
    state.steps = plan
        .topo
        .iter()
        .map(|id| {
            let kind = plan.step(id).map(|s| s.kind.clone()).unwrap_or_default();
            StepView {
                id: id.clone(),
                kind,
                status: Status::Pending,
                detail: String::new(),
            }
        })
        .collect();
    state.artifacts.clear();
    state.console.clear();
    state.outcome = None;
    state.run_id = Some(run_id.clone());
    state.running = true;
    state.cancel = Some(cancel.clone());

    let opts = RunOptions {
        run_id,
        inputs,
        workdir,
        roster,
        factory,
        cancel,
        approval,
        allowlist: load_allowlist(),
    };

    Task::stream(run_stream(plan, opts))
}

fn apply_event(state: &mut App, ev: RunEvent) {
    // Console line (compact).
    state.console.push(console_line(&ev));
    if state.console.len() > 300 {
        let drop = state.console.len() - 300;
        state.console.drain(0..drop);
    }

    let set = |steps: &mut Vec<StepView>, id: &str, status: Status, detail: Option<String>| {
        if let Some(s) = steps.iter_mut().find(|s| s.id == id) {
            s.status = status;
            if let Some(d) = detail {
                s.detail = d;
            }
        }
    };

    match ev {
        RunEvent::RunStarted { .. } => {}
        RunEvent::TaskAssigned { step, agent } => {
            set(&mut state.steps, &step, Status::Pending, Some(format!("→ {agent}")));
        }
        RunEvent::TaskStarted { step } => {
            set(&mut state.steps, &step, Status::Running, None);
        }
        RunEvent::AgentMessage { step, message } => {
            set(&mut state.steps, &step, Status::Running, Some(truncate(&message, 90)));
        }
        RunEvent::TaskFinished {
            step,
            artifact,
            summary,
            ..
        } => {
            set(&mut state.steps, &step, Status::Done, Some(truncate(&summary, 90)));
            if let Some(a) = artifact {
                if !state.artifacts.contains(&a) {
                    state.artifacts.push(a);
                }
            }
        }
        RunEvent::TaskError { step, error } => {
            set(&mut state.steps, &step, Status::Failed, Some(truncate(&error, 120)));
        }
        RunEvent::StepRetrying { step, attempt } => {
            set(&mut state.steps, &step, Status::Running, Some(format!("retry #{attempt}")));
        }
        RunEvent::StepSkipped { step, reason } => {
            set(&mut state.steps, &step, Status::Skipped, Some(reason));
        }
        RunEvent::ApprovalRequested { step, .. } => {
            set(&mut state.steps, &step, Status::Running, Some("awaiting approval".into()));
        }
        RunEvent::RunFinished { artifacts, .. } => {
            state.running = false;
            state.cancel = None;
            state.artifacts = artifacts;
            state.outcome = Some(Ok(state.artifacts.len()));
        }
        RunEvent::RunFailed { reason, step, .. } => {
            state.running = false;
            state.cancel = None;
            if let Some(s) = step {
                set(&mut state.steps, &s, Status::Failed, None);
            }
            state.outcome = Some(Err(reason));
        }
        RunEvent::RunCancelled { .. } => {
            state.running = false;
            state.cancel = None;
            state.outcome = Some(Err("cancelled".into()));
        }
    }
}

// ── the run → iced event bridge ──────────────────────────────────────

/// A sink that forwards every run event onto an async channel.
struct ChannelSink {
    tx: async_channel::Sender<RunEvent>,
}

#[async_trait::async_trait]
impl EventSink for ChannelSink {
    async fn emit(&self, event: RunEvent) {
        let _ = self.tx.send(event).await;
    }
}

/// Drive a run inside a stream: a once-future runs the conductor (on
/// iced's tokio executor) while the event receiver yields `Event`s.
/// When the run returns, the sink drops, the channel closes, and the
/// stream ends.
fn run_stream(plan: FlowPlan, opts: RunOptions) -> impl Stream<Item = Message> {
    let (tx, rx) = async_channel::unbounded::<RunEvent>();
    let sink: Arc<dyn EventSink> = Arc::new(ChannelSink { tx });
    let driver = futures_util::stream::once(async move {
        let _ = run_flow(&plan, opts, sink).await;
    })
    .filter_map(|_| async { None::<Message> });
    let events = rx.map(Message::Event);
    futures_util::stream::select(events, driver)
}

// ── view ─────────────────────────────────────────────────────────────

fn view(state: &App) -> Element<'_, Message> {
    let pal = &state.palette;
    let body = row![sidebar(state), main_panel(state)].spacing(0);
    container(body)
        .style(style::window(pal))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn sidebar(state: &App) -> Element<'_, Message> {
    let pal = &state.palette;
    let header = row![
        section_header("Flows"),
        Space::new().width(Length::Fill),
        button(text("↻").size(13))
            .padding([2, 8])
            .on_press(Message::Refresh)
            .style(style::btn_flat(pal)),
    ]
    .align_y(Alignment::Center);
    let mut col = column![header].spacing(10).padding(16);

    if state.flows.is_empty() {
        col = col.push(
            text("No flows found. Author them under ~/.config/oxidemx/flows/<id>/flow.md.")
                .size(11)
                .style(style::text_dim(pal)),
        );
    }
    for id in &state.flows {
        let selected = state.selected.as_deref() == Some(id.as_str());
        let label = text(id)
            .size(13)
            .style(text_color(if selected { pal.accent } else { pal.subtext0 }));
        col = col.push(
            button(label)
                .width(Length::Fill)
                .padding([6, 8])
                .on_press(Message::SelectFlow(id.clone()))
                .style(style::btn_flat(pal)),
        );
    }

    col = col.push(rule::horizontal(1).style(style::rule_style(pal)));

    if state.selected.is_some() {
        col = col.push(inputs_form(state));
        col = col.push(controls(state));
    }
    if !state.validation.is_empty() {
        col = col.push(
            text("Validation errors").size(12).style(style::text_accent(pal)),
        );
        for e in &state.validation {
            col = col.push(text(e).size(10).style(text_color(pal.danger)));
        }
    }

    container(scrollable(col).height(Length::Fill).style(style::scrollable_style(pal)))
        .width(Length::Fixed(300.0))
        .height(Length::Fill)
        .style(style::sidebar(pal))
        .into()
}

fn inputs_form(state: &App) -> Element<'_, Message> {
    let pal = &state.palette;
    let mut col = column![text("Inputs").size(12).style(style::text_dim(pal))].spacing(8);
    if state.inputs.is_empty() {
        col = col.push(text("(no inputs)").size(11).style(style::text_faint(pal)));
    }
    for (i, (key, val)) in state.inputs.iter().enumerate() {
        col = col.push(
            column![
                text(key).size(11).style(style::text_dim(pal)),
                text_input("", val)
                    .on_input(move |s| Message::SetInput(i, s))
                    .padding(6)
                    .size(12),
            ]
            .spacing(3),
        );
    }
    col.into()
}

fn controls(state: &App) -> Element<'_, Message> {
    let pal = &state.palette;
    let provider_label = if state.use_mock { "Provider: Mock" } else { "Provider: Gemini" };
    let provider_btn = button(text(provider_label).size(12))
        .padding([6, 10])
        .on_press(Message::ToggleMock)
        .style(style::btn_secondary(pal));

    let action: Element<Message> = if state.running {
        button(text("Cancel").size(13))
            .padding([7, 14])
            .on_press(Message::Cancel)
            .style(style::btn_danger(pal))
            .into()
    } else {
        let mut b = button(text("Run flow").size(13))
            .padding([7, 14])
            .style(style::btn_primary(pal));
        if state.plan.is_some() {
            b = b.on_press(Message::Run);
        }
        b.into()
    };

    column![
        rule::horizontal(1).style(style::rule_style(pal)),
        provider_btn,
        action,
    ]
    .spacing(10)
    .into()
}

fn main_panel(state: &App) -> Element<'_, Message> {
    let pal = &state.palette;

    let title = state
        .selected
        .as_deref()
        .unwrap_or("Select a flow to begin");
    let status_pill = run_status_pill(state);
    let header = row![
        text(title).size(18).style(style::text_accent(pal)),
        Space::new().width(Length::Fill),
        status_pill,
    ]
    .align_y(Alignment::Center)
    .spacing(10);

    let steps_panel = step_list(state);
    let lower = row![artifacts_panel(state), console_panel(state)].spacing(12);

    let content = column![
        header,
        rule::horizontal(1).style(style::rule_style(pal)),
        steps_panel,
        lower,
    ]
    .spacing(14)
    .padding(20);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::page(pal))
        .into()
}

fn run_status_pill(state: &App) -> Element<'_, Message> {
    let pal = &state.palette;
    let (label, color) = if state.running {
        ("running", pal.accent)
    } else {
        match &state.outcome {
            Some(Ok(n)) => return pill(pal, &format!("done · {n} artifacts"), pal.success),
            Some(Err(e)) if e == "cancelled" => ("cancelled", pal.warning),
            Some(Err(_)) => ("failed", pal.danger),
            None => ("idle", pal.overlay0),
        }
    };
    pill(pal, label, color)
}

fn pill<'a>(pal: &Palette, label: &str, color: iced::Color) -> Element<'a, Message> {
    container(text(label.to_string()).size(11).style(text_color(color)))
        .padding([3, 10])
        .style(style::chip(pal))
        .into()
}

fn step_list(state: &App) -> Element<'_, Message> {
    let pal = &state.palette;
    let mut col = column![text("Steps").size(12).style(style::text_dim(pal))].spacing(6);
    if state.steps.is_empty() {
        col = col.push(
            text("Run a flow to watch its steps execute here.")
                .size(12)
                .style(style::text_faint(pal)),
        );
    }
    for s in &state.steps {
        let glyph = text(s.status.glyph()).size(15).style(text_color(s.status.color(pal)));
        let kind_tag: Element<Message> = if s.kind == "agent" {
            Space::new().into()
        } else {
            container(text(s.kind.clone()).size(9).style(style::text_faint(pal)))
                .padding([1, 5])
                .style(style::chip(pal))
                .into()
        };
        let rowy = row![
            glyph,
            text(s.id.clone()).size(13).style(style::text_dim(pal)),
            kind_tag,
            Space::new().width(Length::Fill),
            text(s.detail.clone()).size(11).style(style::text_faint(pal)),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        col = col.push(
            container(rowy)
                .padding([5, 8])
                .width(Length::Fill)
                .style(style::card_quiet(pal)),
        );
    }
    container(scrollable(col).height(Length::Fill).style(style::scrollable_style(pal)))
        .height(Length::FillPortion(3))
        .into()
}

fn artifacts_panel(state: &App) -> Element<'_, Message> {
    let pal = &state.palette;
    let mut col = column![text("Artifacts").size(12).style(style::text_dim(pal))].spacing(4);
    if state.artifacts.is_empty() {
        col = col.push(text("—").size(11).style(style::text_faint(pal)));
    }
    for a in &state.artifacts {
        col = col.push(text(a.clone()).size(11).style(style::text_dim(pal)));
    }
    container(scrollable(col).style(style::scrollable_style(pal)))
        .width(Length::FillPortion(1))
        .height(Length::FillPortion(2))
        .padding(10)
        .style(style::card(pal))
        .into()
}

fn console_panel(state: &App) -> Element<'_, Message> {
    let pal = &state.palette;
    let mut col = column![text("Event console").size(12).style(style::text_dim(pal))].spacing(2);
    for line in &state.console {
        col = col.push(text(line.clone()).size(10).style(style::text_faint(pal)));
    }
    container(scrollable(col).style(style::scrollable_style(pal)))
        .width(Length::FillPortion(2))
        .height(Length::FillPortion(2))
        .padding(10)
        .style(style::card(pal))
        .into()
}

// ── helpers ──────────────────────────────────────────────────────────

fn text_color(c: iced::Color) -> impl Fn(&iced::Theme) -> text::Style {
    move |_| text::Style { color: Some(c) }
}

fn console_line(ev: &RunEvent) -> String {
    match ev {
        RunEvent::RunStarted { flow_id, steps, .. } => {
            format!("▶ run_started {flow_id} ({} steps)", steps.len())
        }
        RunEvent::TaskAssigned { step, agent } => format!("· assigned {step} → {agent}"),
        RunEvent::TaskStarted { step } => format!("· started {step}"),
        RunEvent::AgentMessage { step, message } => format!("  {step}: {}", truncate(message, 100)),
        RunEvent::TaskFinished { step, summary, .. } => {
            format!("✓ finished {step}: {}", truncate(summary, 80))
        }
        RunEvent::TaskError { step, error } => format!("✗ error {step}: {}", truncate(error, 100)),
        RunEvent::StepRetrying { step, attempt } => format!("↻ retry {step} #{attempt}"),
        RunEvent::StepSkipped { step, reason } => format!("⊘ skipped {step}: {reason}"),
        RunEvent::ApprovalRequested { step, .. } => format!("? approval {step}"),
        RunEvent::RunFinished { artifacts, .. } => {
            format!("■ run_finished ({} artifacts)", artifacts.len())
        }
        RunEvent::RunFailed { reason, .. } => format!("■ run_failed: {}", truncate(reason, 80)),
        RunEvent::RunCancelled { .. } => "■ run_cancelled".into(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    let one = s.replace('\n', " ");
    if one.chars().count() <= max {
        one
    } else {
        let head: String = one.chars().take(max).collect();
        format!("{head}…")
    }
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn runs_root() -> PathBuf {
    std::env::var_os("OXIDEMX_RUNS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let base = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
                .unwrap_or_else(|| PathBuf::from("."));
            base.join("oxidemx").join("runs")
        })
}

fn load_allowlist() -> Vec<String> {
    oxidemx_shared::config::default_config_path()
        .and_then(|p| oxidemx_shared::AppConfig::load_from(&p).ok())
        .map(|c| c.overlay.ai.command_allowlist)
        .unwrap_or_default()
}
