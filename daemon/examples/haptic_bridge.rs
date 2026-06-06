//! Standalone harness for the gamepad-rumble → MX Master 4 haptic
//! bridge.
//!
//! Runs *only* the bridge — no D-Bus, no radial overlay, no KWin
//! scripting — so the rumble → haptic path can be exercised against
//! real games without standing up the whole daemon. Connects to the
//! MX Master 4, creates the virtual gamepad in Standalone mode, and
//! holds it until Ctrl-C.
//!
//! ```text
//! cargo run -p oxidemxd --example haptic_bridge
//! ```
//!
//! Then launch a game. Force feedback the game sends to the virtual
//! pad is logged here ("FF effect playing …") and rendered on the
//! mouse.

use oxidemx_shared::{HapticRedirectConfig, HapticRedirectMode};
use oxidemxd::config::HapticConfig;
use oxidemxd::{new_shared_haptic_manager, GamepadHapticsService};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    // Connect a haptic manager to the MX Master 4.
    let haptics = new_shared_haptic_manager(&HapticConfig::default());
    {
        let mut manager = haptics.lock().expect("haptic manager lock");
        match manager.connect() {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("No MX Master 4 found — is it connected and powered on?");
                return;
            }
            Err(e) => {
                eprintln!("Failed to connect to the MX Master 4: {e}");
                return;
            }
        }
        manager.set_enabled(true);
    }

    // Standalone mode — virtual pad only, no real controller proxied.
    let config = HapticRedirectConfig {
        enabled: true,
        mode: HapticRedirectMode::Standalone,
        ..Default::default()
    };
    let service = GamepadHapticsService::start(config, haptics, tokio::runtime::Handle::current());

    println!();
    println!("  +- gamepad-rumble -> MX Master 4 haptic bridge ------------+");
    println!("  |  Virtual gamepad is live. Launch a game now.             |");
    println!("  |  Rumble the game sends is logged here and felt on the    |");
    println!("  |  mouse. Press Ctrl-C to stop.                            |");
    println!("  +----------------------------------------------------------+");
    println!();

    let _ = tokio::signal::ctrl_c().await;

    println!("\nStopping bridge...");
    drop(service);
    // Give the teardown task a beat to destroy the virtual device.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    println!("Stopped.");
}
