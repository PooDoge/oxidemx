use serde::{Deserialize, Serialize};

/// What a radial slice does when activated.
///
/// Encoded as the `type` field on a slice in `config.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Run a command via the shell. The slice's `command` field is the cmd.
    Exec,
    /// Open a submenu. Submenu items are stored on the slice's `submenu`
    /// field (added in the upcoming editor work — currently the daemon
    /// hard-codes the AI submenu).
    Submenu,
    /// Trigger a configured macro by id (see daemon/src/macros).
    Macro,
    /// Switch the MX Master's Easy-Switch host (1, 2, or 3).
    EasySwitch,
    /// No-op slice. Useful for placeholders while editing.
    None,
}

impl Default for ActionKind {
    fn default() -> Self {
        ActionKind::Exec
    }
}
