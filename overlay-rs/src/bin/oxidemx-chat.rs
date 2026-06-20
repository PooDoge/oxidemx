//! Standalone OxideMX chat window — a normal decorated toplevel that reuses the
//! overlay's chat in place (see oxidemx_overlay::run_chat_window). Single-instance
//! is added in a later task.
fn main() -> iced::Result {
    // Mirror the overlay binary's tracing init so RUST_LOG works (wgpu adapter
    // selection, agentd stream, etc. are otherwise invisible from this binary).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();
    oxidemx_overlay::run_chat_window()
}
