//! The run supervisor — the orchestration AutoAgents doesn't ship.
//!
//! Given a validated `FlowPlan`, it drives the DAG to completion:
//! schedules every step whose `needs` are satisfied **concurrently**
//! (free parallelism from the DAG — fixing kowalski's sequential-only
//! limitation), **joins** fan-ins by waiting for all needs before a
//! step assembles its context, **retries** failed steps with backoff,
//! **times out** per step, and **cancels** the whole run via a
//! `CancellationToken`. Each step runs as one AutoAgents ReAct
//! DirectAgent (`step_agent`).
//!
//! This realizes the parallel.rs "supervisor watches completions,
//! schedules ready steps" pattern (spec §7.1) with direct
//! `agent.run()` calls + a `JoinSet` instead of topic pub/sub — the
//! same intent, less framework indirection, and (per the multi-
//! provider reality) the same cancellation guarantees, since built-in
//! providers wouldn't honor a per-run runtime's `stop()` inside their
//! HTTP anyway (spec §7.3).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use autoagents::llm::LLMProvider;
use oxidemx_shared::config::AiProvider;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::approval::ApprovalPolicy;
use crate::event::{EventSink, RunEvent};
use crate::flowdoc::Step;
use crate::plan::FlowPlan;
use crate::roster::Roster;
use crate::step_agent::{build_and_run, StepAgent};
use crate::template::{self, UpstreamResult};

/// Constructs an `LLMProvider` for a given model — the seam that lets
/// a run use the deterministic mock or a real configured backend.
pub trait ProviderFactory: Send + Sync {
    fn provider(&self, model: &str) -> Result<Arc<dyn LLMProvider>, String>;
}

/// Returns a fixed provider for every step (the mock, or any single
/// `Arc<dyn LLMProvider>`).
pub struct FixedFactory(pub Arc<dyn LLMProvider>);

impl ProviderFactory for FixedFactory {
    fn provider(&self, _model: &str) -> Result<Arc<dyn LLMProvider>, String> {
        Ok(self.0.clone())
    }
}

/// Builds a real provider per step from the configured backend + key,
/// honoring each step's model override.
pub struct ConfigFactory {
    pub provider: AiProvider,
    pub api_key: String,
}

impl ProviderFactory for ConfigFactory {
    fn provider(&self, model: &str) -> Result<Arc<dyn LLMProvider>, String> {
        oxidemx_agent::factory::provider_from_config(self.provider, model, &self.api_key)
            .map_err(|e| e.to_string())
    }
}

/// Inputs + environment for one run.
pub struct RunOptions {
    pub run_id: String,
    /// Run inputs (already merged with defaults / validated for
    /// required presence by `resolve_inputs`).
    pub inputs: BTreeMap<String, String>,
    /// Directory artifacts are written under.
    pub workdir: PathBuf,
    pub roster: Roster,
    pub factory: Arc<dyn ProviderFactory>,
    pub cancel: CancellationToken,
    /// Effective default approval policy (governs the process
    /// allowlist installed for `execute_command`).
    pub approval: ApprovalPolicy,
    pub allowlist: Vec<String>,
}

/// A handle to an in-flight run: its id and the token that cancels it.
#[derive(Clone)]
pub struct RunHandle {
    pub run_id: String,
    pub cancel: CancellationToken,
}

/// The result of a finished run.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub run_id: String,
    pub success: bool,
    pub artifacts: Vec<String>,
    pub outputs: BTreeMap<String, String>,
    pub handoff_markdown: String,
    pub error: Option<String>,
}

