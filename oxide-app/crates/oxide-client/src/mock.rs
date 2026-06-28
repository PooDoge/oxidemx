//! In-memory `Transport` for UI tests. Scripted projects/conversations/history
//! and a scripted event stream per conversation.
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures_util::stream::{self, BoxStream, StreamExt};

use crate::dto::*;
use crate::error::TransportError;
use crate::transport::Transport;

pub struct MockTransport {
    pub projects: Vec<Project>,
    pub conversations: Vec<Conversation>,
    pub history: Vec<Turn>,
    /// Events the next `subscribe` will yield, in order.
    pub events: Mutex<Vec<AgentEvent>>,
    pub healthy: bool,
    /// When `true`, `create_conversation` returns `Err(Unreachable)`.
    pub create_conversation_fails: bool,
    /// All attachments from every `send_message` call, in order of arrival.
    pub recorded: Arc<Mutex<Vec<AttachmentPayload>>>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self {
            projects: Vec::new(),
            conversations: Vec::new(),
            history: Vec::new(),
            events: Mutex::new(Vec::new()),
            healthy: true,
            create_conversation_fails: false,
            recorded: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns a clone of all attachments received across every `send_message` call.
    pub fn recorded_attachments(&self) -> Vec<AttachmentPayload> {
        self.recorded.lock().unwrap().clone()
    }
}

impl Default for MockTransport {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Transport for MockTransport {
    async fn health(&self) -> Result<(), TransportError> {
        if self.healthy { Ok(()) } else { Err(TransportError::Unreachable("mock".into())) }
    }
    async fn list_projects(&self) -> Result<Vec<Project>, TransportError> { Ok(self.projects.clone()) }
    async fn list_conversations(&self, _p: &str) -> Result<Vec<Conversation>, TransportError> { Ok(self.conversations.clone()) }
    async fn create_conversation(&self, project_id: &str, _wd: Option<&str>) -> Result<Conversation, TransportError> {
        if self.create_conversation_fails {
            return Err(TransportError::Unreachable("mock create".into()));
        }
        Ok(Conversation { id: ConversationId::from("mock-conv"), project_id: ProjectId::from(project_id),
            title: "New".into(), working_dir: String::new(), model: String::new(), created_at: 0, updated_at: 0,
            worktree: None })
    }
    async fn get_history(&self, _c: &str) -> Result<Vec<Turn>, TransportError> { Ok(self.history.clone()) }
    async fn send_message(&self, _c: &str, _t: &str, attachments: &[AttachmentPayload]) -> Result<MessageId, TransportError> {
        self.recorded.lock().unwrap().extend_from_slice(attachments);
        Ok(MessageId::from("mock-msg"))
    }
    async fn delete_conversation(&self, _conversation_id: &str) -> Result<(), TransportError> { Ok(()) }
    fn subscribe(&self, _c: &str) -> BoxStream<'static, Result<AgentEvent, TransportError>> {
        let evs = self.events.lock().unwrap().clone();
        stream::iter(evs.into_iter().map(Ok)).boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn default_and_new_are_both_healthy() {
        assert!(MockTransport::default().health().await.is_ok());
        assert!(MockTransport::new().health().await.is_ok());
    }

    #[tokio::test]
    async fn mock_subscribe_yields_scripted_events() {
        let m = MockTransport {
            events: Mutex::new(vec![AgentEvent { seq: 1, kind: "delta".into(),
                payload: serde_json::json!({"kind":"delta","text":"hi"}) }]),
            ..MockTransport::new()
        };
        let got: Vec<_> = m.subscribe("c").collect().await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].as_ref().unwrap().text(), Some("hi"));
    }

    #[tokio::test]
    async fn send_message_records_attachments() {
        let m = MockTransport::new();
        let att = AttachmentPayload {
            name:     "a.png".into(),
            mime:     "image/png".into(),
            kind:     "image".into(),
            data_b64: Some("AAAA".into()),
        };
        let result = m.send_message("c1", "hi", &[att.clone()]).await;
        assert!(result.is_ok());
        let recorded = m.recorded_attachments();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], att);
    }

    #[tokio::test]
    async fn send_message_no_attachments_records_empty() {
        let m = MockTransport::new();
        m.send_message("c1", "hello", &[]).await.unwrap();
        assert!(m.recorded_attachments().is_empty());
    }

    #[tokio::test]
    async fn send_message_accumulates_across_calls() {
        let m = MockTransport::new();
        let a1 = AttachmentPayload { name: "a.png".into(), mime: "image/png".into(), kind: "image".into(), data_b64: Some("AAAA".into()) };
        let a2 = AttachmentPayload { name: "b.txt".into(), mime: "text/plain".into(), kind: "text".into(), data_b64: None };
        m.send_message("c1", "first", &[a1.clone()]).await.unwrap();
        m.send_message("c1", "second", &[a2.clone()]).await.unwrap();
        let recorded = m.recorded_attachments();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0], a1);
        assert_eq!(recorded[1], a2);
    }
}
