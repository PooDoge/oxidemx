//! Resolution + dispatch for slice actions. Runs the slice's command
//! on activation. Spawned as detached children so the overlay is
//! never blocked waiting for the launched program to exit.

use oxidemx_shared::{ActionKind, Slice};
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
            //      (target/release/oxidemx-settings)
            //   3. PATH lookup for `oxidemx-settings`
            let user_override = slice.command.trim();
            let resolved = if !user_override.is_empty() {
                user_override.to_string()
            } else if let Some(sibling) = sibling_binary("oxidemx-settings") {
                sibling.display().to_string()
            } else {
                "oxidemx-settings".to_string()
            };
            spawn_shell(&resolved, &slice.label);
        }
        ActionKind::Submenu => {
            // Submenu opens are handled in the radial widget on
            // hover, not on release. Nothing to dispatch.
        }
        ActionKind::Macro => {
            // The slice's `command` field holds the macro id (set
            // by the settings UI when the user picks one from the
            // Macros tab). Empty id is a misconfigured slice — log
            // and bail rather than firing a no-op D-Bus call.
            let id = slice.command.trim();
            if id.is_empty() {
                warn!("Macro slice '{}' has no macro id — skipping", slice.label);
                return;
            }
            info!(label = %slice.label, macro_id = id, "dispatching macro");
            crate::haptic_client::execute_macro_blocking(id.to_string());
        }
        ActionKind::Shortcut => {
            // `slice.command` holds the key chord in xdotool format
            // ("ctrl+shift+z", "super+e"). Daemon's
            // `ActionExecutor::execute_shortcut` does the rest.
            let keys = slice.command.trim();
            if keys.is_empty() {
                warn!("Shortcut slice '{}' has no key chord — skipping", slice.label);
                return;
            }
            info!(label = %slice.label, keys, "dispatching shortcut");
            crate::haptic_client::execute_shortcut_blocking(keys.to_string());
        }
        ActionKind::EasySwitch => {
            // `slice.command` holds the 1-based host index ("1",
            // "2", "3") — what's printed under each button on the
            // mouse. Parse + clamp before sending so a typo doesn't
            // panic the daemon's HID++ writer.
            let raw = slice.command.trim();
            match raw.parse::<u8>() {
                Ok(idx) if (1..=3).contains(&idx) => {
                    info!(label = %slice.label, host = idx, "dispatching host switch");
                    crate::haptic_client::set_host_blocking(idx);
                }
                _ => {
                    warn!(
                        "EasySwitch slice '{}' command must be 1, 2, or 3 (got {raw:?})",
                        slice.label
                    );
                }
            }
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
