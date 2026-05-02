//! Icon resolution.
//!
//! Resolution order for a slice's `icon` field:
//!   1. **Absolute path** → load as SVG/PNG via gdk-pixbuf, cache the
//!      rendered RGBA buffer.
//!   2. **Recognised internal id** (legacy hand-drawn ids:
//!      "play_pause", "folder", "easy_switch", "os_linux", …) → cairo
//!      paths constructed at draw time. *Not yet implemented* —
//!      lands in the cairo-glyphs follow-up commit. Until then we
//!      pass through to the theme lookup, which works for any id
//!      that happens to also be a valid freedesktop name.
//!   3. **Otherwise** treat as a freedesktop symbolic name and look
//!      it up via `gtk::IconTheme::for_display(display)`.
//!      Symbolic icons render in the theme's "fg" colour by default;
//!      we composite our slice colour over the alpha mask in
//!      `cairo::Operator::Atop` to tint to whatever the user picked.
//!
//! Pixmaps are cached by `(source, pixel_size, color_rgba_u32)` so
//! the per-frame paint cost stays negligible (the icon doesn't
//! change between frames unless the user picks a new one).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use cairo::{Context, Format, ImageSurface};
use gdk4 as gdk;
use gdk_pixbuf as gpb;
use gtk4 as gtk;
use gtk::prelude::*;
use tracing::warn;

/// Compact key for the cache. Color is packed into a u32 so the
/// hashmap stays cheap.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    source: String,
    size: i32,
    color: u32,
}

/// Per-overlay icon cache. One instance per `RadialWidget`. Wrapped
/// in `Rc<RefCell<…>>` because rendering happens on the GTK main
/// thread; no need for `Arc<Mutex>`.
#[derive(Default)]
pub struct IconCache {
    map: RefCell<HashMap<CacheKey, ImageSurface>>,
}

impl IconCache {
    pub fn new() -> Rc<Self> {
        Rc::new(IconCache::default())
    }

    /// Resolve an icon string to a tinted cairo `ImageSurface` ready
    /// to composite at any position. Returns `None` when the source
    /// can't be loaded — the caller falls back to a placeholder so a
    /// missing icon never breaks the menu.
    pub fn resolve(
        &self,
        source: &str,
        size: i32,
        color_rgba: (f64, f64, f64, f64),
    ) -> Option<ImageSurface> {
        if source.is_empty() || size <= 0 {
            return None;
        }
        let color_packed = pack_color(color_rgba);
        let key = CacheKey {
            source: source.to_string(),
            size,
            color: color_packed,
        };
        if let Some(s) = self.map.borrow().get(&key) {
            return Some(s.clone());
        }

        let raw = load_raw(source, size)?;
        let tinted = match tint_to_color(&raw, color_rgba) {
            Some(s) => s,
            None => raw,
        };
        self.map.borrow_mut().insert(key, tinted.clone());
        Some(tinted)
    }
}

/// Composite an icon `surface` centred at `(cx, cy)` on `cr`.
pub fn draw_icon(cr: &Context, cx: f64, cy: f64, surface: &ImageSurface) {
    let w = surface.width() as f64;
    let h = surface.height() as f64;
    let x = cx - w / 2.0;
    let y = cy - h / 2.0;
    let _ = cr.set_source_surface(surface, x, y);
    let _ = cr.paint();
}

// =============================================================================
// loading + tinting
// =============================================================================

/// Load the raw (untinted) icon as an `ImageSurface` at the requested
/// pixel size. Tries an absolute filesystem path first, then falls
/// through to the GTK icon theme.
fn load_raw(source: &str, size: i32) -> Option<ImageSurface> {
    let pb = if let Some(p) = absolute_path(source) {
        gpb::Pixbuf::from_file_at_scale(&p, size, size, true).ok()?
    } else if let Some(pb) = load_from_theme(source, size) {
        pb
    } else {
        warn!("icon '{source}' not found in any source");
        return None;
    };
    pixbuf_to_surface(&pb)
}

fn absolute_path(source: &str) -> Option<PathBuf> {
    let p = PathBuf::from(source);
    if p.is_absolute() && p.exists() {
        Some(p)
    } else {
        None
    }
}

fn load_from_theme(name: &str, size: i32) -> Option<gpb::Pixbuf> {
    let display = gdk::Display::default()?;
    let theme = gtk::IconTheme::for_display(&display);
    if !theme.has_icon(name) {
        return None;
    }
    // Fractional scale 1 + the requested size — the theme picks the
    // best matching SVG / scalable variant. FORCE_SYMBOLIC keeps us
    // on the recolourable variant so the tint pass works.
    let info = theme.lookup_icon(
        name,
        &[],
        size,
        1,
        gtk::TextDirection::None,
        gtk::IconLookupFlags::FORCE_SYMBOLIC,
    );
    let file = info.file()?;
    let path = file.path()?;
    gpb::Pixbuf::from_file_at_scale(&path, size, size, true).ok()
}

/// Convert a pixbuf to a cairo `ImageSurface`. Round-trips through
/// PNG bytes so we don't depend on the `cairo_set_source_pixbuf`
/// bridge (which moved between GDK3 and GDK4 — using this approach
/// works regardless of which crate version provides it).
fn pixbuf_to_surface(pb: &gpb::Pixbuf) -> Option<ImageSurface> {
    let bytes = pb.save_to_bufferv("png", &[]).ok()?;
    let mut reader = std::io::Cursor::new(bytes);
    ImageSurface::create_from_png(&mut reader).ok()
}

/// Composite `color` over the alpha mask of `raw`. Source-In keeps
/// only the pixels where the icon was opaque, replacing their colour
/// with our tint. Returns `None` when surface creation fails.
fn tint_to_color(raw: &ImageSurface, color: (f64, f64, f64, f64)) -> Option<ImageSurface> {
    let w = raw.width();
    let h = raw.height();
    let tinted = ImageSurface::create(Format::ARgb32, w, h).ok()?;
    let cr = Context::new(&tinted).ok()?;
    let _ = cr.set_source_surface(raw, 0.0, 0.0);
    let _ = cr.paint();
    cr.set_operator(cairo::Operator::SourceIn);
    cr.set_source_rgba(color.0, color.1, color.2, color.3);
    let _ = cr.paint();
    Some(tinted)
}

fn pack_color((r, g, b, a): (f64, f64, f64, f64)) -> u32 {
    let to_u8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (to_u8(a) << 24) | (to_u8(r) << 16) | (to_u8(g) << 8) | to_u8(b)
}
