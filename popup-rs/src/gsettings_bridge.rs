//! GSettings bridge for the GNOME indicator extension's battery-colour prefs.
//!
//! Reads `org.gnome.shell.extensions.oxidemx-indicator` so the popup's
//! battery-ring colours exactly match the top-bar indicator. Uses a 1-second
//! poll calling the `gsettings` CLI utility via a subprocess to avoid GObject/GIO
//! threading context issues and deadlocks.
//!
//! SPDX-License-Identifier: GPL-3.0

use tracing::warn;

/// Resolved battery-colour prefs from GSettings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatteryColors {
    pub threshold_critical: u8,
    pub threshold_low: u8,
    pub color_critical: String,
    pub color_low: String,
    pub color_healthy: String,
    pub color_charging: String,
}

impl Default for BatteryColors {
    fn default() -> Self {
        BatteryColors {
            threshold_critical: 15,
            threshold_low: 30,
            color_critical: "#f38ba8".to_string(),
            color_low: "#f9e2af".to_string(),
            color_healthy: "#89b4fa".to_string(),
            color_charging: "#a6e3a1".to_string(),
        }
    }
}

/// Helper to run `gsettings get` CLI command to retrieve a key.
fn get_gsettings_value(key: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let schema_dir = format!("{}/.local/share/gnome-shell/extensions/oxidemx-indicator@dev.oxidemx.com/schemas", home);
    let output = std::process::Command::new("gsettings")
        .env("GSETTINGS_SCHEMA_DIR", &schema_dir)
        .args(["get", "org.gnome.shell.extensions.oxidemx-indicator", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let val = String::from_utf8(output.stdout).ok()?;
    let val = val.trim();
    // gsettings CLI returns strings wrapped in single quotes, e.g. '#f38ba8' or 'color'
    let val = val.strip_prefix('\'').unwrap_or(val);
    let val = val.strip_suffix('\'').unwrap_or(val);
    Some(val.to_string())
}

/// Read the current `BatteryColors` from GSettings using the CLI tool.
fn read_from_gsettings() -> Option<BatteryColors> {
    let threshold_critical = get_gsettings_value("threshold-critical")?
        .parse::<u8>()
        .ok()?;
    let threshold_low = get_gsettings_value("threshold-low")?
        .parse::<u8>()
        .ok()?;
    let color_critical = get_gsettings_value("color-critical")?;
    let color_low = get_gsettings_value("color-low")?;
    let color_healthy = get_gsettings_value("color-healthy")?;
    let color_charging = get_gsettings_value("color-charging")?;

    Some(BatteryColors {
        threshold_critical,
        threshold_low,
        color_critical,
        color_low,
        color_healthy,
        color_charging,
    })
}

/// Poll GSettings once per second and yield `BatteryColors` whenever
/// the value changes. Used as an iced `Subscription::run` source.
pub fn poll_stream()
-> impl futures_util::stream::Stream<Item = BatteryColors>
{
    use futures_util::stream;

    let initial_state = BatteryColors {
        threshold_critical: 255,
        ..BatteryColors::default()
    };

    stream::unfold(Some(initial_state), |state_opt| async move {
        let last = state_opt?;

        loop {
            let current = tokio::task::spawn_blocking(read_from_gsettings)
                .await
                .unwrap_or(None);

            let current = match current {
                Some(c) => c,
                None => {
                    warn!(
                        "GSettings schema \
                         'org.gnome.shell.extensions.oxidemx-indicator' \
                         not found or query failed — using default battery colours."
                    );
                    return Some((BatteryColors::default(), None));
                }
            };

            if current != last {
                let next_state = current.clone();
                return Some((current, Some(next_state)));
            }

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    })
}
