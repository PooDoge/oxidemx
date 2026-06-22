//! oxide-client — agentd transport (UDS desktop now; tailnet-TCP in 2c).
pub mod dto;
pub mod error;
pub mod mock;
pub mod sse;
pub mod transport;
pub mod uds;

pub use dto::{AgentEvent, Conversation, ConversationId, MessageId, Project, ProjectId, Turn};
pub use error::TransportError;
pub use transport::Transport;
pub use uds::UdsTransport;
