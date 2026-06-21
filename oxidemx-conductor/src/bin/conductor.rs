//! `oxidemx-conductor` — the flow CLI (spec §12 P3 exit criterion:
//! "research-digest runs headless end-to-end with events on stdout").
//!
//! Commands:
//!   list                         List available flows.
//!   validate <flow-id>           Validate a flow; print errors or OK.
//!   run <flow-id> [opts]         Run a flow; stream run events as JSON
//!                                lines to stdout.
//!   status [<run-id>]            Show recent runs / one run's outcome.
//!   oneshot [opts] <prompt>      Single context-free prompt → output.
//!                                Defaults to the keyless Claude Code
//!                                CLI — good for prompt optimization
//!                                and one-shot code/tool generators.
//!
//! `run` options:
//!   --input k=v        Supply a run input (repeatable).
//!   --mock             Use the deterministic mock provider (no key).
//!   --provider NAME    gemini|openai|anthropic|ollama|claude_code.
//!   --model M          Override the model for every step.
//!   --workdir DIR      Run workspace (default under the runs root).
//!   --flows-dir DIR / --agents-dir DIR   Override the roots.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use oxidemx_conductor::approval::ApprovalPolicy;
use oxidemx_conductor::event::JsonLinesSink;
use oxidemx_conductor::loader::{default_agents_root, default_flows_root, list_flows, load_flow};
use oxidemx_conductor::mock::MockProvider;
use oxidemx_conductor::step_agent::{build_and_run, StepAgent};
use oxidemx_conductor::supervisor::{
    resolve_inputs, run_flow, ConfigFactory, FixedFactory, ProviderFactory, RunOptions,
};
use oxidemx_conductor::schedule::Schedule;
use oxidemx_conductor::{validate, KNOWN_TOOLS};
use oxidemx_shared::config::AiProvider;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    match cmd {
        "list" => cmd_list(),
        "validate" => cmd_validate(&args[1..]),
        "run" => cmd_run(&args[1..]).await,
        "tick" => cmd_tick(&args[1..]).await,
        "oneshot" => cmd_oneshot(&args[1..]).await,
        "status" => cmd_status(&args[1..]),
        "help" | "-h" | "--help" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command `{other}`\n");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    eprintln!(
        "oxidemx-conductor — flow orchestration CLI\n\n\
         USAGE:\n  \
         oxidemx-conductor list\n  \
         oxidemx-conductor validate <flow-id>\n  \
         oxidemx-conductor run <flow-id> [--input k=v]... [--mock] [--provider P] [--model M] [--workdir DIR]\n  \
         oxidemx-conductor oneshot [--provider P] [--model M] [--system S] <prompt>\n  \
         oxidemx-conductor tick    (run flows whose schedule trigger is due — for a timer)\n  \
         oxidemx-conductor status [<run-id>]\n\n\
         `oneshot` runs a single context-free prompt (default provider: claude_code,\n\
         the keyless Claude Code CLI) — handy for prompt optimization and one-shot\n\
         code/tool generators. Reads the prompt from stdin if no prompt arg is given.\n\n\
         Flows are read from $OXIDEMX_FLOWS_DIR (default ~/.config/oxidemx/flows);\n\
         agents from $OXIDEMX_AGENTS_DIR (default ~/.config/oxidemx/agents)."
    );
}

fn cmd_list() -> ExitCode {
    let root = default_flows_root();
    let flows = list_flows(&root);
    if flows.is_empty() {
        eprintln!("no flows found under {}", root.display());
    } else {
        for f in flows {
            println!("{f}");
        }
    }
    ExitCode::SUCCESS
}

