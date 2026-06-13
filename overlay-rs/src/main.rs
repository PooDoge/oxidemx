//! OxideMX overlay (Rust + iced + xdg-shell).
//!
//! Replacement for the legacy Python overlay/. Same daemon, same
//! D-Bus contract (`org.oxidemx.Daemon`). Mutter doesn't advertise
//! `wlr-layer-shell` on stable GNOME, so positioning is delegated to
//! the `oxidemx-cursor` GNOME Shell extension's `MoveOverlay`
//! D-Bus method — the overlay is a regular xdg-shell window that
//! the extension places exactly where we want it after each show.
//!
//! Flow:
//!   1. iced application opens a transparent, decorationless,
//!      always-on-top xdg-shell window.
//!   2. zbus listener subscribes to `MenuRequested(x, y)` /
//!      `HideMenu()` / `CursorMoved(dx, dy)` from the daemon.
//!   3. On a request: present the window, fire `MoveOverlay` to the
//!      cursor position, paint the radial.
//!   4. On hide: hide the window.
//!
//! All rendering happens via iced's `canvas::Frame` — no cairo, no
//! GTK, no `*-devel` rpm-ostree layering. Pure Rust dep tree.

mod actions;
mod agent;
mod agent_runtime;
mod ai_client;
mod anim;
mod app;
mod chat_shell;
mod chat_ui;
mod config;
mod dbus;
mod fonts;
mod editor {
    pub mod icon_picker;
    pub mod preview;
    pub mod slice_panel;
    pub mod window;
}
mod geometry;
mod handoff;
mod haptic_client;
mod input;
mod radial;
mod sampler;
mod render {
    pub mod animation;
    pub mod aurora;
    pub mod center_dome;
    pub mod disc_bevel;
    pub mod dispatch_burst;
    pub mod drop_shadow;
    pub mod hover_glow;
    pub mod hover_tilt;
    pub mod icons;
    pub mod page_fx;
    pub mod ripple;
    pub mod sdf_ring;
    pub mod slice_bevel;
    pub mod slices;
    pub mod specular_sweep;
    pub mod status_fx;
}
mod theme;
mod tray;
mod widget_host;

fn main() -> iced::Result {
    // Headless heartbeat tick (run by a systemd user timer; see
    // agent/heartbeat.rs). Handled before the iced app boots: no
    // window, no D-Bus listener, no bus-name claim — so the
    // single-instance guard and a live overlay are both untouched.
    if std::env::args().any(|a| a == "--heartbeat") {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let code = rt.block_on(async {
            match ai_client::run_heartbeat().await {
                Ok(None) => 0,
                Ok(Some(alert)) => {
                    agent::heartbeat::deliver_alert(&alert);
                    0
                }
                Err(e) => {
                    eprintln!("heartbeat failed: {e}");
                    1
                }
            }
        });
        std::process::exit(code);
    }

    // Headless agent self-test: drives the real `agent_runtime` path
    // (OverlayAgent + tools + provider + session) against the live API
    // with no window or bus claim. Used to verify the AutoAgents
    // replacement end-to-end without the radial UI. Usage:
    //   oxidemx-overlay --agent-selftest "your prompt here"
    if let Some(pos) = std::env::args().position(|a| a == "--agent-selftest") {
        let prompt = std::env::args()
            .nth(pos + 1)
            .unwrap_or_else(|| "Say hello in one short sentence.".to_string());
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let code = rt.block_on(async {
            // Provider + key resolve inside the runtime from config.
            match ai_client::ask_ai(
                ai_client::AgentMode::Agentic,
                ai_client::DEFAULT_MODEL,
                &prompt,
                None,
                &[],
            )
            .await
            {
                Ok((reply, _)) => {
                    println!("--- reply ---\n{reply}");
                    0
                }
                Err(e) => {
                    eprintln!("selftest failed: {e}");
                    1
                }
            }
        });
        std::process::exit(code);
    }

    // Headless memory-recall probe: prints the hybrid lexical+semantic
    // injection block for a query, for verifying semantic recall.
    //   oxidemx-overlay --memory-recall "what theme should I use?"
    if let Some(pos) = std::env::args().position(|a| a == "--memory-recall") {
        let query = std::env::args().nth(pos + 1).unwrap_or_default();
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            match agent::memory::injection_block_for_async(&query).await {
                Some(block) => println!("{block}"),
                None => println!("(no memories)"),
            }
        });
        std::process::exit(0);
    }

    // Default filter: info-level for our crates, error-only for usvg
    // (which spams "Failed to parse marker-start value: 'none'." for
    // every freedesktop icon — the parser warns on perfectly valid
    // CSS the icons use, and the rendering is unaffected).
    let default_filter = "info,usvg=error";
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_filter)),
        )
        .init();

    // Seed bundled built-in widgets into the user widgets dir BEFORE
    // the widget host's first registry scan (spec §16; the scan runs
    // when the iced subscription spawns the worker). Blocking is fine
    // here: the common case is a handful of version compares against
    // already-installed copies. See oxidemx_widget_cli::seed for the
    // trust model (install-media bundles seed without consent).
    match oxidemx_widget_cli::seed_builtin_widgets() {
        Ok(outcomes) => {
            for o in &outcomes {
                tracing::info!("builtin widget seed: {o}");
            }
        }
        Err(e) => tracing::warn!("builtin widget seeding failed: {e}"),
    }

    // Debug-only smoke hook (`scripts/widget-smoke.sh`): boot the
    // widget-host worker without iced, print the first scene
    // revision, exit. Compiled out of release builds.
    #[cfg(debug_assertions)]
    if std::env::args().any(|a| a == "--widget-smoke") {
        std::process::exit(widget_host::run_smoke());
    }

    app::run()
}
