//! `GeminiInteractionsProvider` — the Gemini **Interactions API**
//! (`v1beta/interactions`) behind AutoAgents' `LLMProvider` traits.
//!
//! Session model (spec Part I §3.2, hybrid option a): the API is
//! server-side stateful, so this provider keeps the interaction id
//! returned by each round and sends only the NEWEST message as
//! `input` with `previous_interaction_id` for continuity. AutoAgents
//! executors ship full history per call; everything before the
//! newest turn is already known to the server and is dropped here.
//!
//! CONSTRAINT: one provider instance per agent/conversation. Sharing
//! an instance across agents interleaves their server-side sessions.
//!
//! Stateless worker mode (`store_session(false)`, hybrid option b)
//! sends `store: false` and no session id — for short-lived flow
//! workers that need reproducible context.

pub mod sse;
pub mod wire;

use std::fmt;
use std::sync::Arc;

use autoagents::async_trait;
use autoagents::llm::chat::{
    ChatMessage, ChatProvider, ChatResponse, StructuredOutputFormat, Tool,
};
use autoagents::llm::completion::{CompletionProvider, CompletionRequest, CompletionResponse};
use autoagents::llm::embedding::EmbeddingProvider;
use autoagents::llm::error::LLMError;
use autoagents::llm::models::ModelsProvider;
use autoagents::llm::{FunctionCall, LLMProvider, ToolCall};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use sse::{DeltaSender, RoundOutcome};

/// The Interactions endpoint (same as the production overlay client).
pub const INTERACTIONS_URL: &str = "https://generativelanguage.googleapis.com/v1beta/interactions";

pub const DEFAULT_MODEL: &str = "gemini-2.5-flash";
pub const PRO_MODEL: &str = "gemini-2.5-pro";

pub struct GeminiInteractionsProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    /// `previous_interaction_id` of the live server-side session.
    session: tokio::sync::Mutex<Option<String>>,
    /// false ⇒ stateless worker mode (`store: false`, no session).
    store: bool,
    /// Live text deltas (SSE) stream out here when attached.
    delta_sink: Option<DeltaSender>,
    /// Cancels in-flight HTTP work mid-round — the seam AutoAgents
    /// lacks upstream (spec Part II §7.3).
    pub cancel: CancellationToken,
}

impl GeminiInteractionsProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client"),
            api_key: api_key.into(),
            model: model.into(),
            session: tokio::sync::Mutex::new(None),
            store: true,
            delta_sink: None,
            cancel: CancellationToken::new(),
        })
    }

    /// Builder-style toggles (call before the Arc is shared).
    pub fn with_session_store(mut self: Arc<Self>, store: bool) -> Arc<Self> {
        Arc::get_mut(&mut self).expect("unshared").store = store;
        self
    }

    pub fn with_delta_sink(mut self: Arc<Self>, sink: DeltaSender) -> Arc<Self> {
        Arc::get_mut(&mut self).expect("unshared").delta_sink = Some(sink);
        self
    }

    /// Request body for one round. `prev` is the session id, if any.
    fn request_body(
        &self,
        input: Value,
        tools: &[Value],
        system: Option<&str>,
        prev: Option<&str>,
    ) -> Value {
        let mut body = json!({
            "model": self.model,
            "input": input,
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
        }
        if let Some(s) = system {
            body["system_instruction"] = json!(s);
        }
        if self.store {
            if let Some(p) = prev {
                body["previous_interaction_id"] = json!(p);
            }
        } else {
            body["store"] = json!(false);
        }
        body
    }

    /// One blocking JSON round (port of overlay `blocking_round`,
    /// `ai_client.rs:437-459`), cancellable.
    async fn blocking_round(&self, req_body: &Value) -> Result<RoundOutcome, LLMError> {
        let request = self
            .client
            .post(INTERACTIONS_URL)
            .header("x-goog-api-key", &self.api_key)
            .json(req_body)
            .send();
        let res = tokio::select! {
            r = request => r.map_err(|e| LLMError::HttpError(e.to_string()))?,
            _ = self.cancel.cancelled() => return Err(LLMError::Generic("cancelled".into())),
        };
        let status = res.status();
        if !status.is_success() {
            let error_text = res.text().await.unwrap_or_default();
            let detail = serde_json::from_str::<Value>(&error_text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(String::from))
                .unwrap_or(error_text);
            return Err(LLMError::ProviderError(format!(
                "API error ({status}): {detail}"
            )));
        }
        let body: Value = res
            .json()
            .await
            .map_err(|e| LLMError::ResponseFormatError {
                message: e.to_string(),
                raw_response: String::new(),
            })?;
        Ok(parse_interaction_body(&body))
    }

    /// One round, streaming over SSE when a sink is attached, with
    /// transparent fallback to the blocking path on SSE hiccups
    /// (same recovery the overlay ships).
    async fn round(&self, req_body: &Value) -> Result<RoundOutcome, LLMError> {
        if self.delta_sink.is_some() {
            match sse::stream_round(
                &self.client,
                &self.api_key,
                req_body,
                &self.delta_sink,
                &self.cancel,
            )
            .await
            {
                Ok(o) => return Ok(o),
                Err(e) if e.to_string().starts_with("API error") => {
                    return Err(LLMError::ProviderError(e.to_string()));
                }
                Err(e) if e.to_string() == "cancelled" => {
                    return Err(LLMError::Generic("cancelled".into()));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "SSE round failed; falling back to blocking call");
                }
            }
        }
        self.blocking_round(req_body).await
    }
}

