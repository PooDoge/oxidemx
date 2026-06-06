//! Diagnostics for the gamepad-haptic bridge.
//!
//! Backs the settings tab's "Diagnose" button (design §9.2 / §11.4)
//! and the D-Bus `DiagnoseHapticRedirect` method. The job is to
//! shift the "why isn't my rumble reaching the mouse?" debugging
//! burden off the user: report what's on the input bus and give a
//! plain-language recommendation.

use std::fmt::Write as _;

use evdev::Device;
use oxidemx_shared::{HapticRedirectConfig, HapticRedirectMode};

use super::{proxy, virtual_pad};

/// An input device discovered during a diagnostic scan.
struct Found {
    /// Kernel node name, e.g. `"event24"`.
    node: String,
    name: String,
    vendor: u16,
    product: u16,
}

/// Produce the human-readable diagnostic report shown by the
/// settings "Diagnose" button.
pub fn diagnostic_report(config: &HapticRedirectConfig) -> String {
    let steam = steam_running();

    let mut bridge_node: Option<String> = None;
    let mut steam_pad: Option<Found> = None;
    let mut controllers: Vec<Found> = Vec::new();

    if let Ok(entries) = std::fs::read_dir("/dev/input") {
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("event"))
            })
            .collect();
        paths.sort();

        for path in paths {
            let node = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();
            let device = match Device::open(&path) {
                Ok(device) => device,
                Err(_) => continue, // unreadable — skip
            };
            let name = device.name().unwrap_or("<unknown>").to_string();
            let id = device.input_id();
            let (vendor, product) = (id.vendor(), id.product());

            if name == virtual_pad::NAME {
                bridge_node = Some(node);
            } else if vendor == 0x28DE && product == 0x11FF {
                // Steam's virtual Xbox-360 pad (Valve VID 0x28DE).
                steam_pad = Some(Found { node, name, vendor, product });
            } else if proxy::is_gamepad(&device) {
                controllers.push(Found { node, name, vendor, product });
            }
        }
    }

    let mut report = String::new();
    let _ = writeln!(report, "Game Rumble -> Haptic — diagnostics");
    let _ = writeln!(report);
    let _ = writeln!(report, "Steam running:            {}", yes_no(steam));
    let _ = writeln!(
        report,
        "Steam Input virtual pad:  {}",
        match &steam_pad {
            Some(f) => format!("detected (on {})", f.node),
            None => "not detected".to_string(),
        }
    );
    let _ = writeln!(
        report,
        "Bridge virtual pad:       {}",
        match &bridge_node {
            Some(node) => format!("active (on {node})"),
            None => "not active".to_string(),
        }
    );
    let _ = writeln!(report, "Real controllers:         {}", controllers.len());
    for c in &controllers {
        let _ = writeln!(
            report,
            "  - {}  ({:04x}:{:04x}) on {}",
            c.name, c.vendor, c.product, c.node
        );
    }

    // "Did the game actually open us?" — the single most useful
    // signal when in-game rumble produces nothing. If this list is
    // empty while a game is foregrounded, the game never enumerated
    // our pad (Steam Input stole it, SDL_HIDAPI bypassed us, the
    // game gates rumble on last-input being a gamepad, …).
    if let Some(node) = &bridge_node {
        let openers = processes_holding(node);
        let _ = writeln!(
            report,
            "Bridge pad open by:       {}",
            if openers.is_empty() {
                "no other process".to_string()
            } else {
                format!("{} process(es)", openers.len())
            }
        );
        for (pid, comm) in &openers {
            let _ = writeln!(report, "  - {comm} (pid {pid})");
        }
    }
    let _ = writeln!(report);
    let _ = writeln!(report, "Recommendation:");
    let _ = write!(
        report,
        "  {}",
        recommendation(config, steam, &bridge_node, &steam_pad, &controllers)
    );

    report
}

