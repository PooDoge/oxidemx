//! Standalone OxideMX chat window — a normal decorated toplevel that reuses the
//! overlay's chat in place (see oxidemx_overlay::run_chat_window). Single-instance
//! via D-Bus name `org.oxidemx.Chat`: a second launch signals the running window
//! to present/focus and exits without spawning a duplicate.
use oxidemx_overlay::chat_window::single_instance::{
    acquire_or_present, stash_present_receiver, SingleInstance,
};

fn main() -> iced::Result {
    // Mirror the overlay binary's tracing init so RUST_LOG works (wgpu adapter
    // selection, agentd stream, etc. are otherwise invisible from this binary).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    // Single-instance: become primary or signal the existing window + exit.
    // A dedicated runtime handles the async D-Bus work before iced takes over
    // the thread. The leaked zbus connection keeps the service alive on the
    // same runtime for as long as the process runs.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let instance = rt.block_on(acquire_or_present());
    match instance {
        SingleInstance::Secondary => {
            tracing::info!("another oxidemx-chat is running; signalled Present and exiting");
            Ok(())
        }
        SingleInstance::Primary { present } => {
            stash_present_receiver(present);
            // Keep the runtime alive so the leaked zbus connection's async
            // tasks (signal dispatch etc.) keep running while iced is active.
            let _rt = rt;
            oxidemx_overlay::run_chat_window()
        }
    }
}
