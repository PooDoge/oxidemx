//! In-memory `Transport` for UI tests. Scripted projects/conversations/history
//! and a scripted event stream per conversation.
use std::sync::Mutex;

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
}

impl MockTransport {
    pub fn new() -> Self {
        Self {
            projects: Vec::new(),
            conversations: Vec::new(),
            history: Vec::new(),
            events: Mutex::new(Vec::new()),
            healthy: true,
        }
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
        Ok(Conversation { id: ConversationId::from("mock-conv"), project_id: ProjectId::from(project_id),
            title: "New".into(), working_dir: String::new(), model: String::new(), created_at: 0, updated_at: 0,
            worktree: None })
    }
    async fn get_history(&self, _c: &str) -> Result<Vec<Turn>, TransportError> { Ok(self.history.clone()) }
    async fn send_message(&self, _c: &str, _t: &str) -> Result<MessageId, TransportError> { Ok(MessageId::from("mock-msg")) }
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
}