/// Merge provided inputs with declared defaults and check required
/// inputs are present. Returns the resolved map or the list of
/// missing required inputs.
pub fn resolve_inputs(
    plan: &FlowPlan,
    provided: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, Vec<String>> {
    let mut out = BTreeMap::new();
    let mut missing = Vec::new();
    for (name, spec) in &plan.doc.manifest.inputs {
        if let Some(v) = provided.get(name) {
            out.insert(name.clone(), v.clone());
        } else if let Some(d) = &spec.default {
            out.insert(name.clone(), d.clone());
        } else if spec.required {
            missing.push(name.clone());
        }
    }
    // Pass through any extra provided inputs (harmless; ignored by
    // templating unless referenced).
    for (k, v) in provided {
        out.entry(k.clone()).or_insert_with(|| v.clone());
    }
    if missing.is_empty() {
        Ok(out)
    } else {
        Err(missing)
    }
}

/// Resolve a step's system prompt from its roster agent's persona plus
/// any `normalize` directive.
fn resolve_system(step: &Step, roster: &Roster) -> String {
    let mut sys = String::new();
    if let Some(agent_ref) = &step.agent {
        if let Some(def) = roster.get(agent_ref) {
            if !def.persona.trim().is_empty() {
                sys.push_str(def.persona.trim());
                sys.push_str("\n\n");
            }
        }
    }
    if let Some(norm) = &step.normalize {
        if let Some(title) = &norm.title {
            sys.push_str(&format!("Format your output as a markdown document titled \"{title}\".\n"));
        }
        if !norm.sections.is_empty() {
            sys.push_str(&format!(
                "Use exactly these `##` sections, in order: {}.\n",
                norm.sections.join(", ")
            ));
        }
    }
    if sys.trim().is_empty() {
        sys.push_str("You are a focused step agent. Complete the task and return only the result.");
    }
    sys.trim().to_string()
}

/// The granted tools for a step (from its roster agent).
fn resolve_tools(step: &Step, roster: &Roster) -> Vec<String> {
    step.agent
        .as_ref()
        .and_then(|a| roster.get(a))
        .map(|d| d.decl.tools.clone())
        .unwrap_or_default()
}

/// What a finished step task returns to the scheduler.
struct StepResult {
    step_id: String,
    result: Result<String, String>,
}

/// Run a flow to completion, emitting run-layer events to `sink`.
pub async fn run_flow(
    plan: &FlowPlan,
    opts: RunOptions,
    sink: Arc<dyn EventSink>,
) -> RunOutcome {
    // Install the allowlist the bridged `execute_command` self-gates
    // against. (Process-global OnceLock; first run wins — a known v1
    // limitation for a long-lived host, fine for the CLI.)
    if opts.approval != ApprovalPolicy::Autonomous {
        oxidemx_agent::tools::set_allowlist(opts.allowlist.clone());
    }

    sink.emit(RunEvent::RunStarted {
        flow_id: plan.doc.manifest.flow.id.clone(),
        run_id: opts.run_id.clone(),
        steps: plan.topo.clone(),
    })
    .await;

    let max_turns = plan.doc.manifest.defaults.max_turns;
    let all_steps: BTreeSet<String> = plan.topo.iter().cloned().collect();
    let mut done: BTreeMap<String, String> = BTreeMap::new();
    let mut artifacts: Vec<String> = Vec::new();
    let mut running: BTreeSet<String> = BTreeSet::new();
    // Steps cut off by a route branch that wasn't taken (and their
    // transitively-unreachable dependents).
    let mut skipped: BTreeSet<String> = BTreeSet::new();
    let mut failed: Option<(String, String)> = None;
    let mut join: JoinSet<StepResult> = JoinSet::new();

    'schedule: loop {
        if opts.cancel.is_cancelled() {
            break;
        }

        // Propagate route-branch skips: a pending step any of whose
        // needs is skipped can never run (needs are all-of), so it is
        // skipped too — to a fixpoint.
        propagate_skips(plan, &done, &mut skipped, &sink).await;

        // Done when every step has either finished or been skipped.
        if done.len() + skipped.len() == all_steps.len() {
            break;
        }

        // Ready = pending, not running/skipped, with all `needs` (and,
        // for a reflect step, its `target`) finished.
        let ready: Vec<String> = plan
            .topo
            .iter()
            .filter(|id| {
                !done.contains_key(*id) && !running.contains(*id) && !skipped.contains(*id)
            })
            .filter(|id| {
                let s = plan.step(id).expect("topo id exists");
                s.needs.iter().all(|n| done.contains_key(n))
                    && effective_target(s).map(|t| done.contains_key(t)).unwrap_or(true)
            })
            .cloned()
            .collect();

        if ready.is_empty() && running.is_empty() {
            // No ready and nothing in flight: either done (handled
            // above) or a genuinely stuck schedule.
            failed = Some((
                String::new(),
                "scheduler stalled: no ready or running steps".into(),
            ));
            break;
        }

        for id in ready {
            let step = plan.step(&id).expect("topo id exists").clone();
            match step.kind.as_str() {
                // reflect / route run inline (serialization points): a
                // bounded critic↔reviser loop / a one-shot classifier.
                "reflect" => {
                    match run_reflect_inline(plan, &opts, &step, &done, max_turns, &sink).await {
                        Ok(output) => {
                            finish_step(&opts, &step, output, &mut done, &mut artifacts, &sink).await;
                        }
                        Err(e) => {
                            emit_task_error(&sink, &id, &e).await;
                            failed = Some((id, e));
                            opts.cancel.cancel();
                            break 'schedule;
                        }
                    }
                }
                "route" => {
                    match run_route_inline(plan, &opts, &step, &done, max_turns, &sink).await {
                        Ok((label, chosen_target)) => {
                            // Skip the branches not taken.
                            for tgt in step.choices.values() {
                                if tgt != &chosen_target {
                                    mark_skipped(plan, tgt, &mut skipped, &sink).await;
                                }
                            }
                            finish_step(&opts, &step, label, &mut done, &mut artifacts, &sink).await;
                        }
                        Err(e) => {
                            emit_task_error(&sink, &id, &e).await;
                            failed = Some((id, e));
                            opts.cancel.cancel();
                            break 'schedule;
                        }
                    }
                }
                // agent steps run concurrently in the JoinSet.
                _ => {
                    sink.emit(RunEvent::TaskAssigned {
                        step: id.clone(),
                        agent: step.agent.clone().unwrap_or_default(),
                    })
                    .await;

                    let task_tmpl = step.task.clone().unwrap_or_default();
                    let task = match template::expand_inputs(&task_tmpl, &opts.inputs) {
                        Ok(t) => t,
                        Err(e) => {
                            failed = Some((id.clone(), e.to_string()));
                            break 'schedule;
                        }
                    };
                    let predecessor = (step.needs.len() == 1).then(|| UpstreamResult {
                        step_id: step.needs[0].clone(),
                        output: done.get(&step.needs[0]).cloned().unwrap_or_default(),
                    });
                    let prompt =
                        template::assemble_prompt(&task, &step.context, predecessor.as_ref(), &done);

                    let system = resolve_system(&step, &opts.roster);
                    let tools = resolve_tools(&step, &opts.roster);
                    let model = step.model(&plan.doc.manifest.defaults).to_string();
                    let provider = match opts.factory.provider(&model) {
                        Ok(p) => p,
                        Err(e) => {
                            failed = Some((id.clone(), format!("provider: {e}")));
                            break 'schedule;
                        }
                    };

                    let cancel = opts.cancel.clone();
                    let sink2 = sink.clone();
                    let id2 = id.clone();
                    let timeout = step.timeout_secs;
                    let retry = step.retry;
                    running.insert(id.clone());

                    join.spawn(async move {
                        sink2.emit(RunEvent::TaskStarted { step: id2.clone() }).await;
                        let agent = StepAgent::new(id2.clone(), system, tools, cancel.clone());
                        let result = run_step_with_retry(
                            provider, agent, &prompt, max_turns, timeout, retry, &cancel, &sink2, &id2,
                        )
                        .await;
                        StepResult { step_id: id2, result }
                    });
                }
            }
        }

        // If only inline steps ran this round, recompute readiness;
        // otherwise collect the next concurrent agent result.
        if running.is_empty() {
            continue;
        }
        let Some(joined) = join.join_next().await else {
            continue;
        };
        let sr = match joined {
            Ok(sr) => sr,
            Err(e) => {
                failed = Some((String::new(), format!("step task panicked: {e}")));
                break;
            }
        };
        running.remove(&sr.step_id);

        match sr.result {
            Ok(output) => {
                let step = plan.step(&sr.step_id).expect("step exists").clone();
                finish_step(&opts, &step, output, &mut done, &mut artifacts, &sink).await;
            }
            Err(e) => {
                emit_task_error(&sink, &sr.step_id, &e).await;
                failed = Some((sr.step_id, e));
                opts.cancel.cancel(); // stop scheduling further work
            }
        }
    }

    // Drain any still-running tasks (cancel already fired on failure /
    // external cancel) so we don't leak them.
    join.abort_all();
    while join.join_next().await.is_some() {}

    finalize(plan, &opts, sink, done, artifacts, failed).await
}