fn cmd_validate(args: &[String]) -> ExitCode {
    let opts = parse_opts(args);
    let Some(id) = opts.positionals.first() else {
        eprintln!("usage: oxidemx-conductor validate <flow-id>");
        return ExitCode::FAILURE;
    };
    let flows_root = opts.flows_dir.unwrap_or_else(default_flows_root);
    let agents_root = opts.agents_dir.unwrap_or_else(default_agents_root);

    let (doc, roster) = match load_flow(&flows_root, &agents_root, id) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    match validate(&doc, &roster, KNOWN_TOOLS) {
        Ok(plan) => {
            println!(
                "✓ `{}` valid — {} step(s), order: {}",
                doc.manifest.flow.id,
                plan.topo.len(),
                plan.topo.join(" → ")
            );
            if opts.stages {
                println!("stages:");
                for (i, stage) in plan.stages().iter().enumerate() {
                    let label = if i == 0 { " (entry)" } else if stage.len() > 1 { " (parallel)" } else { "" };
                    println!("  stage {i}{label}: {}", stage.join(", "));
                }
            }
            // Non-fatal foot-gun warning: a route step with no needs runs with
            // no upstream context.
            for s in &doc.manifest.steps {
                if s.kind == "route" && s.needs.is_empty() {
                    eprintln!("  warning: route step `{}` has no `needs`; it will run with no upstream context", s.id);
                }
            }
            ExitCode::SUCCESS
        }
        Err(errors) => {
            eprintln!("✗ `{}` has {} problem(s):", doc.manifest.flow.id, errors.len());
            for e in errors {
                eprintln!("  - {e}");
            }
            ExitCode::FAILURE
        }
    }
}

async fn cmd_run(args: &[String]) -> ExitCode {
    let Some(id) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: oxidemx-conductor run <flow-id> [opts]");
        return ExitCode::FAILURE;
    };
    let opts = parse_opts(args);
    let flows_root = opts.flows_dir.clone().unwrap_or_else(default_flows_root);
    let agents_root = opts.agents_dir.clone().unwrap_or_else(default_agents_root);

    let (doc, roster) = match load_flow(&flows_root, &agents_root, id) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let plan = match validate(&doc, &roster, KNOWN_TOOLS) {
        Ok(p) => p,
        Err(errors) => {
            eprintln!("✗ flow does not validate; fix before running:");
            for e in errors {
                eprintln!("  - {e}");
            }
            return ExitCode::FAILURE;
        }
    };

    let inputs = match resolve_inputs(&plan, &opts.inputs) {
        Ok(i) => i,
        Err(missing) => {
            eprintln!("✗ missing required input(s): {}", missing.join(", "));
            eprintln!("  supply with --input <name>=<value>");
            return ExitCode::FAILURE;
        }
    };

    // Provider factory: mock (deterministic, no key) or a real backend.
    let factory: Arc<dyn ProviderFactory> = if opts.mock {
        Arc::new(FixedFactory(MockProvider::echoing()))
    } else {
        let provider = opts.provider.unwrap_or(AiProvider::Gemini);
        let key = if provider.needs_key() {
            match oxidemx_agent::keys::provider_key(provider) {
                Some(k) => k,
                None => {
                    eprintln!(
                        "✗ no API key for {}. Set {} or write ~/.config/oxidemx/{}.key, or use --mock.",
                        provider.label(),
                        provider.key_env().unwrap_or("the key env"),
                        provider.key_file_stem().unwrap_or("<provider>")
                    );
                    return ExitCode::FAILURE;
                }
            }
        } else {
            String::new()
        };
        Arc::new(ConfigFactory { provider, api_key: key })
    };

    let run_id = format!("{id}-{}", unix_millis());
    let workdir = opts.workdir.unwrap_or_else(|| runs_root().join(&run_id));
    if let Err(e) = std::fs::create_dir_all(&workdir) {
        eprintln!("✗ cannot create workdir {}: {e}", workdir.display());
        return ExitCode::FAILURE;
    }

    let approval = ApprovalPolicy::parse(&plan.doc.manifest.defaults.approval);
    let allowlist = load_allowlist();
    let cancel = CancellationToken::new();
    install_signal_handler(cancel.clone());

    eprintln!(
        "▶ running `{id}` (run_id={run_id}, provider={}, workdir={})",
        if opts.mock { "mock" } else { "real" },
        workdir.display()
    );

    let run_opts = RunOptions {
        run_id: run_id.clone(),
        inputs,
        workdir: workdir.clone(),
        roster,
        factory,
        cancel,
        approval,
        allowlist,
        conversation_id: String::new(),
    };
    let outcome = run_flow(&plan, run_opts, Arc::new(JsonLinesSink)).await;

    // Persist a status record for `flow status`.
    let record = serde_json::json!({
        "run_id": outcome.run_id,
        "flow_id": plan.doc.manifest.flow.id,
        "success": outcome.success,
        "artifacts": outcome.artifacts,
        "error": outcome.error,
    });
    let _ = std::fs::write(
        workdir.join("run.json"),
        serde_json::to_string_pretty(&record).unwrap_or_default(),
    );

    if outcome.success {
        eprintln!(
            "✓ run finished — {} artifact(s) under {}",
            outcome.artifacts.len(),
            workdir.display()
        );
        ExitCode::SUCCESS
    } else {
        eprintln!("✗ run failed: {}", outcome.error.unwrap_or_default());
        ExitCode::FAILURE
    }
}

