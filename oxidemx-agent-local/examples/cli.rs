//! Live CLI harness for `oxidemx-agent-local`.
//!
//! Exercises load → chat → status → idle-unload → reload in a single run.
//!
//! **Not CI.**  Needs a real model file and enough VRAM/RAM to load it.
//!
//! # Run
//!
//! ```bash
//! cargo run -p oxidemx-agent-local --features mistral --example cli -- \
//!     <alias> <gguf-path-or-hf-repo> "your prompt here"
//! ```
//!
//! For Metal (macOS GPU) acceleration replace `--features mistral` with
//! `--features metal`.  For CUDA replace with `--features cuda`.
//!
//! # Arguments
//!
//! - `<alias>`              — short name for the model (e.g. `phi3`)
//! - `<gguf-path-or-repo>`  — local `.gguf` file path **or** HF repo id
//!                            (e.g. `microsoft/Phi-3-mini-4k-instruct-gguf`)
//! - `"<prompt>"`           — the user message to send
//!
//! If the second argument ends with `.gguf`, the example treats it as a local
//! GGUF file (parent directory + filename split automatically).  Otherwise it
//! is treated as an HF repo id.

// ── Feature-gated body ────────────────────────────────────────────────────────

#[cfg(feature = "mistral")]
mod inner {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use oxidemx_agent_local::{
        LocalModelManager, LocalModelService,
        mistral::MistralEngine,
        types::{ChatRequest, Message, Role},
        mode::Mode,
    };
    use oxidemx_shared::config::{Capabilities, ModelSource, ModelSpec, SamplingConfig};
    use tokio::time::Instant;

    pub async fn run() {
        let args: Vec<String> = std::env::args().collect();
        if args.len() < 4 {
            eprintln!(
                "usage: {} <alias> <gguf-path-or-hf-repo> \"prompt\"",
                args[0]
            );
            eprintln!(
                "\nexample:\n  cargo run -p oxidemx-agent-local --features mistral \
                 --example cli -- phi3 /models/phi3.gguf \"What is Rust?\""
            );
            std::process::exit(1);
        }

        let alias = args[1].clone();
        let model_arg = args[2].clone();
        let prompt = args[3].clone();

        // ── Build ModelSpec ───────────────────────────────────────────────────

        let source = if model_arg.ends_with(".gguf") {
            // Local GGUF file — split into dir + filename.
            let path = PathBuf::from(&model_arg);
            let dir = path
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string());
            let file = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| model_arg.clone());
            ModelSource::Gguf {
                dir,
                files: vec![file],
            }
        } else {
            // HF repo id.
            ModelSource::Hf {
                repo: model_arg.clone(),
                revision: None,
            }
        };

        let spec = ModelSpec {
            alias: alias.clone(),
            source,
            capabilities: Capabilities::empty(),
            sampling: SamplingConfig::default(),
            isq: None,
            keep_resident: false,
            ctx_window: None,
        };

        println!("[cli] alias={alias} model={model_arg}");
        println!("[cli] prompt={prompt:?}");
        println!();

        // ── Build MistralEngine ───────────────────────────────────────────────

        // Download dir: ~/.cache/oxidemx-local (created if absent).
        let download_dir = dirs_home().join(".cache").join("oxidemx-local");
        let _ = std::fs::create_dir_all(&download_dir);

        println!("[cli] building MistralEngine …");
        let engine = match MistralEngine::new(download_dir, &[spec.clone()]).await {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[cli] ERROR: failed to build engine: {e}");
                std::process::exit(1);
            }
        };

        // ── Build LocalModelManager ───────────────────────────────────────────

        // Idle timeout of 10 s so the demo can demonstrate unload quickly.
        let idle_timeout = Duration::from_secs(10);
        let manager = Arc::new(LocalModelManager::with_mistral_engine(
            vec![spec],
            alias.clone(),
            idle_timeout,
            engine,
        ));

        // ── Status before load ────────────────────────────────────────────────

        println!("[cli] status before load:");
        print_status(&*manager);

        // ── ensure_loaded ─────────────────────────────────────────────────────

        println!("[cli] loading model …");
        if let Err(e) = manager.ensure_loaded(&alias).await {
            eprintln!("[cli] ERROR during ensure_loaded: {e}");
            std::process::exit(1);
        }

        println!("[cli] status after load:");
        print_status(&*manager);

        // ── chat_with_model ───────────────────────────────────────────────────

        let req = ChatRequest {
            messages: vec![Message {
                role: Role::User,
                content: prompt.clone(),
            }],
            mode: Mode::Chat,
            tools: vec![],
            sampling_override: None,
            system_template: None,
        };

        println!("[cli] running chat …");
        match manager.chat_with_model(&alias, req).await {
            Ok(resp) => {
                println!("[cli] response text: {}", resp.text);
                println!(
                    "[cli] usage: prompt={} completion={}",
                    resp.usage.prompt_tokens, resp.usage.completion_tokens
                );
                println!("[cli] guard verdict: {:?}", resp.verdict);
            }
            Err(e) => {
                eprintln!("[cli] ERROR during chat_with_model: {e}");
                std::process::exit(1);
            }
        }

        // ── idle unload demo ──────────────────────────────────────────────────

        // Simulate far-future "now" so the idle sweep fires immediately.
        let far_future = Instant::now() + Duration::from_secs(9999);
        println!();
        println!("[cli] triggering idle sweep (simulated far-future) …");
        manager.run_idle_sweep_once(far_future).await;

        println!("[cli] status after idle sweep (expect Unloaded):");
        print_status(&*manager);

        // ── reload ────────────────────────────────────────────────────────────

        println!("[cli] reloading model …");
        if let Err(e) = manager.ensure_loaded(&alias).await {
            eprintln!("[cli] ERROR during reload: {e}");
            std::process::exit(1);
        }

        println!("[cli] status after reload:");
        print_status(&*manager);

        println!();
        println!("[cli] done.");
    }

    fn print_status(svc: &dyn LocalModelService) {
        for info in svc.status() {
            println!("  {} → {:?} (last_used={:?})", info.alias, info.state, info.last_used);
        }
    }

    /// Best-effort home directory: $HOME or current dir as fallback.
    fn dirs_home() -> PathBuf {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    }
}

// ── Entry points ──────────────────────────────────────────────────────────────

#[cfg(feature = "mistral")]
#[tokio::main]
async fn main() {
    inner::run().await;
}

#[cfg(not(feature = "mistral"))]
fn main() {
    eprintln!("oxidemx-agent-local cli example: build with --features mistral (or metal/cuda/accelerate).");
    eprintln!("  cargo run -p oxidemx-agent-local --features mistral --example cli -- <alias> <gguf-or-hf-repo> \"prompt\"");
}
