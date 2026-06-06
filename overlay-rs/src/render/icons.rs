//! Iced wrapper around the shared `oxidemx_icons` resolver.
//!
//! The resolver itself (XDG theme walk + SVG/PNG raster + alpha-mask
//! tinting + cache) lives in the toolkit-free `oxidemx-icons`
//! crate so the settings GUI can share it. This module's job is
//! just to convert the resolver's `RasterIcon` into an
//! `iced::widget::image::Handle` and provide a `Frame::draw_image`
//! convenience for the canvas Painter.

use iced::widget::canvas::{Frame, Image};
use iced::widget::image::Handle;
use iced::{Point, Rectangle, Size};
use oxidemx_icons::{IconCache as RawCache, RasterIcon};
use std::cell::RefCell;
use std::collections::HashMap;

/// Per-overlay icon cache. Wraps the toolkit-free resolver and
/// memoises the iced `Handle` so we don't re-upload the same
/// pixels to the GPU per frame.
#[derive(Default)]
pub struct IconCache {
    raw: RawCache,
    handles: RefCell<HashMap<HandleKey, Handle>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HandleKey {
    source: String,
    size: u32,
    color: u32,
}

impl IconCache {
    pub fn new() -> Self {
        IconCache::default()
    }

    /// Resolve `source` (XDG icon name OR absolute path) at `size`
    /// pixels, tinted by `color_rgba`. Returns `None` if the source
    /// can't be loaded.
    pub fn resolve(
        &self,
        source: &str,
        size: u32,
        color_rgba: (f32, f32, f32, f32),
    ) -> Option<Handle> {
        if source.is_empty() || size == 0 {
            return None;
        }
        let key = HandleKey {
            source: source.to_string(),
            size,
            color: pack_color(color_rgba),
        };
        if let Some(h) = self.handles.borrow().get(&key) {
            return Some(h.clone());
        }
        let icon: RasterIcon = self.raw.resolve(source, size, color_rgba)?;
        let handle = Handle::from_rgba(icon.size, icon.size, icon.rgba);
        self.handles.borrow_mut().insert(key, handle.clone());
        Some(handle)
    }

    /// Resolve `source` at `size` pixels with **original colours
    /// preserved** (no alpha-mask tint). Used for slices marked
    /// `icon_untinted` — typically full-colour app icons where
    /// brand identity matters more than slice-colour theming.
    /// Cached separately from tinted entries via the `color = 0`
    /// sentinel key.
    pub fn resolve_untinted(&self, source: &str, size: u32) -> Option<Handle> {
        if source.is_empty() || size == 0 {
            return None;
        }
        let key = HandleKey {
            source: source.to_string(),
            size,
            color: 0,
        };
        if let Some(h) = self.handles.borrow().get(&key) {
            return Some(h.clone());
        }
        let icon: RasterIcon = self.raw.resolve_untinted(source, size)?;
        let handle = Handle::from_rgba(icon.size, icon.size, icon.rgba);
        self.handles.borrow_mut().insert(key, handle.clone());
        Some(handle)
    }
}

/// Composite an icon `handle` centred at `(cx, cy)` into `frame`
/// at the given `opacity` (0..1). Alpha is applied via iced's
/// per-image multiplier so the cached texture stays alpha=1 — a
/// crossfade ramp re-uses the same handle for every frame instead
/// of forcing a re-raster per alpha step.
pub fn draw_icon(frame: &mut Frame, cx: f32, cy: f32, size: f32, handle: &Handle, opacity: f32) {
    let bounds = Rectangle::new(
        Point::new(cx - size / 2.0, cy - size / 2.0),
        Size::new(size, size),
    );
    frame.draw_image(
        bounds,
        Image::new(handle.clone()).opacity(opacity.clamp(0.0, 1.0)),
    );
}

/// Pack the *RGB* part of an icon tint into a 24-bit cache key.
/// Alpha is intentionally dropped — every frame of a fade animates
/// through dozens of alpha values, and we don't want each step to
/// produce a fresh cache entry. The render-time opacity multiplier
/// (see `draw_icon`) handles the fade without burning the
/// rasteriser.
fn pack_color((r, g, b, _a): (f32, f32, f32, f32)) -> u32 {
    let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u32;
    (to_u8(r) << 16) | (to_u8(g) << 8) | to_u8(b)
}
