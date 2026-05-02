//! Headless overlay — prints what the radial menu would render right
//! now without spinning up a GTK window. Exercises every module in
//! juhradial-shared end-to-end:
//!
//!   * AppConfig load from `~/.config/juhradial/config.json`
//!   * ProfileResolver lookup for a fake focused-class arg
//!   * Theme resolution + bundled palette decode
//!   * `Condition::eval` filter on each slice
//!   * `applications::enumerate_applications` for the icon-source
//!     summary at the end
//!
//! Useful as a CLI smoke test ("does my config parse?") and as a
//! debugging tool ("why isn't the Git slice showing?"). Also lets
//! us verify the whole data pipeline before the GTK4 dev libs are
//! layered.
//!
//! Usage:
//!   cargo run -q -p juhradial-shared --example dump_menu              # main menu
//!   cargo run -q -p juhradial-shared --example dump_menu firefox      # menu for the firefox window class

use juhradial_shared::{ProfileResolver, Theme};

fn main() {
    let focused_class = std::env::args().nth(1);

    println!("== JuhRadial MX — headless menu dump ==");
    println!();

    let resolver = match ProfileResolver::load_default() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to load config: {e}");
            std::process::exit(1);
        }
    };

    let theme_name = &resolver.main_config().theme;
    let theme = match Theme::load(theme_name) {
        Some(t) => t,
        None => {
            eprintln!("Theme '{}' did not load — using default", theme_name);
            Theme::load(&Default::default())
                .expect("default theme must load")
        }
    };
    println!("Theme:     {} ({})", theme.name, theme_name);

    println!(
        "Profiles:  {}",
        if resolver.profile_names().is_empty() {
            "(none configured)".into()
        } else {
            resolver.profile_names().join(", ")
        }
    );
    println!(
        "Bindings:  {}",
        if resolver.bindings().count() == 0 {
            "(no per-app overrides)".into()
        } else {
            resolver
                .bindings()
                .map(|(k, v)| format!("{k} → {v}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!(
        "Focused:   {}",
        focused_class.as_deref().unwrap_or("(none)")
    );

    let menu = resolver.menu_for(focused_class.as_deref());
    println!();
    println!("Active menu — {} slot(s):", menu.slices.len());

    for (i, slice) in menu.slices.iter().enumerate().take(8) {
        let visible = slice
            .visible_if
            .as_ref()
            .map(|c| c.eval())
            .unwrap_or(true);
        let marker = if visible { " " } else { "✗" };
        let cmd = if slice.command.is_empty() {
            "(none)".into()
        } else {
            slice.command.clone()
        };
        let icon = if slice.icon.is_empty() {
            "(no icon)".into()
        } else {
            slice.icon.clone()
        };
        println!(
            "  [{i}] {marker} {label:24} type={kind:?} icon={icon:24} cmd={cmd}",
            i = i,
            marker = marker,
            label = slice.label,
            kind = slice.kind,
            icon = icon,
            cmd = cmd,
        );
        if !slice.submenu.is_empty() {
            for (j, sub) in slice.submenu.iter().enumerate() {
                println!(
                    "       └─ sub[{j}] {label} ({cmd})",
                    label = sub.label,
                    cmd = sub.command,
                );
            }
        }
    }

    println!();
    println!("Slice colour samples (theme palette):");
    for key in [
        "green", "yellow", "red", "blue", "mauve", "pink", "peach", "teal",
        "sapphire", "lavender",
    ] {
        let (r, g, b, _) = theme.colors.slice_color_rgba(key);
        println!(
            "  {key:9} #{:02x}{:02x}{:02x}",
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
        );
    }

    println!();
    println!(
        "Application catalogue: {} entries available for the editor's app picker",
        juhradial_shared::enumerate_applications().len()
    );
}