/// Parse a complete (non-SSE) interaction body into a round outcome.
/// Function calls are pending only while `status == "waiting"`.
fn parse_interaction_body(body: &Value) -> RoundOutcome {
    let mut text = String::new();
    for step in body["steps"].as_array().into_iter().flatten() {
        if step["type"] == "model_output" {
            for content in step["content"].as_array().into_iter().flatten() {
                if content["type"] == "text" {
                    if let Some(t) = content["text"].as_str() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
            }
        }
    }
    let mut out = RoundOutcome {
        id: body["id"].as_str().map(String::from),
        status: body["status"].as_str().unwrap_or_default().to_string(),
        text,
        calls: Vec::new(),
    };
    for step in body["steps"].as_array().into_iter().flatten() {
        if step["type"] == "function_call" && step["status"] == "waiting" {
            out.calls.push((
                step["id"].as_str().unwrap_or_default().to_string(),
                step["name"].as_str().unwrap_or_default().to_string(),
                step.get("arguments").cloned().unwrap_or(json!({})),
            ));
        }
    }
    out
}

/// `ChatResponse` adapter over one round.
#[derive(Debug)]
pub struct InteractionsResponse(pub RoundOutcome);

impl fmt::Display for InteractionsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.text)
    }
}

impl ChatResponse for InteractionsResponse {
    fn text(&self) -> Option<String> {
        if self.0.text.is_empty() {
            None
        } else {
            Some(self.0.text.clone())
        }
    }

    fn tool_calls(&self) -> Option<Vec<ToolCall>> {
        if self.0.calls.is_empty() {
            return None;
        }
        Some(
            self.0
                .calls
                .iter()
                .map(|(id, name, args)| ToolCall {
                    id: id.clone(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: name.clone(),
                        arguments: args.to_string(),
                    },
                })
                .collect(),
        )
    }
}

#[async_trait]
impl ChatProvider for GeminiInteractionsProvider {
    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Tool]>,
        _json_schema: Option<StructuredOutputFormat>,
    ) -> Result<Box<dyn ChatResponse>, LLMError> {
        let (input, system) = wire::newest_input(messages);
        let tool_decls = tools.map(wire::tools_to_interactions).unwrap_or_default();

        let mut session = self.session.lock().await;
        let body = self.request_body(input, &tool_decls, system.as_deref(), session.as_deref());
        let outcome = self.round(&body).await?;

        // Thread the session id; terminal failure statuses surface
        // as provider errors so the executor stops cleanly.
        if self.store {
            if let Some(id) = &outcome.id {
                *session = Some(id.clone());
            }
        }
        match outcome.status.as_str() {
            "completed" | "requires_action" => Ok(Box::new(InteractionsResponse(outcome))),
            other => Err(LLMError::ProviderError(format!(
                "Interaction ended with status '{other}'"
            ))),
        }
    }
}