/// The extra scheduling dependency a reflect step has on its `target`
/// (it can't critique an output that doesn't exist yet).
fn effective_target(step: &Step) -> Option<&str> {
    if step.kind == "reflect" {
        step.target.as_deref()
    } else {
        None
    }
}

/// Commit a finished step: write its artifact, emit `task_finished`,
/// record its output for downstream context.
async fn finish_step(
    opts: &RunOptions,
    step: &Step,
    output: String,
    done: &mut BTreeMap<String, String>,
    artifacts: &mut Vec<String>,
    sink: &Arc<dyn EventSink>,
) {
    let artifact = write_artifact(&opts.workdir, step, &output).await;
    if let Some(path) = &artifact {
        artifacts.push(path.clone());
    }
    sink.emit(RunEvent::TaskFinished {
        step: step.id.clone(),
        success: true,
        artifact,
        summary: summarize(&output),
    })
    .await;
    done.insert(step.id.clone(), output);
}

async fn emit_task_error(sink: &Arc<dyn EventSink>, step: &str, error: &str) {
    sink.emit(RunEvent::TaskError {
        step: step.to_string(),
        error: error.to_string(),
    })
    .await;
}

/// Skip a step (route branch not taken) and announce it.
async fn mark_skipped(
    _plan: &FlowPlan,
    step: &str,
    skipped: &mut BTreeSet<String>,
    sink: &Arc<dyn EventSink>,
) {
    if skipped.insert(step.to_string()) {
        sink.emit(RunEvent::StepSkipped {
            step: step.to_string(),
            reason: "route branch not taken".into(),
        })
        .await;
    }
}

