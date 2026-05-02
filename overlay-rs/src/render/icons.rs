//! Icon resolver — XDG theme lookup + SVG/PNG rasterisation + alpha-
//! mask tinting, cached per `(source, size, color)` so the per-frame
//! cost is just a hashmap lookup once each cache key is warm.
//!
//! Resolution order for a slice's `icon` field:
//!
//!   1. **Absolute path** → `image` decodes PNG/JPEG; `resvg` +
//!      `tiny-skia` rasterise SVG.
//!   2. **freedesktop icon name** → walk the standard XDG icon
//!      directories (current GTK theme + `hicolor` fallback +
//!      Adwaita symbolic) for a matching `<name>.svg` /
//!      `<name>-symbolic.svg` / `<name>.png`. Self-contained — no
//!      `gtk-rs` / `linicon` dep.
//!   3. **Otherwise** → no icon (slice renders the placeholder
//!      colour dot from `render::slices`).
//!
//! Tinting composites the slice colour over the icon's alpha mask
//! (effectively `cairo::Operator::SourceIn`), so the silhouette is
//! preserved but recoloured. The freedesktop "symbolic" icon
//! convention treats them as alpha masks anyway, so this matches
//! expectations.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use iced::widget::canvas::{Frame, Image};
use iced::widget::image::Handle;
use iced::{Point, Rectangle, Size};

const DEFAULT_THEME: &str = "Adwaita";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    source: String,
    size: u32,
    /// Packed colour (0xAARRGGBB).
    color: u32,
}

/// Per-overlay icon cache. Owned by RadialState; rendering happens
/// on the iced main thread so a `RefCell` is enough.
#[derive(Default)]
pub struct IconCache {
    cache: RefCell<HashMap<CacheKey, Handle>>,
}

impl IconCache {
    pub fn new() -> Self {
        IconCache::default()
    }

    /// Resolve an icon to an iced `image::Handle`. Returns `None`
    /// when the source can't be loaded — caller falls back to the
    /// placeholder dot.
    pub fn resolve(
        &self,
        source: &str,
        size: u32,
        color_rgba: (f32, f32, f32, f32),
    ) -> Option<Handle> {
        if source.is_empty() || size == 0 {
            return None;
        }
        let key = CacheKey {
            source: source.to_string(),
            size,
            color: pack_color(color_rgba),
        };
        if let Some(h) = self.cache.borrow().get(&key) {
            return Some(h.clone());
        }
        let raw = load_raw_rgba(source, size)?;
        let tinted = tint_alpha_mask(&raw, color_rgba);
        let handle = Handle::from_rgba(size, size, tinted);
        self.cache.borrow_mut().insert(key, handle.clone());
        Some(handle)
    }
}

/// Composite an icon `handle` centred at `(cx, cy)` into `frame`.
pub fn draw_icon(frame: &mut Frame, cx: f32, cy: f32, size: f32, handle: &Handle) {
    let bounds = Rectangle::new(
        Point::new(cx - size / 2.0, cy - size / 2.0),
        Size::new(size, size),
    );
    frame.draw_image(bounds, Image::new(handle.clone()));
}

// =============================================================================
// loading
// =============================================================================

fn load_raw_rgba(source: &str, size: u32) -> Option<Vec<u8>> {
    let path = if Path::new(source).is_absolute() && Path::new(source).exists() {
        PathBuf::from(source)
    } else {
        find_themed_icon(source)?
    };

    match path.extension().and_then(|s| s.to_str()) {
        Some("svg") | Some("SVG") => rasterise_svg(&path, size),
        _ => rasterise_raster(&path, size),
    }
}

fn rasterise_svg(path: &Path, size: u32) -> Option<Vec<u8>> {
    let svg_bytes = std::fs::read(path).ok()?;
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&svg_bytes, &opt).ok()?;

    let tree_size = tree.size();
    let scale = (size as f32) / tree_size.width().max(tree_size.height());
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);

    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(pixmap.take())
}