#[async_trait]
impl CompletionProvider for GeminiInteractionsProvider {
    async fn complete(
        &self,
        _req: &CompletionRequest,
        _json_schema: Option<StructuredOutputFormat>,
    ) -> Result<CompletionResponse, LLMError> {
        Err(LLMError::ProviderError(
            "completion API not supported by the Interactions transport (P0)".into(),
        ))
    }
}

#[async_trait]
impl EmbeddingProvider for GeminiInteractionsProvider {
    async fn embed(&self, _input: Vec<String>) -> Result<Vec<Vec<f32>>, LLMError> {
        Err(LLMError::ProviderError(
            "embeddings land with the Tier-3 memory work (P2)".into(),
        ))
    }
}

impl ModelsProvider for GeminiInteractionsProvider {}

impl LLMProvider for GeminiInteractionsProvider {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_body_extracts_text_status_and_waiting_calls() {
        let body = json!({
            "id": "intx_123",
            "status": "requires_action",
            "steps": [
                {"type": "model_output", "content": [{"type": "text", "text": "Let me check."}]},
                {"type": "function_call", "status": "waiting", "id": "call_1",
                 "name": "execute_command", "arguments": {"command": "systemctl --user status oxidemx"}},
                {"type": "function_call", "status": "completed", "id": "call_0",
                 "name": "older_tool", "arguments": {}}
            ]
        });
        let out = parse_interaction_body(&body);
        assert_eq!(out.id.as_deref(), Some("intx_123"));
        assert_eq!(out.status, "requires_action");
        assert_eq!(out.text, "Let me check.");
        // Only the WAITING call is pending; completed ones are history.
        assert_eq!(out.calls.len(), 1);
        assert_eq!(out.calls[0].1, "execute_command");
    }

    #[test]
    fn response_adapter_maps_calls_to_autoagents_toolcalls() {
        let out = RoundOutcome {
            id: Some("i".into()),
            status: "requires_action".into(),
            text: String::new(),
            calls: vec![(
                "call_1".into(),
                "execute_command".into(),
                json!({"command": "wpctl status"}),
            )],
        };
        let resp = InteractionsResponse(out);
        assert_eq!(resp.text(), None);
        let calls = resp.tool_calls().expect("calls");
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].call_type, "function");
        assert_eq!(calls[0].function.name, "execute_command");
        let args: Value = serde_json::from_str(&calls[0].function.arguments).unwrap();
        assert_eq!(args["command"], "wpctl status");
    }

    #[tokio::test]
    async fn request_body_threads_session_and_store_modes() {
        let p = GeminiInteractionsProvider::new("k", DEFAULT_MODEL);
        // Session mode: previous_interaction_id present, no store field.
        let b = p.request_body(json!("hi"), &[], Some("sys"), Some("intx_9"));
        assert_eq!(b["previous_interaction_id"], "intx_9");
        assert_eq!(b["system_instruction"], "sys");
        assert!(b.get("store").is_none());
        assert!(b.get("tools").is_none());

        // Stateless worker mode: store:false, never a session id.
        let p = GeminiInteractionsProvider::new("k", DEFAULT_MODEL).with_session_store(false);
        let b = p.request_body(json!("hi"), &[json!({"type":"function"})], None, Some("ignored"));
        assert_eq!(b["store"], json!(false));
        assert!(b.get("previous_interaction_id").is_none());
        assert_eq!(b["tools"].as_array().unwrap().len(), 1);
    }
}
