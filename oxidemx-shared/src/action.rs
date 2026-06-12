use serde::{Deserialize, Serialize};

/// What a radial slice does when activated.
///
/// Encoded as the `type` field on a slice in `config.json`. Values
/// match the strings used by the existing Python overlay so configs
/// round-trip between the two implementations during the migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Run a command via the shell. The slice's `command` field is the cmd.
    #[default]
    Exec,
    /// Open a submenu. Submenu items are stored on the slice's `submenu`
    /// field (added in the upcoming editor work — currently the daemon
    /// hard-codes the AI submenu).
    Submenu,
    /// Trigger a configured macro by id (see daemon/src/macros).
    Macro,
    /// Switch the MX Master's Easy-Switch host (1, 2, or 3).
    EasySwitch,
    /// Open the OxideMX settings window.
    Settings,
    /// Open the OS emoji picker (e.g. `ibus emoji`, `gnome-characters`).
    /// Treated like Exec on the overlay side; kept as a distinct kind
    /// so the editor can render an emoji-specific icon by default.
    Emoji,
    /// Send a keyboard shortcut via the daemon's evdev/ydotool path.
    /// `command` carries the chord string (e.g. "ctrl+c").
    Shortcut,
    /// Live-data widget wedge (Splice Widgets page). Display-only —
    /// activation is a no-op unless the slice also carries a submenu.
    /// The data binding lives in the slice's `widget` field
    /// (`WidgetConfig`); a `Widget` slice without one renders the
    /// stub placeholder.
    Widget,
    /// Adjustable dial wedge (brightness / volume). Scroll or drag
    /// over the slice adjusts the value; the wedge shows the current
    /// percentage. The target lives in the slice's `dial` field
    /// (`DialKind`).
    Dial,
    /// Session power action. `command` carries which one:
    /// "lock" | "logoff" | "suspend" | "restart" | "shutdown".
    /// Executed via logind / gnome-session, not the daemon.
    Power,
    /// Toggle GNOME night light (gsettings
    /// `org.gnome.settings-daemon.plugins.color night-light-enabled`).
    /// The wedge shows a state dot bound to the same key.
    NightLight,
    /// Mouse quick setting via the daemon's D-Bus surface. `command`
    /// carries the setting: "dpi:<value>" | "smartshift" | "haptics"
    /// | "gaming".
    MouseSetting,
    /// No-op slice. Useful for placeholders while editing.
    None,
}