/// Transitively skip any pending step that needs an already-skipped
/// step (needs are all-of, so one skipped need makes the step
/// unreachable). Runs to a fixpoint.
async fn propagate_skips(
    plan: &FlowPlan,
    done: &BTreeMap<String, String>,
    skipped: &mut BTreeSet<String>,
    sink: &Arc<dyn EventSink>,
) {
    loop {
        let newly: Vec<String> = plan
            .doc
            .manifest
            .steps
            .iter()
            .filter(|s| !done.contains_key(&s.id) && !skipped.contains(&s.id))
            .filter(|s| s.needs.iter().any(|n| skipped.contains(n)))
            .map(|s| s.id.clone())
            .collect();
        if newly.is_empty() {
            break;
        }
        for id in newly {
            sink.emit(RunEvent::StepSkipped {
                step: id.clone(),
                reason: "upstream branch not taken".into(),
            })
            .await;
            skipped.insert(id);
        }
    }
}

/// A roster agent's persona, or a sensible fallback.
fn persona_of(roster: &Roster, id: &str, fallback: &str) -> String {
    roster
        .get(id)
        .map(|d| d.persona.trim().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

/// Run a route step: a one-shot classifier picks one of the declared
/// labels; returns `(label, chosen_target_step_id)`.
async fn run_route_inline(
    plan: &FlowPlan,
    opts: &RunOptions,
    step: &Step,
    done: &BTreeMap<String, String>,
    max_turns: usize,
    sink: &Arc<dyn EventSink>,
) -> Result<(String, String), String> {
    sink.emit(RunEvent::TaskAssigned {
        step: step.id.clone(),
        agent: "router".into(),
    })
    .await;
    sink.emit(RunEvent::TaskStarted { step: step.id.clone() }).await;

    let base = template::expand_inputs(step.prompt.as_deref().unwrap_or(""), &opts.inputs)
        .map_err(|e| e.to_string())?;
    let pred = (step.needs.len() == 1).then(|| UpstreamResult {
        step_id: step.needs[0].clone(),
        output: done.get(&step.needs[0]).cloned().unwrap_or_default(),
    });
    let labels: Vec<String> = step.choices.keys().cloned().collect();
    let body = template::assemble_prompt(&base, &step.context, pred.as_ref(), done);
    let task = format!(
        "{body}\n\nChoose exactly ONE option and reply with ONLY its label (no other words). Options: {}",
        labels.join(", ")
    );
    let system = "You are a router. Read the context and the question, then output exactly one of \
                  the allowed labels and nothing else."
        .to_string();
    let model = step.model(&plan.doc.manifest.defaults).to_string();
    let provider = opts.factory.provider(&model).map_err(|e| format!("provider: {e}"))?;
    let agent = StepAgent::new(format!("{}:router", step.id), system, vec![], opts.cancel.clone());
    let out = build_and_run(provider, agent, &task, max_turns.clamp(1, 2), &opts.cancel)
        .await
        .map_err(|e| e.to_string())?;
    let (label, target) = parse_route_choice(&out, &step.choices);
    sink.emit(RunEvent::AgentMessage {
        step: step.id.clone(),
        message: format!("routed → {label}"),
    })
    .await;
    Ok((label, target))
}

/// Map a classifier's free-text answer to a declared choice: prefer an
/// exact label token, then a substring; fall back to the first choice.
fn parse_route_choice(output: &str, choices: &BTreeMap<String, String>) -> (String, String) {
    let lower = output.to_lowercase();
    let mut substring: Option<(&String, &String)> = None;
    for (label, target) in choices {
        let l = label.to_lowercase();
        if lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|tok| tok == l)
        {
            return (label.clone(), target.clone());
        }
        if substring.is_none() && lower.contains(&l) {
            substring = Some((label, target));
        }
    }
    if let Some((l, t)) = substring {
        return (l.clone(), t.clone());
    }
    let (l, t) = choices.iter().next().expect("route step has choices");
    (l.clone(), t.clone())
}

/// Run a reflect step: a bounded critic↔reviser loop over the target
/// step's output (reflection.rs pattern, supervisor-counted). Returns
/// the accepted/last revision — a SEPARATE artifact from the target's
/// own output (downstream may consume either).
async fn run_reflect_inline(
    plan: &FlowPlan,
    opts: &RunOptions,
    step: &Step,
    done: &BTreeMap<String, String>,
    max_turns: usize,
    sink: &Arc<dyn EventSink>,
) -> Result<String, String> {
    let target_id = step.target.as_deref().ok_or("reflect step missing target")?;
    let target_step = plan
        .step(target_id)
        .ok_or_else(|| format!("reflect target `{target_id}` not found"))?
        .clone();
    let critic_ref = step.critic.clone().unwrap_or_default();

    sink.emit(RunEvent::TaskAssigned {
        step: step.id.clone(),
        agent: critic_ref.clone(),
    })
    .await;
    sink.emit(RunEvent::TaskStarted { step: step.id.clone() }).await;

    let mut current = done.get(target_id).cloned().unwrap_or_default();
    let critic_system = persona_of(
        &opts.roster,
        &critic_ref,
        "You are a rigorous critic. Identify only genuinely blocking problems.",
    );
    let critic_model = opts
        .roster
        .get(&critic_ref)
        .and_then(|d| d.decl.model.clone())
        .unwrap_or_else(|| plan.doc.manifest.defaults.model.clone());
    let critic_provider = opts
        .factory
        .provider(&critic_model)
        .map_err(|e| format!("provider: {e}"))?;

    let target_system = resolve_system(&target_step, &opts.roster);
    let target_tools = resolve_tools(&target_step, &opts.roster);
    let target_task = template::expand_inputs(target_step.task.as_deref().unwrap_or(""), &opts.inputs)
        .map_err(|e| e.to_string())?;
    let target_model = target_step.model(&plan.doc.manifest.defaults).to_string();
    let target_provider = opts
        .factory
        .provider(&target_model)
        .map_err(|e| format!("provider: {e}"))?;

    let rounds = step.max_rounds.unwrap_or(1).max(1);
    for round in 1..=rounds {
        if opts.cancel.is_cancelled() {
            return Err("run cancelled".into());
        }
        let critique_prompt = format!(
            "Critically review the output below for BLOCKING problems (factual errors, unmet \
             requirements, unsafe suggestions). List each as a bullet. If there are none, reply \
             with exactly: NO BLOCKING FINDINGS.\n\n---\n{current}"
        );
        let critic_agent = StepAgent::new(
            format!("{}:critic", step.id),
            critic_system.clone(),
            vec![],
            opts.cancel.clone(),
        );
        let critique = build_and_run(
            critic_provider.clone(),
            critic_agent,
            &critique_prompt,
            max_turns,
            &opts.cancel,
        )
        .await
        .map_err(|e| e.to_string())?;
        sink.emit(RunEvent::AgentMessage {
            step: step.id.clone(),
            message: format!("round {round} critique: {}", summarize(&critique)),
        })
        .await;

        if reflect_accepts(&critique) {
            sink.emit(RunEvent::AgentMessage {
                step: step.id.clone(),
                message: format!("accepted after round {round}"),
            })
            .await;
            return Ok(current);
        }

        // Revise via the target's own agent, addressing the critique.
        let revise_prompt = format!(
            "Original task:\n{target_task}\n\nYour previous output:\n{current}\n\nA reviewer raised \
             these blocking findings:\n{critique}\n\nProduce a revised version that resolves them. \
             Return only the revised output."
        );
        let reviser = StepAgent::new(
            format!("{}:revise", step.id),
            target_system.clone(),
            target_tools.clone(),
            opts.cancel.clone(),
        );
        current = build_and_run(
            target_provider.clone(),
            reviser,
            &revise_prompt,
            max_turns,
            &opts.cancel,
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(current)
}

/// v1 acceptance predicate: the critic declared no blocking findings.
/// (`accept_when` other than `no_blocking_findings` falls back to this
/// same marker for now.)
fn reflect_accepts(critique: &str) -> bool {
    critique.to_uppercase().contains("NO BLOCKING FINDINGS")
}

/// One step's execution including timeout + retry/backoff.
#[allow(clippy::too_many_arguments)]
async fn run_step_with_retry(
    provider: Arc<dyn LLMProvider>,
    agent: StepAgent,
    prompt: &str,
    max_turns: usize,
    timeout_secs: Option<u64>,
    retry: Option<crate::flowdoc::Retry>,
    cancel: &CancellationToken,
    sink: &Arc<dyn EventSink>,
    step_id: &str,
) -> Result<String, String> {
    let max_attempts = retry.map(|r| r.max + 1).unwrap_or(1).max(1);
    let backoff = retry.map(|r| r.backoff_secs).unwrap_or(0);
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let fut = build_and_run(provider.clone(), agent.clone(), prompt, max_turns, cancel);
        let res = match timeout_secs {
            Some(t) => match tokio::time::timeout(Duration::from_secs(t), fut).await {
                Ok(r) => r.map_err(|e| e.to_string()),
                Err(_) => Err(format!("step timed out after {t}s")),
            },
            None => fut.await.map_err(|e| e.to_string()),
        };
        match res {
            Ok(out) => return Ok(out),
            Err(e) => {
                if attempt >= max_attempts || cancel.is_cancelled() {
                    return Err(e);
                }
                sink.emit(RunEvent::StepRetrying {
                    step: step_id.to_string(),
                    attempt,
                })
                .await;
                if backoff > 0 {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
                        _ = cancel.cancelled() => return Err("run cancelled".into()),
                    }
                }
            }
        }
    }
}

/// Write a step's output to its declared artifact path (under the run
/// workdir), creating parent dirs. Returns the relative path written.
async fn write_artifact(workdir: &std::path::Path, step: &Step, output: &str) -> Option<String> {
    let rel = step.output.as_ref()?;
    let full = workdir.join(rel);
    if let Some(parent) = full.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    match tokio::fs::write(&full, output).await {
        Ok(()) => Some(rel.clone()),
        Err(_) => None,
    }
}

fn summarize(output: &str) -> String {
    let head: String = output.chars().take(160).collect();
    head.replace('\n', " ")
}

/// Emit the terminal event and build the outcome.
async fn finalize(
    plan: &FlowPlan,
    opts: &RunOptions,
    sink: Arc<dyn EventSink>,
    done: BTreeMap<String, String>,
    artifacts: Vec<String>,
    failed: Option<(String, String)>,
) -> RunOutcome {
    if opts.cancel.is_cancelled() && failed.is_none() {
        sink.emit(RunEvent::RunCancelled {
            run_id: opts.run_id.clone(),
        })
        .await;
        return RunOutcome {
            run_id: opts.run_id.clone(),
            success: false,
            artifacts,
            outputs: done,
            handoff_markdown: String::new(),
            error: Some("cancelled".into()),
        };
    }

    if let Some((step, reason)) = failed {
        sink.emit(RunEvent::RunFailed {
            run_id: opts.run_id.clone(),
            reason: reason.clone(),
            step: (!step.is_empty()).then_some(step.clone()),
        })
        .await;
        return RunOutcome {
            run_id: opts.run_id.clone(),
            success: false,
            artifacts,
            outputs: done,
            handoff_markdown: String::new(),
            error: Some(reason),
        };
    }

    // Handoff markdown: the delivery root's output if named, else the
    // last topological step's output. Capped at the inline limit.
    let handoff_src = plan
        .doc
        .manifest
        .delivery
        .as_ref()
        .and_then(|d| d.root.as_ref())
        .and_then(|root| {
            // root is an artifact path; find the step that wrote it.
            plan.doc
                .manifest
                .steps
                .iter()
                .find(|s| s.output.as_deref() == Some(root.as_str()))
                .map(|s| s.id.clone())
        })
        .or_else(|| plan.topo.last().cloned())
        .and_then(|id| done.get(&id).cloned())
        .unwrap_or_default();
    let handoff_markdown = RunEvent::cap_inline(&handoff_src);

    sink.emit(RunEvent::RunFinished {
        run_id: opts.run_id.clone(),
        artifacts: artifacts.clone(),
        handoff_markdown: handoff_markdown.clone(),
    })
    .await;

    RunOutcome {
        run_id: opts.run_id.clone(),
        success: true,
        artifacts,
        outputs: done,
        handoff_markdown,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::CollectingSink;
    use crate::flowdoc::FlowDoc;
    use crate::mock::MockProvider;
    use crate::plan::validate;
    use crate::roster::AgentDef;

    fn roster() -> Roster {
        let mut r = Roster::new();
        for id in ["web-researcher", "summarizer", "writer", "extractor", "skeptic"] {
            r.insert(AgentDef::parse(&format!("---\nid = \"{id}\"\n---\nYou are {id}.")).unwrap());
        }
        r
    }

    fn opts(inputs: BTreeMap<String, String>, factory: Arc<dyn ProviderFactory>, workdir: PathBuf) -> RunOptions {
        RunOptions {
            run_id: "test-run".into(),
            inputs,
            workdir,
            roster: roster(),
            factory,
            cancel: CancellationToken::new(),
            approval: ApprovalPolicy::Autonomous,
            allowlist: vec![],
        }
    }

    const LINEAR: &str = r#"---
[flow]
id = "research-digest"
[inputs]
url = { type = "string", required = true }
[[step]]
id = "ingest"
agent = "web-researcher"
task = "Fetch {{input.url}}."
output = "debug/raw.md"
[[step]]
id = "digest"
agent = "summarizer"
needs = ["ingest"]
task = "Digest the source."
context = ["@artifact@"]
output = "debug/digest.md"
[[step]]
id = "answer"
agent = "writer"
needs = ["digest"]
task = "Write the final answer."
context = ["@step:digest@"]
output = "ANSWER.md"
---
"#;

    #[tokio::test]
    async fn linear_flow_runs_end_to_end_with_events_and_artifacts() {
        let plan = validate(&FlowDoc::parse(LINEAR).unwrap(), &roster(), &[]).unwrap();
        let dir = std::env::temp_dir().join("conductor-test-linear");
        let _ = std::fs::remove_dir_all(&dir);
        let inputs = resolve_inputs(
            &plan,
            &BTreeMap::from([("url".to_string(), "https://example.com".to_string())]),
        )
        .unwrap();
        let sink = Arc::new(CollectingSink::default());
        let outcome = run_flow(
            &plan,
            opts(inputs, Arc::new(FixedFactory(MockProvider::echoing())), dir.clone()),
            sink.clone(),
        )
        .await;

        assert!(outcome.success, "run failed: {:?}", outcome.error);
        assert_eq!(outcome.artifacts.len(), 3);
        // The answer's output carries the digest carries the ingest
        // carries the url — proves context flowed down the chain.
        let answer = &outcome.outputs["answer"];
        assert!(answer.contains("example.com"), "answer lost upstream context: {answer}");
        // Artifact file actually written.
        assert!(dir.join("ANSWER.md").exists());

        let events = sink.snapshot().await;
        let kinds: Vec<&str> = events
            .iter()
            .map(|e| match e {
                RunEvent::RunStarted { .. } => "run_started",
                RunEvent::TaskAssigned { .. } => "task_assigned",
                RunEvent::TaskStarted { .. } => "task_started",
                RunEvent::TaskFinished { .. } => "task_finished",
                RunEvent::RunFinished { .. } => "run_finished",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds.first(), Some(&"run_started"));
        assert_eq!(kinds.last(), Some(&"run_finished"));
        assert_eq!(kinds.iter().filter(|k| **k == "task_finished").count(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    const DIAMOND: &str = r#"---
[flow]
id = "diamond"
[inputs]
topic = { type = "string", default = "rust" }
[[step]]
id = "seed"
agent = "web-researcher"
task = "Seed on {{input.topic}}."
[[step]]
id = "left"
agent = "summarizer"
needs = ["seed"]
task = "Left branch."
context = ["@artifact@"]
[[step]]
id = "right"
agent = "extractor"
needs = ["seed"]
task = "Right branch."
context = ["@artifact@"]
[[step]]
id = "merge"
agent = "writer"
needs = ["left", "right"]
task = "Merge both branches."
context = ["@step:left@", "@step:right@"]
output = "MERGED.md"
---
"#;

    #[tokio::test]
    async fn diamond_flow_fans_out_and_joins() {
        let plan = validate(&FlowDoc::parse(DIAMOND).unwrap(), &roster(), &[]).unwrap();
        let dir = std::env::temp_dir().join("conductor-test-diamond");
        let _ = std::fs::remove_dir_all(&dir);
        let inputs = resolve_inputs(&plan, &BTreeMap::new()).unwrap();
        let sink = Arc::new(CollectingSink::default());
        let outcome = run_flow(
            &plan,
            opts(inputs, Arc::new(FixedFactory(MockProvider::echoing())), dir.clone()),
            sink.clone(),
        )
        .await;
        assert!(outcome.success, "diamond failed: {:?}", outcome.error);
        // merge saw BOTH branches (join worked).
        let merged = &outcome.outputs["merge"];
        assert!(merged.contains("output of `left`"), "merge missing left: {merged}");
        assert!(merged.contains("output of `right`"), "merge missing right: {merged}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn missing_required_input_is_reported() {
        let plan = validate(&FlowDoc::parse(LINEAR).unwrap(), &roster(), &[]).unwrap();
        let missing = resolve_inputs(&plan, &BTreeMap::new()).unwrap_err();
        assert_eq!(missing, vec!["url".to_string()]);
    }

    #[tokio::test]
    async fn a_failing_step_fails_the_run() {
        // A provider that always errors ⇒ step exhausts (no retry) ⇒ run fails.
        struct ErrFactory;
        impl ProviderFactory for ErrFactory {
            fn provider(&self, _m: &str) -> Result<Arc<dyn LLMProvider>, String> {
                Err("no backend".into())
            }
        }
        let plan = validate(&FlowDoc::parse(LINEAR).unwrap(), &roster(), &[]).unwrap();
        let inputs = resolve_inputs(
            &plan,
            &BTreeMap::from([("url".to_string(), "x".to_string())]),
        )
        .unwrap();
        let sink = Arc::new(CollectingSink::default());
        let outcome = run_flow(
            &plan,
            opts(inputs, Arc::new(ErrFactory), std::env::temp_dir().join("conductor-test-err")),
            sink.clone(),
        )
        .await;
        assert!(!outcome.success);
        assert!(sink.snapshot().await.iter().any(|e| matches!(e, RunEvent::RunFailed { .. })));
    }

    // ── reflect ──────────────────────────────────────────────────────

    const REFLECT_FLOW: &str = r#"---
[flow]
id = "reflecty"
[[step]]
id = "draft"
agent = "writer"
task = "Write a draft."
output = "debug/draft.md"
[[step]]
id = "review"
kind = "reflect"
target = "draft"
critic = "skeptic"
max_rounds = 2
accept_when = "no_blocking_findings"
output = "REVIEW.md"
---
"#;

    #[tokio::test]
    async fn reflect_accepts_when_critic_finds_nothing() {
        // Critic replies with the accept marker on round 1 ⇒ the
        // review output is the unchanged draft.
        let provider = MockProvider::scripted(
            vec![
                ("Critically review", "NO BLOCKING FINDINGS"),
                ("Write a draft", "INITIAL-DRAFT"),
            ],
            "X",
        );
        let plan = validate(&FlowDoc::parse(REFLECT_FLOW).unwrap(), &roster(), &[]).unwrap();
        let dir = std::env::temp_dir().join("conductor-test-reflect-accept");
        let _ = std::fs::remove_dir_all(&dir);
        let sink = Arc::new(CollectingSink::default());
        let outcome = run_flow(
            &plan,
            opts(BTreeMap::new(), Arc::new(FixedFactory(provider)), dir.clone()),
            sink.clone(),
        )
        .await;
        assert!(outcome.success, "reflect run failed: {:?}", outcome.error);
        assert_eq!(outcome.outputs["review"], "INITIAL-DRAFT");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn reflect_revises_when_critic_finds_problems() {
        // Critic never accepts ⇒ the target's agent revises each round;
        // the review output is the revised text, not the draft.
        let provider = MockProvider::scripted(
            vec![
                ("Produce a revised version", "REVISED-OUTPUT"),
                ("Critically review", "FINDING: please expand"),
                ("Write a draft", "INITIAL-DRAFT"),
            ],
            "X",
        );
        let plan = validate(&FlowDoc::parse(REFLECT_FLOW).unwrap(), &roster(), &[]).unwrap();
        let dir = std::env::temp_dir().join("conductor-test-reflect-revise");
        let _ = std::fs::remove_dir_all(&dir);
        let sink = Arc::new(CollectingSink::default());
        let outcome = run_flow(
            &plan,
            opts(BTreeMap::new(), Arc::new(FixedFactory(provider)), dir.clone()),
            sink.clone(),
        )
        .await;
        assert!(outcome.success, "reflect run failed: {:?}", outcome.error);
        assert_eq!(outcome.outputs["draft"], "INITIAL-DRAFT");
        assert_eq!(outcome.outputs["review"], "REVISED-OUTPUT");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── route ────────────────────────────────────────────────────────

    const ROUTE_FLOW: &str = r#"---
[flow]
id = "routey"
[[step]]
id = "classify"
agent = "writer"
task = "Produce a topic."
[[step]]
id = "decide"
kind = "route"
needs = ["classify"]
prompt = "Pick the right branch."
choices = { alpha = "path_a", beta = "path_b" }
[[step]]
id = "path_a"
agent = "writer"
needs = ["decide"]
task = "Handle the A branch."
output = "A.md"
[[step]]
id = "path_b"
agent = "writer"
needs = ["decide"]
task = "Handle the B branch."
output = "B.md"
---
"#;

    #[tokio::test]
    async fn route_runs_chosen_branch_and_skips_the_other() {
        // The router sees the option list and picks "alpha" ⇒ path_a
        // runs, path_b is skipped, and the run still completes.
        let provider = MockProvider::scripted(vec![("Options:", "alpha")], "BRANCH-DONE");
        let plan = validate(&FlowDoc::parse(ROUTE_FLOW).unwrap(), &roster(), &[]).unwrap();
        let dir = std::env::temp_dir().join("conductor-test-route");
        let _ = std::fs::remove_dir_all(&dir);
        let sink = Arc::new(CollectingSink::default());
        let outcome = run_flow(
            &plan,
            opts(BTreeMap::new(), Arc::new(FixedFactory(provider)), dir.clone()),
            sink.clone(),
        )
        .await;
        assert!(outcome.success, "route run failed: {:?}", outcome.error);
        assert_eq!(outcome.outputs["decide"], "alpha");
        assert!(outcome.outputs.contains_key("path_a"), "chosen branch did not run");
        assert!(!outcome.outputs.contains_key("path_b"), "skipped branch ran anyway");
        assert!(dir.join("A.md").exists());
        assert!(!dir.join("B.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
