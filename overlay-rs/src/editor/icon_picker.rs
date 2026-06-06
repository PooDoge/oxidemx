//! Icon picker dialog. Three tabs:
//!   * **Theme** — searchable grid of every freedesktop symbolic icon
//!     available on the system, enumerated via `gtk::IconTheme`.
//!   * **Files** — drop a custom SVG/PNG via GtkFileChooser; copies it
//!     into `~/.local/share/oxidemx/icons/` for portability.
//!   * **Bundled** — the legacy hand-drawn ids ("play_pause", "folder",
//!     "easy_switch", …) shown with a thumbnail.
//!
//! Returns the chosen icon as the string that gets written to the
//! slice's `icon` field — either a freedesktop name, an absolute path,
//! or one of the legacy internal ids.

// TODO: implement. Resolution lives in `render::icons` so the picker's
// preview can render the same way the menu does.
