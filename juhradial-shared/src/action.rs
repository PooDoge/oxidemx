use serde::{Deserialize, Serialize};

/// What a radial slice does when activated.
///
/// Encoded as the `type` field on a slice in `config.json`. Values
/// match the strings used by the existing Python overlay so configs
/// round-trip between the two implementations during the migration.
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
    /// Open the JuhRadial MX settings window.
    Settings,
    /// Open the OS emoji picker (e.g. `ibus emoji`, `gnome-characters`).
    /// Treated like Exec on the overlay side; kept as a distinct kind
    /// so the editor can render an emoji-specific icon by default.
    Emoji,
    /// Send a keyboard shortcut via the daemon's evdev/ydotool path.
    /// `command` carries the chord string (e.g. "ctrl+c").
    Shortcut,
    /// No-op slice. Useful for placeholders while editing.
    None,
}

impl Default for ActionKind {
    fn default() -> Self {
        ActionKind::Exec
    }
}
