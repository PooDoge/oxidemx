//! Per-thread binary attachment store.
//!
//! [`AttachmentStore`] writes raw byte blobs to disk, one file per
//! attachment, keyed by a content-derived hex ID. Blobs are organized as
//! `<root>/<thread_id>/<id>`. This mirrors the structure of
//! [`crate::sessions::TranscriptStore`] (per-thread directories under a
//! configurable root).
//!
//! [`AttachmentRef`] is the serialisable handle returned by [`AttachmentStore::write`]
//! and stored alongside a transcript turn so the caller can retrieve the blob later.

#![forbid(unsafe_code)]

use std::fs;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Hard upper bound on a single attachment. Writes beyond this limit return
/// [`AttachmentError::TooLarge`] without touching the filesystem.
pub const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024; // 20 MiB

// ── AttachmentRef ─────────────────────────────────────────────────────────────

/// A lightweight, serialisable handle to a stored attachment.
///
/// The `id` field is a 16-character lowercase hex string derived from a
/// content hash of the bytes (using `std::hash::DefaultHasher`), so identical
/// blobs written to the same thread share the same id and file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachmentRef {
    /// Content-hash id; used as the filename under `<root>/<thread>/`.
    pub id: String,
    /// MIME type supplied by the caller (e.g. `"image/png"`).
    pub mime: String,
    /// Original filename supplied by the caller.
    pub name: String,
}

// ── AttachmentError ───────────────────────────────────────────────────────────

/// Errors returned by [`AttachmentStore`] operations.
#[derive(Debug, thiserror::Error)]
pub enum AttachmentError {
    /// A filesystem operation failed.
    #[error("attachment I/O error: {0}")]
    Io(String),

    /// The supplied byte slice exceeds [`MAX_ATTACHMENT_BYTES`].
    #[error("attachment too large: {size} bytes (max {MAX_ATTACHMENT_BYTES})")]
    TooLarge { size: usize },

    /// The requested attachment id was not found in the thread directory.
    #[error("attachment not found: thread={thread} id={id}")]
    NotFound { thread: String, id: String },
}

impl From<std::io::Error> for AttachmentError {
    fn from(e: std::io::Error) -> Self {
        AttachmentError::Io(e.to_string())
    }
}

// ── AttachmentStore ───────────────────────────────────────────────────────────

/// Disk-backed blob store for attachments scoped to a conversation thread.
///
/// Layout: `<root>/<thread_id>/<content_id>` — one file per blob, one
/// sub-directory per thread. The root is created lazily on first write.
pub struct AttachmentStore {
    root: PathBuf,
}

impl AttachmentStore {
    /// Create a store rooted at `root`. Cheap and infallible — the directory
    /// is created on first write, not at construction time.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    // ── private helpers ───────────────────────────────────────────────────────

    fn thread_dir(&self, thread: &str) -> PathBuf {
        self.root.join(thread)
    }

    fn blob_path(&self, thread: &str, id: &str) -> PathBuf {
        self.thread_dir(thread).join(id)
    }

    /// Compute a 16-char lowercase hex id from the content bytes.
    ///
    /// Uses `std::hash::DefaultHasher` — no external dep required.
    /// Identical byte slices produce the same id within a process build,
    /// enabling natural dedup within a thread directory.
    fn content_id(bytes: &[u8]) -> String {
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        format!("{:016x}", h.finish())
    }

    // ── public API ────────────────────────────────────────────────────────────