/// Plain-language guidance derived from the scan.
fn recommendation(
    config: &HapticRedirectConfig,
    steam: bool,
    bridge_node: &Option<String>,
    steam_pad: &Option<Found>,
    controllers: &[Found],
) -> String {
    if bridge_node.is_none() {
        return "The bridge pad is not active. Turn on Game Mode with \
                'Game Rumble -> Haptic' enabled to create it."
            .to_string();
    }

    let mut notes: Vec<String> = Vec::new();

    if steam && steam_pad.is_some() {
        notes.push(
            "Steam Input is active. If a game's rumble does not reach the \
             mouse, open Steam -> Settings -> Controller and turn Steam \
             Input OFF for the bridge pad (or for that game)."
                .to_string(),
        );
    }

    match config.mode {
        HapticRedirectMode::Proxy if controllers.is_empty() => notes.push(
            "Proxy mode is selected but no controller is connected — the \
             bridge is running standalone."
                .to_string(),
        ),
        HapticRedirectMode::Proxy => notes.push(format!(
            "Proxy mode will wrap '{}'.",
            controllers[0].name
        )),
        HapticRedirectMode::Standalone if !controllers.is_empty() => notes.push(
            "A real controller is connected. SDL may route its rumble over \
             hidraw and bypass the bridge — unplug it, or launch the game \
             with SDL_JOYSTICK_HIDAPI=0."
                .to_string(),
        ),
        HapticRedirectMode::Standalone => {}
    }

    if notes.is_empty() {
        "Looks good — rumble from evdev-path games will route to the mouse."
            .to_string()
    } else {
        notes.join("\n  ")
    }
}

/// Processes (pid, comm) that currently have `/dev/input/<node>`
/// open. Scans `/proc/<pid>/fd/*` symlinks — no `lsof` dependency.
/// Skips entries we can't read (other users' pids) silently; the
/// daemon runs as the user, so games launched by the same user are
/// always visible.
fn processes_holding(node: &str) -> Vec<(u32, String)> {
    let target = format!("/dev/input/{node}");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        let fd_dir = entry.path().join("fd");
        let Ok(fds) = std::fs::read_dir(&fd_dir) else {
            continue;
        };
        let mut holds = false;
        for fd in fds.flatten() {
            if let Ok(link) = std::fs::read_link(fd.path()) {
                if link == std::path::Path::new(&target) {
                    holds = true;
                    break;
                }
            }
        }
        if holds {
            // Skip ourselves — the daemon owns the uinput fd, that's
            // expected and would just be noise.
            if pid == std::process::id() {
                continue;
            }
            let comm = std::fs::read_to_string(entry.path().join("comm"))
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "?".to_string());
            out.push((pid, comm));
        }
    }
    out
}

/// Whether a process named exactly `steam` is running. Scans
/// `/proc/<pid>/comm` — no external `pgrep` dependency.
fn steam_running() -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    for entry in entries.flatten() {
        let is_pid = entry
            .file_name()
            .to_str()
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !is_pid {
            continue;
        }
        if let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) {
            if comm.trim() == "steam" {
                return true;
            }
        }
    }
    false
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_is_nonempty_and_well_formed() {
        // Runs against the live /dev/input + /proc — content varies,
        // but the report must always have the fixed sections.
        let report = diagnostic_report(&HapticRedirectConfig::default());
        // Visible with `--nocapture` for a plausibility eyeball.
        eprintln!("---- diagnostic report ----\n{report}\n---------------------------");
        assert!(report.contains("Steam running:"));
        assert!(report.contains("Bridge virtual pad:"));
        assert!(report.contains("Real controllers:"));
        assert!(report.contains("Recommendation:"));
    }

    #[test]
    fn recommendation_flags_missing_bridge() {
        let cfg = HapticRedirectConfig::default();
        let rec = recommendation(&cfg, false, &None, &None, &[]);
        assert!(rec.contains("bridge pad is not active"));
    }

    #[test]
    fn recommendation_warns_about_hidapi_in_standalone() {
        let cfg = HapticRedirectConfig {
            mode: HapticRedirectMode::Standalone,
            ..Default::default()
        };
        let controller = Found {
            node: "event9".into(),
            name: "Xbox Wireless Controller".into(),
            vendor: 0x045E,
            product: 0x02FD,
        };
        let rec = recommendation(
            &cfg,
            false,
            &Some("event24".into()),
            &None,
            std::slice::from_ref(&controller),
        );
        assert!(rec.contains("SDL_JOYSTICK_HIDAPI=0"));
    }

    #[test]
    fn recommendation_notes_proxy_target() {
        let cfg = HapticRedirectConfig {
            mode: HapticRedirectMode::Proxy,
            ..Default::default()
        };
        let controller = Found {
            node: "event9".into(),
            name: "Xbox Wireless Controller".into(),
            vendor: 0x045E,
            product: 0x02FD,
        };
        let rec = recommendation(
            &cfg,
            false,
            &Some("event24".into()),
            &None,
            std::slice::from_ref(&controller),
        );
        assert!(rec.contains("will wrap 'Xbox Wireless Controller'"));
    }
}
