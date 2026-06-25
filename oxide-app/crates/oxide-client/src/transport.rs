//! The transport seam. `oxide-ui`/`oxide-freya` depend on `Arc<dyn Transport>`,
//! never a concrete client — so the UI is testable against `MockTransport`.
use async_trait::async_trait;
use futures_util::stream::BoxStream;

use crate::dto::{AgentEvent, AttachmentPayload, Conversation, MessageId, Project, Turn};
use crate::error::TransportError;

#[async_trait]
pub trait Transport: Send + Sync {
    async fn health(&self) -> Result<(), TransportError>;
    async fn list_projects(&self) -> Result<Vec<Project>, TransportError>;
    async fn list_conversations(&self, project_id: &str) -> Result<Vec<Conversation>, TransportError>;
    async fn create_conversation(&self, project_id: &str, working_dir: Option<&str>) -> Result<Conversation, TransportError>;
    async fn get_history(&self, conversation_id: &str) -> Result<Vec<Turn>, TransportError>;
    async fn send_message(&self, conversation_id: &str, text: &str, attachments: &[AttachmentPayload]) -> Result<MessageId, TransportError>;
    /// SSE subscription. Yields normalized events; reconnects with Last-Event-ID on drop.
    fn subscribe(&self, conversation_id: &str) -> BoxStream<'static, Result<AgentEvent, TransportError>>;
}
