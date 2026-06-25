//! Clipboard image paste helpers for the Composer.
//!
//! `image_data_to_attachment` is PURE (testable without a display server):
//! it PNG-encodes raw RGBA8 bytes and wraps them in an `Attachment`.
//!
//! `read_clipboard_image` is IMPURE: it opens the OS clipboard via `arboard`,
//! reads an image if one is present, and delegates to `image_data_to_attachment`.
//! All error paths use `?` / `.ok()?`; the function never panics.
use std::io::Cursor;

use image::{ImageFormat, RgbaImage};

use super::attachment::{AttachData, AttachKind, Attachment};
use crate::tokens::Tone;

// ── Pure helper ───────────────────────────────────────────────────────────────

/// PNG-encode `rgba` (raw RGBA8, row-major, `width * height * 4` bytes) and
/// return an `Attachment`.
///
/// Returns `None` if `from_raw` rejects the dimensions / buffer size or if
/// PNG encoding fails.  Never panics.
pub fn image_data_to_attachment(width: usize, height: usize, rgba: &[u8]) -> Option<Attachment> {
    let img = RgbaImage::from_raw(width as u32, height as u32, rgba.to_vec())?;

    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png).ok()?;

    Some(Attachment {
        icon: "camera",
        tone: Tone::Mauve,
        name: "pasted-image.png".into(),
        meta: format!("{width}\u{00d7}{height}"),
        kind: AttachKind::Image,
        data: AttachData::Image(buf),
    })
}

// ── Impure clipboard reader ───────────────────────────────────────────────────

/// Read an image from the OS clipboard and return it as an `Attachment`.
///
/// Returns `None` if the clipboard is unavailable, contains no image, or
/// encoding fails.  Never panics.
pub fn read_clipboard_image() -> Option<Attachment> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let img    = cb.get_image().ok()?;
    image_data_to_attachment(img.width, img.height, &img.bytes)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::composer::attachment::{AttachData, AttachKind};

    #[test]
    fn rgba_2x2_becomes_image_attachment() {
        let rgba = vec![255u8; 2 * 2 * 4]; // 2×2 opaque white
        let a = image_data_to_attachment(2, 2, &rgba).expect("encodes");
        assert_eq!(a.kind, AttachKind::Image);
        assert_eq!(a.meta, "2\u{00d7}2");
        match a.data {
            AttachData::Image(bytes) => {
                assert!(
                    bytes.starts_with(&[0x89, b'P', b'N', b'G']),
                    "expected PNG magic bytes"
                )
            }
            _ => panic!("expected AttachData::Image"),
        }
    }

    #[test]
    fn empty_rgba_returns_none() {
        // Wrong buffer length for the given dimensions — from_raw must reject.
        let result = image_data_to_attachment(100, 100, &[]);
        assert!(result.is_none(), "empty buffer should return None");
    }
}
