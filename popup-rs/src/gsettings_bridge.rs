//! GSettings bridge for the GNOME indicator extension's battery-colour prefs.
//!
//! Reads `org.gnome.shell.extensions.juhradial-indicator` so the popup's
//! battery-ring colours exactly match the top-bar indicator. Uses a 1-second
//! poll rather than glib signal wiring — avoids the complexity of bridging
//! a glib `MainContext` into iced's tokio executor while keeping latency
//! acceptable (colour preferences change rarely).
//!
//! # Why poll, not glib signals?
//!
//! `gio::Settings` is `!Send` (wraps a raw GObject pointer). Holding it
//! across an `.await` would require `spawn_local` which needs a
//! `LocalSet`, complicating the iced executor wiring. Polling each second
//! from a `spawn_blocking` call is simpler and sufficient — colour prefs
//! change at human speed, not event speed.
//!
//! # Trade-off
//!
//! A native glib signal subscription would react immediately. The 1-second
//! poll adds ≤1 s lag when the user edits indicator colours in the
//! extension's prefs panel. This is acceptable because the popup is
//! short-lived.

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

/// Read the current `BatteryColors` from GSettings.
/// Must be called from a thread where the glib type system is available.
/// Returns `None` when the schema is not installed.
fn read_from_gsettings() -> Option<BatteryColors> {
    use gio::prelude::*;

    let source = gio::SettingsSchemaSource::default()?;
    source.lookup(
        "org.gnome.shell.extensions.juhradial-indicator",
        true,
    )?;
    let settings =
        gio::Settings::new("org.gnome.shell.extensions.juhradial-indicator");
    Some(BatteryColors {
        threshold_critical: settings.int("threshold-critical") as u8,
        threshold_low: settings.int("threshold-low") as u8,
        color_critical: settings.string("color-critical").to_string(),
        color_low: settings.string("color-low").to_string(),
        color_healthy: settings.string("color-healthy").to_string(),
        color_charging: settings.string("color-charging").to_string(),
    })
}

/// Poll GSettings once per second and yield `BatteryColors` whenever
/// the value changes. Used as an iced `Subscription::run` source.
///
/// `gio::Settings` is `!Send`, so we open and read it inside
/// `spawn_blocking` on each tick rather than holding it across an
/// `await`. This avoids the `LocalSet` complexity.
///
/// If the schema isn't installed (extension not present), emits a single
/// `BatteryColors::default()` and then terminates — the subscriber
/// receives the default value and never sees another event.
pub fn poll_stream()
-> impl futures_util::stream::Stream<Item = BatteryColors>
{
    use futures_util::StreamExt;

    let (tx, rx) = async_channel::unbounded::<BatteryColors>();

    tokio::task::spawn(async move {
        // Use a sentinel threshold value that will never match a real
        // reading to force the first emit.
        let mut last = BatteryColors {
            threshold_critical: 255,
            ..BatteryColors::default()
        };

        loop {
            let current = tokio::task::spawn_blocking(read_from_gsettings)
                .await
                .unwrap_or(None);

            let current = match current {
                Some(c) => c,
                None => {
                    // Schema not found — emit the default once and stop.
                    warn!(
                        "GSettings schema \
                         'org.gnome.shell.extensions.juhradial-indicator' \
                         not found — extension not installed? \
                         Using default battery colours."
                    );
                    let _ = tx.send(BatteryColors::default()).await;
                    return;
                }
            };

            if current != last {
                if tx.send(current.clone()).await.is_err() {
                    return;
                }
                last = current;
            }

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });

    rx.boxed()
}
