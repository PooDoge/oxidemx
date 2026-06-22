//! Unix-domain-socket transport — stub; filled in Task 5.
use async_trait::async_trait;
use futures_util::stream::BoxStream;

use crate::dto::{AgentEvent, Conversation, MessageId, Project, Turn};
use crate::error::TransportError;
use crate::transport::Transport;

/// Placeholder — will connect to agentd over UDS in Task 5.
pub struct UdsTransport;

#[async_trait]
impl Transport for UdsTransport {
    async fn health(&self) -> Result<(), TransportError> {
        Err(TransportError::Unreachable("UdsTransport not yet implemented".into()))
    }
    async fn list_projects(&self) -> Result<Vec<Project>, TransportError> {
        Err(TransportError::Unreachable("UdsTransport not yet implemented".into()))
    }
    async fn list_conversations(&self, _project_id: &str) -> Result<Vec<Conversation>, TransportError> {
        Err(TransportError::Unreachable("UdsTransport not yet implemented".into()))
    }
    async fn create_conversation(&self, _project_id: &str, _working_dir: Option<&str>) -> Result<Conversation, TransportError> {
        Err(TransportError::Unreachable("UdsTransport not yet implemented".into()))
    }
    async fn get_history(&self, _conversation_id: &str) -> Result<Vec<Turn>, TransportError> {
        Err(TransportError::Unreachable("UdsTransport not yet implemented".into()))
    }
    async fn send_message(&self, _conversation_id: &str, _text: &str) -> Result<MessageId, TransportError> {
        Err(TransportError::Unreachable("UdsTransport not yet implemented".into()))
    }
    fn subscribe(&self, _conversation_id: &str) -> BoxStream<'static, Result<AgentEvent, TransportError>> {
        use futures_util::stream;
        Box::pin(stream::empty())
    }
}