/// Single context-free prompt → output. No flow, no DAG, no history.
/// Defaults to the keyless Claude Code CLI provider; reads the prompt
/// from args or, if none, from stdin.
async fn cmd_oneshot(args: &[String]) -> ExitCode {
    let opts = parse_opts(args);
    let prompt = if opts.positionals.is_empty() {
        use std::io::Read;
        let mut buf = String::new();
        if std::io::stdin().read_to_string(&mut buf).is_err() || buf.trim().is_empty() {
            eprintln!("usage: oxidemx-conductor oneshot [opts] <prompt>   (or pipe the prompt on stdin)");
            return ExitCode::FAILURE;
        }
        buf
    } else {
        opts.positionals.join(" ")
    };

    // Default to the keyless Claude Code CLI — the "no key, no context"
    // quick path the user asked for.
    let provider = opts.provider.unwrap_or(AiProvider::ClaudeCode);
    let model = opts.model.clone().unwrap_or_else(|| provider.default_model().to_string());
    let key = if provider.needs_key() {
        match oxidemx_agent::keys::provider_key(provider) {
            Some(k) => k,
            None => {
                eprintln!(
                    "✗ no API key for {}. Set {} or write ~/.config/oxidemx/{}.key.",
                    provider.label(),
                    provider.key_env().unwrap_or("the key env"),
                    provider.key_file_stem().unwrap_or("<provider>")
                );
                return ExitCode::FAILURE;
            }
        }
    } else {
        String::new()
    };

    let llm = match oxidemx_agent::factory::provider_from_config(provider, &model, &key) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("✗ provider error: {e}");
            return ExitCode::FAILURE;
        }
    };

    // A tool-less, context-less single-turn agent. The system prompt is
    // either the user's `--system` or a terse one-shot default.
    let system = opts.system.clone().unwrap_or_else(|| {
        "You are a precise one-shot assistant. Follow the instruction exactly and \
         output only the result — no preamble, no commentary."
            .to_string()
    });
    let cancel = CancellationToken::new();
    install_signal_handler(cancel.clone());
    let agent = StepAgent::new("oneshot".into(), system, vec![], cancel.clone());

    match build_and_run(llm, agent, &prompt, 1, &cancel).await {
        Ok(out) => {
            println!("{}", out.trim_end());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("✗ oneshot failed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The trigger engine's heartbeat: run every flow whose
/// `[triggers] schedule` is due now (driven by a systemd user timer).
/// Flows with required-but-unsupplied inputs are skipped (a scheduled
/// run can't prompt). Uses the configured real provider.
async fn cmd_tick(args: &[String]) -> ExitCode {
    let opts = parse_opts(args);
    let flows_root = opts.flows_dir.clone().unwrap_or_else(default_flows_root);
    let agents_root = opts.agents_dir.clone().unwrap_or_else(default_agents_root);
    let now = unix_secs();
    let offset = local_offset_secs();

    let provider = opts.provider.unwrap_or(AiProvider::Gemini);
    let key = if opts.mock || !provider.needs_key() {
        String::new()
    } else {
        match oxidemx_agent::keys::provider_key(provider) {
            Some(k) => k,
            None => {
                eprintln!("tick: no API key for {}; nothing run.", provider.label());
                return ExitCode::SUCCESS;
            }
        }
    };
    let allowlist = load_allowlist();
    let mut ran = 0usize;

    for id in list_flows(&flows_root) {
        let Ok((doc, roster)) = load_flow(&flows_root, &agents_root, &id) else {
            continue;
        };
        let schedules: Vec<Schedule> = doc
            .manifest
            .triggers
            .schedule
            .iter()
            .filter_map(|s| Schedule::parse(s))
            .collect();
        if schedules.is_empty() {
            continue;
        }
        let last = last_run_secs(&id);
        if !schedules.iter().any(|s| s.is_due(now, last, offset)) {
            continue;
        }
        let plan = match validate(&doc, &roster, KNOWN_TOOLS) {
            Ok(p) => p,
            Err(_) => {
                eprintln!("tick: skipping `{id}` (invalid)");
                continue;
            }
        };
        let inputs = match resolve_inputs(&plan, &BTreeMap::new()) {
            Ok(i) => i,
            Err(missing) => {
                eprintln!("tick: skipping `{id}` (needs inputs: {})", missing.join(", "));
                continue;
            }
        };
        let run_id = format!("{id}-{}", unix_millis());
        let workdir = runs_root().join(&run_id);
        if std::fs::create_dir_all(&workdir).is_err() {
            continue;
        }
        eprintln!("tick: running `{id}` (run {run_id})");
        let factory: Arc<dyn ProviderFactory> = if opts.mock {
            Arc::new(FixedFactory(MockProvider::echoing()))
        } else {
            Arc::new(ConfigFactory {
                provider,
                api_key: key.clone(),
            })
        };
        let run_opts = RunOptions {
            run_id: run_id.clone(),
            inputs,
            workdir: workdir.clone(),
            roster,
            factory,
            cancel: CancellationToken::new(),
            approval: ApprovalPolicy::parse(&plan.doc.manifest.defaults.approval),
            allowlist: allowlist.clone(),
            conversation_id: String::new(),
        };
        let outcome = run_flow(&plan, run_opts, Arc::new(JsonLinesSink)).await;
        let record = serde_json::json!({
            "run_id": outcome.run_id, "flow_id": id,
            "success": outcome.success, "artifacts": outcome.artifacts,
            "error": outcome.error, "trigger": "schedule",
        });
        let _ = std::fs::write(
            workdir.join("run.json"),
            serde_json::to_string_pretty(&record).unwrap_or_default(),
        );
        ran += 1;
    }
    eprintln!("tick: {ran} due flow(s) ran.");
    ExitCode::SUCCESS
}

/// Latest run time (unix seconds) for a flow, parsed from its newest
/// `<flow>-<unix_millis>` run dir; `None` if it never ran.
fn last_run_secs(flow_id: &str) -> Option<i64> {
    let root = runs_root();
    let prefix = format!("{flow_id}-");
    let mut newest: Option<u128> = None;
    for e in std::fs::read_dir(&root).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(ms) = name.strip_prefix(&prefix).and_then(|s| s.parse::<u128>().ok()) {
            newest = Some(newest.map_or(ms, |n| n.max(ms)));
        }
    }
    newest.map(|ms| (ms / 1000) as i64)
}

/// Local UTC offset in seconds, via `date +%z` (avoids a tz crate).
fn local_offset_secs() -> i64 {
    let out = std::process::Command::new("date").arg("+%z").output();
    let z = out
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    // Format: ±HHMM
    if z.len() == 5 {
        let sign = if z.starts_with('-') { -1 } else { 1 };
        let h: i64 = z[1..3].parse().unwrap_or(0);
        let m: i64 = z[3..5].parse().unwrap_or(0);
        sign * (h * 3600 + m * 60)
    } else {
        0
    }
}

fn unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn cmd_status(args: &[String]) -> ExitCode {
    let root = runs_root();
    let records = collect_run_records(&root);
    if let Some(run_id) = args.first().filter(|a| !a.starts_with("--")) {
        match records.iter().find(|r| r["run_id"] == serde_json::json!(run_id)) {
            Some(r) => println!("{}", serde_json::to_string_pretty(r).unwrap_or_default()),
            None => {
                eprintln!("no run `{run_id}` under {}", root.display());
                return ExitCode::FAILURE;
            }
        }
    } else if records.is_empty() {
        eprintln!("no runs recorded under {}", root.display());
    } else {
        for r in records.iter().rev().take(20) {
            println!(
                "{}  {}  {}",
                r["run_id"].as_str().unwrap_or("?"),
                if r["success"].as_bool().unwrap_or(false) { "ok  " } else { "FAIL" },
                r["flow_id"].as_str().unwrap_or("?"),
            );
        }
    }
    ExitCode::SUCCESS
}

// ── option parsing ───────────────────────────────────────────────────

#[derive(Default)]
struct Opts {
    inputs: BTreeMap<String, String>,
    mock: bool,
    provider: Option<AiProvider>,
    model: Option<String>,
    system: Option<String>,
    workdir: Option<PathBuf>,
    flows_dir: Option<PathBuf>,
    agents_dir: Option<PathBuf>,
    stages: bool,
    /// Non-flag arguments (e.g. the oneshot prompt tokens).
    positionals: Vec<String>,
}

fn parse_opts(args: &[String]) -> Opts {
    let mut o = Opts::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--input" => {
                if let Some(kv) = args.get(i + 1) {
                    if let Some((k, v)) = kv.split_once('=') {
                        o.inputs.insert(k.to_string(), v.to_string());
                    }
                    i += 1;
                }
            }
            "--mock" => o.mock = true,
            "--provider" => {
                if let Some(p) = args.get(i + 1) {
                    o.provider = parse_provider(p);
                    i += 1;
                }
            }
            "--model" => {
                o.model = args.get(i + 1).cloned();
                i += 1;
            }
            "--system" => {
                o.system = args.get(i + 1).cloned();
                i += 1;
            }
            "--workdir" => {
                o.workdir = args.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--flows-dir" => {
                o.flows_dir = args.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--agents-dir" => {
                o.agents_dir = args.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--stages" => o.stages = true,
            other => o.positionals.push(other.to_string()),
        }
        i += 1;
    }
    o
}

