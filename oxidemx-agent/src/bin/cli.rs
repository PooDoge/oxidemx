//! P0 spike harness: a ReAct agent with `execute_command` against
//! the Gemini Interactions provider.
//!
//! ```text
//! oxidemx-agent-cli "<prompt>" [--model <id>] [--allow "<entry>"]...
//! ```
//!
//! API key lookup mirrors the overlay (`ai_client.rs:124-149`):
//! `GEMINI_API_KEY` env, else `~/.config/oxidemx/gemini.key`.

use std::sync::Arc;

use autoagents::core::agent::memory::SlidingWindowMemory;
use autoagents::core::agent::prebuilt::executor::ReActAgent;
use autoagents::core::agent::task::Task;
use autoagents::core::agent::{AgentBuilder, DirectAgent};
use autoagents_derive::{agent, AgentHooks};

use oxidemx_agent::provider::{GeminiInteractionsProvider, DEFAULT_MODEL};
use oxidemx_agent::tools::{set_allowlist, ExecuteCommand};

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
    let mut args = std::env::args().skip(1);
    let mut prompt = None;
    let mut model = DEFAULT_MODEL.to_string();
    // Overlay defaults (oxidemx-shared AiConfig::default).
    let mut allow: Vec<String> = ["brightnessctl", "wpctl", "systemctl --user"]
        .map(String::from)
        .to_vec();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => model = args.next().ok_or("--model needs a value")?,
            "--allow" => allow.push(args.next().ok_or("--allow needs a value")?),
            _ if prompt.is_none() => prompt = Some(a),
            other => return Err(format!("unexpected argument: {other}").into()),
        }
    }
    let prompt =
        prompt.ok_or("usage: oxidemx-agent-cli \"<prompt>\" [--model <id>] [--allow <entry>]...")?;

    set_allowlist(allow);
    let provider = GeminiInteractionsProvider::new(api_key()?, model);

    let handle = AgentBuilder::<_, DirectAgent>::new(ReActAgent::new(ShellAgent {}))
        .llm(provider as Arc<dyn autoagents::llm::LLMProvider>)
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
