//! oxidemx-agent-core — the UI-free agent brain (providers, turn loop, tools,
//! memory, persona), hostable in-process today and by `agentd` (SP1b) later.

pub mod api_key;
pub mod events;
pub mod memory;
pub mod memory_semantic;
pub mod persona;
pub mod tool;
