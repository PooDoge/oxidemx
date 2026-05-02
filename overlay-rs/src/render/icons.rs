//! Icon resolver — STUB pending the iced port.
//!
//! The cairo + gdk-pixbuf version that previously lived here is
//! retired; the new iced-based resolver will use `iced::widget::svg`
//! for SVG icons and a `tiny-skia`-rendered raster cache for PNG
//! sources. Until that lands, slice rendering falls back to the
//! per-slice colour dot in `render::slices`.
//!
//! See `RUST_GTK4_OVERLAY_DESIGN.md` Status section for the order
//! the rendering features land in.

#[allow(dead_code)]
pub struct IconCache;

#[allow(dead_code)]
impl IconCache {
    pub fn new() -> Self {
        IconCache
    }
}
