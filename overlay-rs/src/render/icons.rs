//! Icon resolution.
//!
//! Resolution order for a slice's `icon` field:
//!   1. Absolute path → load as SVG/PNG via gdk-pixbuf.
//!   2. Recognised internal id (legacy hand-drawn ids: "play_pause",
//!      "folder", "easy_switch", "os_linux", …) → cairo paths in
//!      `render::slices`.
//!   3. Otherwise treat as a freedesktop symbolic icon name and look it
//!      up via `gtk::IconTheme::for_display(display).lookup_icon(...)`.
//!      Tint to the slice colour by compositing in
//!      `cairo::Operator::Atop`.
//!
//! Pixmaps are cached by `(source, pixel_size, color_rgba)` to avoid
//! re-rendering on every frame.

// TODO: implement the cache + the three resolution paths.
