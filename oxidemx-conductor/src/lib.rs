//! OxideMX Conductor — flow orchestration on AutoAgents (P3 core).
//!
//! Spec: `docs/plans/agent-framework-integration-brainstorm.md`
//! Part II §7–12. AutoAgents ships only primitives (typed topics,
//! hooks, an event stream) and no orchestration construct (§14.1,
//! verified). This crate is that layer: it parses a **flow document**
//! (§8), validates it into a DAG (`FlowPlan`), and drives it with a
//! **supervisor** that schedules ready steps concurrently, joins
//! fan-ins, retries, times out, and cancels via a per-run
//! `CancellationToken` — running one AutoAgents ReAct agent per step.
//!
//! The integration stays at two seams (the `building-llm-agents-in-
//! rust` skill's thesis): the provider (reused from `oxidemx-agent`'s
//! multi-provider factory) and the tools (the bridged `execute_command`
//! plus the per-step `ApprovalGate`). Everything else — the loop,
//! memory, the ReAct executor — is the framework's.

pub mod approval;
pub mod event;
pub mod flowdoc;
pub mod loader;
pub mod mock;
pub mod plan;
pub mod roster;
pub mod step_agent;
pub mod supervisor;
pub mod template;

pub use event::{EventSink, RunEvent};
pub use flowdoc::{FlowDoc, FlowDocError};
pub use loader::{load_flow, load_roster, LoadError};
pub use plan::{validate, FlowPlan, ValidationError};
pub use roster::{AgentDef, Roster};
pub use supervisor::{run_flow, RunHandle, RunOptions, RunOutcome};

/// Tools the conductor knows how to bridge. Roster `tools` grants are
/// validated against this set — the full `oxidemx-agent` registry:
/// our `execute_command` plus the AutoAgents Toolkit (filesystem,
/// document parsing, web search).
pub use oxidemx_agent::toolkit::BUILTIN_TOOLS as KNOWN_TOOLS;
