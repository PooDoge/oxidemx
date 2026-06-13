//! Claude Code CLI provider — uses the local `claude` binary (and
//! the user's Claude subscription) as a chat backend, no API key.
//!
//! Constraint: Claude Code is itself an agent with its own tools, so
//! this path is **chat-only** — our agent tools (execute_command,
//! memory, …) do not apply. `tool_calls()` always returns `None`, so
//! the ReAct loop ends after one response. For tool-calling Claude,
//! use the `Anthropic` API provider instead.
//!
//! Invocation (verified live, claude 2.1): `claude -p <prompt>
//! --output-format json --disallowed-tools <side-effecting set>
//! [--system-prompt <sys>] [--model <m>]`. `--output-format json`
//! prints a JSON array of transcript messages; the `type=="result"`
//! entry carries the reply in its `result` field.

use std::fmt;

use autoagents::async_trait;
use autoagents::llm::chat::{
    ChatMessage, ChatProvider, ChatResponse, ChatRole, StructuredOutputFormat, Tool,
};
use autoagents::llm::completion::{CompletionProvider, CompletionRequest, CompletionResponse};
use autoagents::llm::embedding::EmbeddingProvider;
use autoagents::llm::error::LLMError;
use autoagents::llm::models::ModelsProvider;
use autoagents::llm::{LLMProvider, ToolCall};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Tools disabled so the CLI behaves as a fast, side-effect-free
/// responder rather than touching the filesystem/network.
const DISALLOWED: &[&str] = &[
    "Bash", "Edit", "Write", "Read", "Task", "WebSearch", "WebFetch", "NotebookEdit", "Workflow",
];

pub struct ClaudeCodeProvider {
    model: Option<String>,
    pub cancel: CancellationToken,
}

impl ClaudeCodeProvider {
    pub fn new(model: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            model: model.filter(|m| !m.is_empty()),
            cancel: CancellationToken::new(),
        })
    }
}

/// Split messages into (system prompt, rendered transcript). System
/// messages join into `--system-prompt`; the rest render as a simple
/// role-prefixed transcript for the `-p` argument.
fn render(messages: &[ChatMessage]) -> (Option<String>, String) {
    let mut system = Vec::new();
    let mut convo = String::new();
    for m in messages {
        match m.role {
            ChatRole::System => system.push(m.content.clone()),
            ChatRole::User => convo.push_str(&format!("User: {}\n", m.content)),
            ChatRole::Assistant => convo.push_str(&format!("Assistant: {}\n", m.content)),
            ChatRole::Tool => convo.push_str(&format!("(tool result: {})\n", m.content)),
        }
    }
    let sys = (!system.is_empty()).then(|| system.join("\n\n"));
    (sys, convo.trim().to_string())
}

/// Extract the reply text from `claude --output-format json` output:
/// a JSON array whose `type=="result"` entry holds `.result`, or a
/// bare object with `.result`, falling back to the raw stdout.
fn extract_reply(stdout: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(stdout) {
        if let Some(arr) = v.as_array() {
            if let Some(r) = arr
                .iter()
                .rev()
                .find(|m| m.get("type").and_then(|t| t.as_str()) == Some("result"))
                .and_then(|m| m.get("result"))
                .and_then(|r| r.as_str())
            {
                return r.to_string();
            }
        } else if let Some(r) = v.get("result").and_then(|r| r.as_str()) {
            return r.to_string();
        }
    }
    stdout.trim().to_string()
}

#[derive(Debug)]
struct ClaudeCodeResponse(String);

impl fmt::Display for ClaudeCodeResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ChatResponse for ClaudeCodeResponse {
    fn text(&self) -> Option<String> {
        (!self.0.is_empty()).then(|| self.0.clone())
    }
    fn tool_calls(&self) -> Option<Vec<ToolCall>> {
        None
    }
}

#[async_trait]
impl ChatProvider for ClaudeCodeProvider {
    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        _tools: Option<&[Tool]>,
        _json_schema: Option<StructuredOutputFormat>,
    ) -> Result<Box<dyn ChatResponse>, LLMError> {
        let (system, prompt) = render(messages);
        let mut cmd = tokio::process::Command::new("claude");
        cmd.arg("-p")
            .arg(&prompt)
            .arg("--output-format")
            .arg("json")
            .arg("--disallowed-tools")
            .args(DISALLOWED);
        if let Some(sys) = &system {
            cmd.arg("--system-prompt").arg(sys);
        }
        if let Some(m) = &self.model {
            cmd.arg("--model").arg(m);
        }
        cmd.stdin(std::process::Stdio::null());

        let run = cmd.output();
        let out = tokio::select! {
            r = run => r.map_err(|e| LLMError::ProviderError(format!("spawn claude failed: {e} (is the Claude Code CLI installed and on PATH?)")))?,
            _ = self.cancel.cancelled() => return Err(LLMError::Generic("cancelled".into())),
        };
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(LLMError::ProviderError(format!(
                "claude exited {}: {}",
                out.status,
                err.trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        Ok(Box::new(ClaudeCodeResponse(extract_reply(&stdout))))
    }
}

#[async_trait]
impl CompletionProvider for ClaudeCodeProvider {
    async fn complete(
        &self,
        _req: &CompletionRequest,
        _json_schema: Option<StructuredOutputFormat>,
    ) -> Result<CompletionResponse, LLMError> {
        Err(LLMError::ProviderError("completion not supported".into()))
    }
}

#[async_trait]
impl EmbeddingProvider for ClaudeCodeProvider {
    async fn embed(&self, _input: Vec<String>) -> Result<Vec<Vec<f32>>, LLMError> {
        Err(LLMError::ProviderError(
            "Claude Code CLI has no embedding endpoint".into(),
        ))
    }
}

impl ModelsProvider for ClaudeCodeProvider {}
impl LLMProvider for ClaudeCodeProvider {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_from_array_and_object() {
        let arr = r#"[{"type":"system"},{"type":"result","result":"hello there"}]"#;
        assert_eq!(extract_reply(arr), "hello there");
        let obj = r#"{"type":"result","result":"hi"}"#;
        assert_eq!(extract_reply(obj), "hi");
        assert_eq!(extract_reply("plain text"), "plain text");
    }

    #[test]
    fn render_splits_system_and_convo() {
        let msgs = vec![
            ChatMessage {
                role: ChatRole::System,
                message_type: autoagents::llm::chat::MessageType::Text,
                content: "be brief".into(),
            },
            ChatMessage {
                role: ChatRole::User,
                message_type: autoagents::llm::chat::MessageType::Text,
                content: "hello".into(),
            },
        ];
        let (sys, convo) = render(&msgs);
        assert_eq!(sys.as_deref(), Some("be brief"));
        assert_eq!(convo, "User: hello");
    }

    // Live: requires the claude CLI + a logged-in subscription.
    //   cargo test -p oxidemx-agent claude_live -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn claude_live_responds() {
        let p = ClaudeCodeProvider::new(None);
        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            message_type: autoagents::llm::chat::MessageType::Text,
            content: "Reply with exactly: claude provider ok".into(),
        }];
        let resp = p.chat_with_tools(&msgs, None, None).await.unwrap();
        let t = resp.text().unwrap_or_default();
        println!("claude reply: {t}");
        assert!(!t.is_empty());
    }
}
