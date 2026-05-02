//! Theme palette resolution. Reads the active `ThemeName` from
//! `juhradial_shared`, looks up the bundled palette (eventually shared
//! with the daemon's `bundled_themes.rs`), and exposes a small helper
//! that converts a slice's `color` key (e.g. "green", "sapphire") into
//! a cairo RGBA tuple for rendering.

use juhradial_shared::ThemeName;

pub struct Palette {
    pub fg: (f64, f64, f64, f64),
    pub bg: (f64, f64, f64, f64),
    pub accent: (f64, f64, f64, f64),
    // TODO: full palette parity with daemon/src/bundled_themes.rs
    //       (slice colors map: green / sapphire / teal / pink / lavender /
    //       peach / yellow / mauve / red).
}

pub fn load(_name: &ThemeName) -> Palette {
    // TODO: port bundled-theme palettes; until then, a debug placeholder
    // so the rest of the rendering pipeline can compile.
    Palette {
        fg: (0.86, 0.87, 0.91, 1.0),
        bg: (0.12, 0.12, 0.18, 0.85),
        accent: (0.30, 0.69, 0.92, 1.0),
    }
}
