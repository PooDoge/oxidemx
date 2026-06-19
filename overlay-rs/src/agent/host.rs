#![forbid(unsafe_code)]
//! D-Bus object serving `org.oxidemx.AgentHost` at `/org/oxidemx/AgentHost`.
//!
//! Each method accepts a JSON `String` (the tool args) and returns a JSON
//! `String` (the result or a clean `{"error":…}` payload).
//!
//! Two capabilities are **real** (wired to existing overlay code):
//! - `ask_multiple_choice_question` — uses [`crate::ai_client::QUESTION_TX`]
//! - `apply_menu_config` — writes the config file and notifies
//!   [`crate::ai_client::CONFIG_CHANGED_TX`]
//!
//! The remaining four (`screenshot`, `vision`, `clipboard`,
//! `current_window`) are **unsupported** from a D-Bus handler context and
//! return a structured `{"error":…}` so callers get a clean, parseable
//! response rather than a D-Bus fault.

use tokio::sync::mpsc;
use zbus::{fdo, interface};

use crate::ai_client::{PendingQuestion, CONFIG_CHANGED_TX, QUESTION_TX};

// ─── helper ──────────────────────────────────────────────────────────────────

/// Resolve the oxidemx config file path (same logic as `ai_client`).
fn config_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    std::path::Path::new(&home).join(".config/oxidemx/config.json")
}

// ─── AgentHostService ────────────────────────────────────────────────────────

/// Unit struct — all state is accessed through the module-level globals.
pub struct AgentHostService;

#[interface(name = "org.oxidemx.AgentHost")]
impl AgentHostService {
    // ── unsupported stubs ─────────────────────────────────────────────────────

    /// Screenshot capture is only possible inside the iced compositor loop.
    async fn screenshot(&self, _args_json: String) -> fdo::Result<String> {
        Ok(r#"{"error":"unsupported","reason":"screenshot requires iced window compositor access"}"#
            .to_string())
    }

    /// Vision analysis is only possible inside the iced compositor loop.
    async fn vision(&self, _args_json: String) -> fdo::Result<String> {
        Ok(r#"{"error":"unsupported","reason":"screenshot requires iced window compositor access"}"#
            .to_string())
    }

    /// Clipboard access requires an iced message dispatch.
    async fn clipboard(&self, _args_json: String) -> fdo::Result<String> {
        Ok(r#"{"error":"unsupported","reason":"clipboard access requires iced message dispatch"}"#
            .to_string())
    }

    /// Querying the active compositor window is not implemented.
    async fn current_window(&self, _args_json: String) -> fdo::Result<String> {
        Ok(r#"{"error":"unsupported","reason":"active window query not implemented"}"#.to_string())
    }

    // ── real capabilities ─────────────────────────────────────────────────────

    /// Push a multiple-choice question to the overlay chat UI and await
    /// the user's selection.
    ///
    /// Expected `args_json` shape:
    /// ```json
    /// {"question":"Which option?","options":["A","B","C"]}
    /// ```
    async fn ask_multiple_choice_question(
        &self,
        args_json: String,
    ) -> fdo::Result<String> {
        // Parse args.
        let parsed: serde_json::Value =
            serde_json::from_str(&args_json).map_err(|e| {
                fdo::Error::InvalidArgs(format!("invalid JSON args: {e}"))
            })?;

        let question = parsed["question"]
            .as_str()
            .ok_or_else(|| fdo::Error::InvalidArgs("missing 'question' field".into()))?
            .to_string();

        let options: Vec<String> = parsed["options"]
            .as_array()
            .ok_or_else(|| fdo::Error::InvalidArgs("missing 'options' array".into()))?
            .iter()
            .enumerate()
            .map(|(i, v)| {
                v.as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("option_{i}"))
            })
            .collect();

        // Grab the sender without holding the lock across the await.
        let tx_opt = QUESTION_TX
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        let Some(tx) = tx_opt else {
            return Ok(r#"{"error":"not yet initialized"}"#.to_string());
        };

        // Create a one-shot-style response channel and send the question.
        let (resp_tx, mut resp_rx) = mpsc::channel::<String>(1);
        tx.send(PendingQuestion {
            question,
            options,
            response_tx: resp_tx,
        })
        .await
        .map_err(|e| fdo::Error::Failed(format!("failed to send question: {e}")))?;

        // Await response (no lock held).
        match resp_rx.recv().await {
            Some(answer) => {
                let payload = serde_json::json!({"answer": answer});
                Ok(payload.to_string())
            }
            None => Ok(r#"{"error":"response channel closed"}"#.to_string()),
        }
    }

    /// Apply a new menu configuration JSON string, writing it to disk and
    /// notifying the running overlay.
    ///
    /// Expected `args_json` shape:
    /// ```json
    /// {"config_json": "{\"slices\": [...], ...}"}
    /// ```
    async fn apply_menu_config(&self, args_json: String) -> fdo::Result<String> {
        // Parse the outer wrapper.
        let parsed: serde_json::Value =
            serde_json::from_str(&args_json).map_err(|e| {
                fdo::Error::InvalidArgs(format!("invalid JSON args: {e}"))
            })?;

        let config_json = parsed["config_json"]
            .as_str()
            .ok_or_else(|| {
                fdo::Error::InvalidArgs("missing 'config_json' string field".into())
            })?;

        // Validate the inner JSON before writing.
        let _: serde_json::Value = serde_json::from_str(config_json).map_err(|e| {
            fdo::Error::InvalidArgs(format!("config_json is not valid JSON: {e}"))
        })?;

        let path = config_path();

        // Create parent directories if needed.
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                fdo::Error::Failed(format!("failed to create config dir: {e}"))
            })?;
        }

        // Notify the UI loop (best-effort — if the channel isn't set yet we
        // skip silently, the file write still happens).
        {
            let tx_opt = CONFIG_CHANGED_TX
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(tx) = tx_opt {
                // Non-blocking send; if the buffer is full we skip.
                let _ = tx.send(config_json.to_string()).await;
            }
        }

        tokio::fs::write(&path, config_json).await.map_err(|e| {
            fdo::Error::Failed(format!("failed to write config: {e}"))
        })?;

        Ok(r#"{"ok":true}"#.to_string())
    }
}
