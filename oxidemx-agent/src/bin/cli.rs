//! Headless harness: a ReAct agent with `execute_command` against
//! any configured provider.
//!
//! ```text
//! oxidemx-agent-cli "<prompt>" [--provider gemini|openai|anthropic|ollama|claude_code] [--model <id>] [--allow "<entry>"]...
//! ```
//!
//! Provider + model + allowlist default to the live config.json; keys
//! resolve via `oxidemx_agent::keys` (env var → per-provider key file).

use autoagents::core::agent::memory::SlidingWindowMemory;
use autoagents::core::agent::prebuilt::executor::ReActAgent;
use autoagents::core::agent::task::Task;
use autoagents::core::agent::{AgentBuilder, DirectAgent};
use autoagents_derive::{agent, AgentHooks};

use oxidemx_agent::factory::provider_from_config;
use oxidemx_agent::keys::provider_key;
use oxidemx_agent::tools::{set_allowlist, ExecuteCommand};
use oxidemx_shared::config::AiProvider;

#[agent(
    name = "shell_agent",
    description = "You are a careful Linux system assistant. Use the execute_command tool to inspect the system; never guess command output. Commands may be denied by the user's allowlist — when that happens, explain what you wanted to run and answer with what you have. Be concise.",
    tools = [ExecuteCommand {}],
)]
#[derive(Default, Clone, AgentHooks)]
pub struct ShellAgent {}

fn parse_provider(s: &str) -> Result<AiProvider, String> {
    Ok(match s {
        "gemini" => AiProvider::Gemini,
        "openai" => AiProvider::OpenAi,
        "anthropic" => AiProvider::Anthropic,
        "ollama" => AiProvider::Ollama,
        "claude_code" | "claude-code" => AiProvider::ClaudeCode,
        other => return Err(format!("unknown provider {other:?}")),
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cfg = oxidemx_shared::config::default_config_path()
        .and_then(|p| oxidemx_shared::AppConfig::load_from(&p).ok())
        .map(|c| c.overlay.ai)
        .unwrap_or_default();

    let mut args = std::env::args().skip(1);
    let mut prompt = None;
    let mut model = cfg.model.clone();
    let mut provider = cfg.provider;
    let mut model_overridden = false;
    let mut allow = cfg.command_allowlist.clone();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => {
                model = args.next().ok_or("--model needs a value")?;
                model_overridden = true;
            }
            "--allow" => allow.push(args.next().ok_or("--allow needs a value")?),
            "--provider" => {
                provider = parse_provider(&args.next().ok_or("--provider needs a value")?)?;
                // Default the model to the new provider's default unless
                // the user also passed --model.
                if !model_overridden {
                    model = provider.default_model().to_string();
                }
            }
            _ if prompt.is_none() => prompt = Some(a),
            other => return Err(format!("unexpected argument: {other}").into()),
        }
    }
    let prompt = prompt.ok_or(
        "usage: oxidemx-agent-cli \"<prompt>\" [--provider gemini|openai|anthropic|ollama|claude_code] [--model <id>] [--allow <entry>]...",
    )?;

    set_allowlist(allow);
    let key = if provider.needs_key() {
        provider_key(provider).ok_or_else(|| {
            format!("no API key for {provider:?}; set the env var or ~/.config/oxidemx/<provider>.key")
        })?
    } else {
        String::new()
    };
    eprintln!("[provider: {provider:?}, model: {model}]");
    let provider = provider_from_config(provider, &model, &key)?;

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