    /// Store `bytes` as an attachment in `thread`, returning a handle.
    ///
    /// Fails with [`AttachmentError::TooLarge`] if `bytes.len() >
    /// MAX_ATTACHMENT_BYTES`. If a blob with the same content id already
    /// exists in the thread directory the file is overwritten (idempotent
    /// for identical content).
    pub fn write(
        &self,
        thread: &str,
        name: &str,
        mime: &str,
        bytes: &[u8],
    ) -> Result<AttachmentRef, AttachmentError> {
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(AttachmentError::TooLarge { size: bytes.len() });
        }
        let id = Self::content_id(bytes);
        let dir = self.thread_dir(thread);
        fs::create_dir_all(&dir)?;
        fs::write(self.blob_path(thread, &id), bytes)?;
        Ok(AttachmentRef {
            id,
            mime: mime.to_string(),
            name: name.to_string(),
        })
    }

    /// Read the raw bytes for a previously stored attachment.
    ///
    /// Returns [`AttachmentError::NotFound`] if the blob does not exist.
    pub fn read(&self, thread: &str, id: &str) -> Result<Vec<u8>, AttachmentError> {
        let path = self.blob_path(thread, id);
        if !path.exists() {
            return Err(AttachmentError::NotFound {
                thread: thread.to_string(),
                id: id.to_string(),
            });
        }
        fs::read(&path).map_err(AttachmentError::from)
    }

    /// Remove all attachments stored for `thread`, deleting the thread
    /// sub-directory and its contents.
    ///
    /// Returns `Ok(())` if the directory does not exist (idempotent cleanup).
    pub fn remove_thread(&self, thread: &str) -> Result<(), AttachmentError> {
        let dir = self.thread_dir(thread);
        if !dir.exists() {
            return Ok(());
        }
        fs::remove_dir_all(&dir).map_err(AttachmentError::from)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── write → read round-trip ───────────────────────────────────────────────

    #[test]
    fn write_then_read_returns_same_bytes() {
        let d = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(d.path().join("attachments"));
        let bytes = b"hello attachment world";
        let r = store.write("t1", "hello.txt", "text/plain", bytes).unwrap();
        assert_eq!(r.mime, "text/plain");
        assert_eq!(r.name, "hello.txt");
        assert!(!r.id.is_empty(), "id must be non-empty");
        let got = store.read("t1", &r.id).unwrap();
        assert_eq!(got, bytes);
    }

    // ── ref fields are preserved ──────────────────────────────────────────────

    #[test]
    fn ref_carries_mime_and_name() {
        let d = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(d.path().join("attachments"));
        let r = store
            .write("t1", "photo.png", "image/png", b"\x89PNG\r\n")
            .unwrap();
        assert_eq!(r.mime, "image/png");
        assert_eq!(r.name, "photo.png");
    }

    // ── oversize → TooLarge ───────────────────────────────────────────────────

    #[test]
    fn oversize_write_returns_too_large_error() {
        let d = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(d.path().join("attachments"));
        // Allocate MAX+1 bytes without touching the heap all at once by using a vec.
        let big = vec![0u8; MAX_ATTACHMENT_BYTES + 1];
        let err = store.write("t1", "big.bin", "application/octet-stream", &big);
        assert!(
            matches!(err, Err(AttachmentError::TooLarge { .. })),
            "expected TooLarge, got {:?}", err
        );
    }

    // ── read of missing id → NotFound ─────────────────────────────────────────

    #[test]
    fn read_missing_id_returns_not_found() {
        let d = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(d.path().join("attachments"));
        let err = store.read("t1", "0000000000000000");
        assert!(
            matches!(err, Err(AttachmentError::NotFound { .. })),
            "expected NotFound, got {:?}", err
        );
    }

    // ── remove_thread deletes the dir ─────────────────────────────────────────

    #[test]
    fn remove_thread_deletes_directory() {
        let d = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(d.path().join("attachments"));
        store.write("t1", "a.txt", "text/plain", b"data").unwrap();
        let dir = d.path().join("attachments").join("t1");
        assert!(dir.exists(), "dir should exist after write");
        store.remove_thread("t1").unwrap();
        assert!(!dir.exists(), "dir should be gone after remove_thread");
    }

    // ── remove_thread on absent thread is ok ─────────────────────────────────

    #[test]
    fn remove_thread_absent_is_ok() {
        let d = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(d.path().join("attachments"));
        // Never wrote anything — should not error.
        store.remove_thread("ghost").unwrap();
    }

    // ── identical content deduplicates within a thread ────────────────────────

    #[test]
    fn identical_bytes_produce_same_id() {
        let d = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(d.path().join("attachments"));
        let bytes = b"same content";
        let r1 = store.write("t1", "a.txt", "text/plain", bytes).unwrap();
        let r2 = store.write("t1", "b.txt", "text/plain", bytes).unwrap();
        assert_eq!(r1.id, r2.id, "same bytes → same id");
        // Both refs read back the correct bytes.
        assert_eq!(store.read("t1", &r1.id).unwrap(), bytes.as_ref());
    }

    // ── threads are isolated ──────────────────────────────────────────────────

    #[test]
    fn threads_are_isolated() {
        let d = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(d.path().join("attachments"));
        let r = store.write("t1", "f.txt", "text/plain", b"data").unwrap();
        // Blob lives in t1, not t2.
        let err = store.read("t2", &r.id);
        assert!(
            matches!(err, Err(AttachmentError::NotFound { .. })),
            "blob from t1 must not be visible in t2"
        );
    }

    // ── AttachmentRef serialises and deserialises cleanly ─────────────────────

    #[test]
    fn attachment_ref_serde_round_trip() {
        let r = AttachmentRef {
            id: "abcd1234abcd1234".to_string(),
            mime: "image/jpeg".to_string(),
            name: "selfie.jpg".to_string(),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: AttachmentRef = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }
}
