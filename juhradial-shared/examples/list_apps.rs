//! Smoke-test for the application enumerator.
//!
//! Walks the user's actual .desktop directories and prints what we found.
//! Useful for verifying the parser against real-world data.
//!
//! Usage: `cargo run --example list_apps -p juhradial-shared`
fn main() {
    let apps = juhradial_shared::enumerate_applications();
    let flatpak: Vec<_> = apps.iter().filter(|e| e.is_flatpak).collect();
    println!(
        "Found {} visible applications ({} native, {} Flatpak)",
        apps.len(),
        apps.len() - flatpak.len(),
        flatpak.len(),
    );
    println!();
    println!("First 8:");
    for e in apps.iter().take(8) {
        let kind = if e.is_flatpak { "FLATPAK" } else { "native " };
        println!(
            "  [{}] {:30} icon={:30} cmd={}",
            kind, e.name, e.icon, e.slice_command()
        );
    }
    if let Some(query) = std::env::args().nth(1) {
        println!();
        println!("Search '{}':", query);
        for e in juhradial_shared::search_applications(&apps, &query).iter().take(8) {
            let kind = if e.is_flatpak { "FLATPAK" } else { "native " };
            println!("  [{}] {:30} {}", kind, e.name, e.path.display());
        }
    }
}