fn rasterise_raster(path: &Path, size: u32) -> Option<Vec<u8>> {
    let dyn_img = image::open(path).ok()?;
    let resized = dyn_img.resize_exact(
        size,
        size,
        image::imageops::FilterType::Lanczos3,
    );
    let rgba = resized.to_rgba8();
    Some(rgba.into_raw())
}

// =============================================================================
// tinting
// =============================================================================

/// Replace each pixel's RGB with `color` while preserving its alpha.
/// `cairo::Operator::SourceIn` equivalent; ideal for symbolic icons.
fn tint_alpha_mask(rgba: &[u8], color: (f32, f32, f32, f32)) -> Vec<u8> {
    let mut out = vec![0u8; rgba.len()];
    let r = (color.0.clamp(0.0, 1.0) * 255.0) as u8;
    let g = (color.1.clamp(0.0, 1.0) * 255.0) as u8;
    let b = (color.2.clamp(0.0, 1.0) * 255.0) as u8;
    for (i, chunk) in rgba.chunks_exact(4).enumerate() {
        out[i * 4]     = r;
        out[i * 4 + 1] = g;
        out[i * 4 + 2] = b;
        out[i * 4 + 3] = chunk[3];
    }
    out
}

// =============================================================================
// XDG icon theme lookup
// =============================================================================

fn find_themed_icon(name: &str) -> Option<PathBuf> {
    let theme = current_icon_theme();
    let candidates = candidate_filenames(name);

    for theme_name in [theme.as_str(), DEFAULT_THEME, "hicolor"] {
        for dir in icon_search_dirs() {
            let theme_dir = dir.join(theme_name);
            if !theme_dir.is_dir() {
                continue;
            }
            for candidate in &candidates {
                if let Some(p) = walk_for(&theme_dir, candidate) {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn candidate_filenames(name: &str) -> Vec<String> {
    let stem = name
        .strip_suffix("-symbolic")
        .map(str::to_string)
        .unwrap_or_else(|| name.to_string());
    let mut out = Vec::new();
    if name != stem {
        // User passed `<name>-symbolic`; preserve that exact form
        // first because most themes use it literally.
        out.push(format!("{name}.svg"));
        out.push(format!("{name}.png"));
    }
    out.push(format!("{stem}-symbolic.svg"));
    out.push(format!("{stem}.svg"));
    out.push(format!("{stem}-symbolic.png"));
    out.push(format!("{stem}.png"));
    out
}

fn walk_for(theme_dir: &Path, filename: &str) -> Option<PathBuf> {
    let mut stack = vec![theme_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.file_name().and_then(|s| s.to_str()) == Some(filename) {
                return Some(path);
            }
        }
    }
    None
}

fn current_icon_theme() -> String {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output()
        .ok()
        .map(|o| {
            let raw = String::from_utf8_lossy(&o.stdout);
            raw.trim().trim_matches('\'').to_string()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_THEME.to_string())
}

fn icon_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/icons"));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_DIRS") {
        for d in std::env::split_paths(&xdg) {
            dirs.push(d.join("icons"));
        }
    }
    dirs.push(PathBuf::from("/usr/local/share/icons"));
    dirs.push(PathBuf::from("/usr/share/icons"));
    dirs.push(PathBuf::from("/usr/share/pixmaps"));
    dirs
}

fn pack_color((r, g, b, a): (f32, f32, f32, f32)) -> u32 {
    let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u32;
    (to_u8(a) << 24) | (to_u8(r) << 16) | (to_u8(g) << 8) | to_u8(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_filenames_keeps_symbolic_literal_first() {
        let v = candidate_filenames("utilities-terminal-symbolic");
        let lit_idx = v
            .iter()
            .position(|s| s == "utilities-terminal-symbolic.svg")
            .expect("literal symbolic .svg present");
        let bare_idx = v
            .iter()
            .position(|s| s == "utilities-terminal.svg")
            .expect("bare .svg present");
        assert!(lit_idx < bare_idx);
    }

    #[test]
    fn pack_color_roundtrip() {
        assert_eq!(pack_color((1.0, 0.0, 0.0, 1.0)), 0xFFFF0000);
        assert_eq!(pack_color((0.0, 1.0, 0.0, 1.0)), 0xFF00FF00);
    }
}