fn parse_provider(s: &str) -> Option<AiProvider> {
    match s.to_ascii_lowercase().as_str() {
        "gemini" => Some(AiProvider::Gemini),
        "openai" => Some(AiProvider::OpenAi),
        "anthropic" => Some(AiProvider::Anthropic),
        "ollama" => Some(AiProvider::Ollama),
        "mistral_rs" | "mistralrs" | "mistral-rs" => Some(AiProvider::MistralRs),
        "claude_code" | "claudecode" | "claude-code" => Some(AiProvider::ClaudeCode),
        _ => None,
    }
}

// ── paths / env ──────────────────────────────────────────────────────

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

fn collect_run_records(root: &std::path::Path) -> Vec<serde_json::Value> {
    let mut recs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for d in dirs {
            if let Ok(s) = std::fs::read_to_string(d.join("run.json")) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                    recs.push(v);
                }
            }
        }
    }
    recs
}

fn load_allowlist() -> Vec<String> {
    oxidemx_shared::config::default_config_path()
        .and_then(|p| oxidemx_shared::AppConfig::load_from(&p).ok())
        .map(|c| c.overlay.ai.command_allowlist)
        .unwrap_or_default()
}

fn unix_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn install_signal_handler(cancel: CancellationToken) {
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("\n⏹ cancelling run…");
            cancel.cancel();
        }
    });
}
