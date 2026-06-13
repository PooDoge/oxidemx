//! P0 spike harness: a ReAct agent with `execute_command` against
//! the Gemini Interactions provider.
//!
//! ```text
//! oxidemx-agent-cli "<prompt>" [--model <id>] [--allow "<entry>"]...
//! ```
//!
//! API key lookup mirrors the overlay (`ai_client.rs:124-149`):
//! `GEMINI_API_KEY` env, else `~/.config/oxidemx/gemini.key`.


use autoagents::core::agent::memory::SlidingWindowMemory;
use autoagents::core::agent::prebuilt::executor::ReActAgent;
use autoagents::core::agent::task::Task;
use autoagents::core::agent::{AgentBuilder, DirectAgent};
use autoagents_derive::{agent, AgentHooks};

use oxidemx_agent::factory::provider_from_config;
use oxidemx_agent::tools::{set_allowlist, ExecuteCommand};
use oxidemx_shared::config::AiBackend;

#[agent(
    name = "shell_agent",
    description = "You are a careful Linux system assistant. Use the execute_command tool to inspect the system; never guess command output. Commands may be denied by the user's allowlist — when that happens, explain what you wanted to run and answer with what you have. Be concise.",
    tools = [ExecuteCommand {}],
)]
#[derive(Default, Clone, AgentHooks)]
pub struct ShellAgent {}

fn api_key() -> Result<String, String> {
    if let Ok(k) = std::env::var("GEMINI_API_KEY") {
        let k = k.trim().to_string();
        if !k.is_empty() {
            return Ok(k);
        }
    }
    let path = key_path().ok_or("no home directory")?;
    std::fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .map_err(|_| {
            format!(
                "Gemini API key not found. Set GEMINI_API_KEY or save it in {}",
                path.display()
            )
        })
}

fn key_path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config/oxidemx/gemini.key"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Defaults come from the live config.json (backend, model,
    // allowlist) so the CLI behaves like the rest of the app; flags
    // override per-invocation.
    let cfg = oxidemx_shared::config::default_config_path()
        .and_then(|p| oxidemx_shared::AppConfig::load_from(&p).ok())
        .map(|c| c.overlay.ai)
        .unwrap_or_default();

    let mut args = std::env::args().skip(1);
    let mut prompt = None;
    let mut model = cfg.model.clone();
    let mut backend = cfg.backend;
    let mut allow = cfg.command_allowlist.clone();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => model = args.next().ok_or("--model needs a value")?,
            "--allow" => allow.push(args.next().ok_or("--allow needs a value")?),
            "--backend" => {
                backend = match args.next().as_deref() {
                    Some("interactions") => AiBackend::Interactions,
                    Some("generate_content") => AiBackend::GenerateContent,
                    other => return Err(format!("--backend must be interactions|generate_content, got {other:?}").into()),
                }
            }
            _ if prompt.is_none() => prompt = Some(a),
            other => return Err(format!("unexpected argument: {other}").into()),
        }
    }
    let prompt = prompt.ok_or(
        "usage: oxidemx-agent-cli \"<prompt>\" [--model <id>] [--backend interactions|generate_content] [--allow <entry>]...",
    )?;

    set_allowlist(allow);
    eprintln!("[backend: {backend:?}, model: {model}]");
    let provider = provider_from_config(backend, &model, &api_key()?)?;

    let handle = AgentBuilder::<_, DirectAgent>::new(ReActAgent::new(ShellAgent {}))
        .llm(provider)
        .memory(Box::new(SlidingWindowMemory::new(10)))
        .build()
        .await?;

    // Surface executor events (tool calls etc.) as they happen.
    let mut rx = handle.rx;
    tokio::spawn(async move {
        use futures::StreamExt;
        while let Some(event) = rx.next().await {
            eprintln!("[event] {event:?}");
        }
    });

    // Output type defaults to String when the #[agent] macro gets no
    // `output =` — fine for a harness; tool activity shows via events.
    let result = handle.agent.run(Task::new(prompt)).await?;
    println!("{result}");
    Ok(())
}
