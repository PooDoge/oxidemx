//! Heartbeat — the OpenClaw-style proactive tick (implementation
//! plan §3). A systemd user timer runs `oxidemx-overlay --heartbeat`
//! periodically; that headless run gives the agent one tool-less
//! turn over the user's `heartbeat.md` checklist plus persona +
//! memories. The response contract keeps it silent by default:
//!
//!   * reply is exactly `HEARTBEAT_OK` → nothing needs attention,
//!     exit without a sound;
//!   * anything else → desktop notification (via `notify-send`) +
//!     appended to the heartbeat log so the chat can reference it.
//!
//! v1 scope notes (deliberate):
//!   * no tools on heartbeat turns — read-only-by-construction;
//!     revisit once the approval ladder has a global read-only
//!     tier;
//!   * the alert lands in `~/.local/share/oxidemx/heartbeat-log.md`
//!     plus a notification, NOT yet as a chat-thread message — the
//!     thread store has no cross-process write coordination, and a
//!     headless writer would race the live overlay's saves;
//!   * the headless run never claims `org.oxidemx.overlay` (it
//!     never starts the D-Bus listener at all), so the
//!     duplicate-instance guard can't kill it and it can't kill
//!     the overlay.

use std::path::PathBuf;

/// Same per-file budget as the persona files.
const MAX_CHARS: usize = 20_000;

pub fn checklist_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    PathBuf::from(home).join(".config/oxidemx/heartbeat.md")
}

fn log_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    PathBuf::from(home).join(".local/share/oxidemx/heartbeat-log.md")
}

/// The user's checklist, capped. `None` = no file → the tick is a
/// no-op (heartbeat is opt-in by creating the file).
pub fn checklist() -> Option<String> {
    let raw = std::fs::read_to_string(checklist_path()).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut s = trimmed.to_string();
    if s.len() > MAX_CHARS {
        let mut cut = MAX_CHARS;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    Some(s)
}

/// Deliver a non-OK heartbeat result: desktop notification +
/// append-only log entry. Failures are logged, never fatal — a
/// broken notification daemon shouldn't crash the tick.
pub fn deliver_alert(text: &str) {
    let body: String = text.chars().take(1000).collect();
    let status = std::process::Command::new("notify-send")
        .arg("--app-name=OxideMX")
        .arg("--icon=input-mouse")
        .arg("OxideMX heartbeat")
        .arg(&body)
        .status();
    if let Err(e) = status {
        tracing::warn!("notify-send failed: {e}");
    }

    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = format!("\n## tick {stamp}\n\n{text}\n");
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut f) => {
            let _ = f.write_all(entry.as_bytes());
        }
        Err(e) => tracing::warn!("heartbeat log append failed: {e}"),
    }
}
