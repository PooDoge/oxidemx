//! Resolution + dispatch for slice actions. Runs the slice's command
//! on activation. Spawned as detached children so the overlay is
//! never blocked waiting for the launched program to exit.

use juhradial_shared::{ActionKind, Slice};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tracing::{info, warn};

/// Run the slice's action. Errors are logged but don't propagate —
/// a misconfigured slice shouldn't crash the overlay.
pub fn dispatch(slice: &Slice) {
    match slice.kind {
        ActionKind::Exec | ActionKind::Emoji => {
            spawn_shell(&slice.command, &slice.label);
        }
        ActionKind::Settings => {
            // Resolve the settings binary in this order so it works
            // for both `cargo build`-style dev runs (binary in
            // target/release/, not on PATH) and packaged installs:
            //   1. user override in slice.command
            //   2. sibling next to the running overlay binary
            //      (target/release/juhradial-settings)
            //   3. PATH lookup for `juhradial-settings`
            let user_override = slice.command.trim();
            let resolved = if !user_override.is_empty() {
                user_override.to_string()
            } else if let Some(sibling) = sibling_binary("juhradial-settings") {
                sibling.display().to_string()
            } else {
                "juhradial-settings".to_string()
            };
            spawn_shell(&resolved, &slice.label);
        }
        ActionKind::Submenu => {
            // Submenu opens are handled in the radial widget on
            // hover, not on release. Nothing to dispatch.
        }
        ActionKind::Macro => {
            // TODO: D-Bus call into the daemon to trigger macro by id.
            warn!("Macro dispatch not yet implemented (slice: {})", slice.label);
        }
        ActionKind::Shortcut => {
            // TODO: D-Bus call into the daemon to send a key chord
            // via evdev/ydotool.
            warn!("Shortcut dispatch not yet implemented (slice: {})", slice.label);
        }
        ActionKind::EasySwitch => {
            // TODO: D-Bus call into the daemon to switch Bolt host.
            warn!("EasySwitch dispatch not yet implemented (slice: {})", slice.label);
        }
        ActionKind::None => {}
    }
}

/// Resolve a sibling binary in the same directory as the running
/// overlay executable. Returns `Some(path)` only if the file
/// actually exists + is readable; lets the dev-mode "binary in
/// target/release/, not on PATH" workflow Just Work without the
/// user having to install anything.
fn sibling_binary(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join(name);
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

/// Spawn `command` via `sh -c` so the user can use shell features
/// (pipes, env expansion, `&&`, etc.) and we don't have to parse
/// argv. Stdio is detached so the child outlives the overlay's
/// lifetime if needed (the user might close the menu before the
/// launched app finishes starting up).
fn spawn_shell(command: &str, label: &str) {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        warn!("dispatch: slice '{label}' has empty command — skipping");
        return;
    }
    info!(label, command = trimmed, "dispatching slice action");
    match Command::new("sh")
        .arg("-c")
        .arg(trimmed)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            // Drop the Child handle on purpose — we want the
            // process to fully detach. If we held it, dropping the
            // Child would leave a zombie until reaped.
            let pid = child.id();
            std::mem::drop(child);
            info!(pid, label, "spawned");
        }
        Err(e) => {
            warn!("dispatch: spawn failed for '{label}' ({command}): {e}");
        }
    }
}
