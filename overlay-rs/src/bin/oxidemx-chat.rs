//! Standalone OxideMX chat window — a normal decorated toplevel that reuses the
//! overlay's chat in place (see oxidemx_overlay::run_chat_window). Single-instance
//! is added in a later task.
fn main() -> iced::Result {
    oxidemx_overlay::run_chat_window()
}
