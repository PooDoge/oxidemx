//! Resolution + dispatch for slice actions. Runs the slice's command
//! on activation. Spawned as detached children so the overlay is
//! never blocked waiting for the launched program to exit.

use juhradial_shared::{ActionKind, Slice};
use std::process::{Command, Stdio};
use tracing::{info, warn};

/// Run the slice's action. Errors are logged but don't propagate —
/// a misconfigured slice shouldn't crash the overlay.
pub fn dispatch(slice: &Slice) {
    match slice.kind {
        ActionKind::Exec | ActionKind::Settings | ActionKind::Emoji => {
            spawn_shell(&slice.command, &slice.label);
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
