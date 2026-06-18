//! oxidemx-agent-local — embedded local-LLM inference (mistral.rs 0.8.1) with
//! lifecycle, capability scoping, and a sanity-check failsafe. Hosted by agentd.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod guard;
pub mod mode;
pub mod types;

pub use error::LocalError;
pub use guard::{Action, Check, GuardConfig, Reason, SchemaKind, Verdict};
pub use mode::Mode;
pub use types::{ChatRequest, ChatResponse, Message, ModelState, ModelStatusInfo, Role, Usage};
