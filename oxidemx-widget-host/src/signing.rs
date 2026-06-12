//! Bundle signing and verification helpers shared by the CLI (pack/verify/install)
//! and the registry scanner (classifying installed widgets).
//!
//! ## .oxw SIGNATURE entry format
//!
//! The `SIGNATURE` file inside a `.oxw` zip (or inside an installed widget dir)
//! is exactly 96 bytes:
//!
//! ```text
//! [0..32]  — ed25519 verifying key (public key bytes, compressed Edwards point)
//! [32..96] — ed25519 signature over `bundle_digest(bundle)`
//! ```
//!
//! This is self-contained: the verifier does not need an out-of-band public key
//! to check *whether* the signature is cryptographically valid — only to decide
//! *which trust level* to assign (Pinned vs Unknown).
//!
//! ## Digest algorithm
//!
//! `bundle_digest` is sha2-256 over the sorted sequence of `(relative_path, file_bytes)`
//! pairs from all entries **except** the `SIGNATURE` entry itself.  Both the zip-based
//! variant (used by pack + verify + install) and the directory-based variant (used by
//! the registry scanner) produce **identical digests** for the same content, so a bundle
//! packed and then installed can be re-verified from the installed dir.

use std::io::{Read, Seek};
use std::path::Path;

use sha2::{Digest, Sha256};

/// Name of the signature entry inside a `.oxw` zip and inside an installed
/// widget directory.
pub const SIGNATURE_ENTRY: &str = "SIGNATURE";

/// Placeholder pinned registry public key.  Replace with the real key bytes
/// before a production release.
pub const PINNED_REGISTRY_PUBKEY: [u8; 32] = [0u8; 32];

/// Compute the bundle digest for a zip reader.
///
/// Iterates all zip entries sorted by name, excluding the `SIGNATURE` entry,
/// and returns `sha256( concat( (name_bytes ++ file_bytes) ... ) )` where the
/// pairs are sorted lexicographically by name.
///
/// The same digest is produced by [`bundle_digest_from_dir`] for the same
/// content, so an installed widget can be re-verified.
pub fn bundle_digest_from_zip<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<Vec<u8>, BundleError> {
    // Collect all names first so we can sort them.
    let names: Vec<String> = (0..archive.len())
        .map(|i| {
            archive
                .by_index(i)
                .map(|f| f.name().to_string())
                .map_err(|e| BundleError::Zip(e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut pairs: Vec<(String, Vec<u8>)> = Vec::new();
    for name in &names {
        if name == SIGNATURE_ENTRY {
            continue;
        }
        let mut file = archive.by_name(name).map_err(|e| BundleError::Zip(e.to_string()))?;
        if file.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|e| BundleError::Io(e.to_string()))?;
        pairs.push((name.clone(), bytes));
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (name, bytes) in &pairs {
        hasher.update(name.as_bytes());
        hasher.update(bytes);
    }
    Ok(hasher.finalize().to_vec())
}

/// Compute the bundle digest for an installed widget directory.
///
/// Walks the directory recursively, collecting `(relative_path, bytes)` pairs
/// sorted lexicographically by relative path (using `/` as separator, matching
/// zip path conventions), excluding any entry named `SIGNATURE`.
///
/// Produces the same digest as [`bundle_digest_from_zip`] for the same content.
pub fn bundle_digest_from_dir(dir: &Path) -> Result<Vec<u8>, BundleError> {
    let mut pairs: Vec<(String, Vec<u8>)> = Vec::new();
    collect_dir_entries(dir, dir, &mut pairs)?;
    pairs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (name, bytes) in &pairs {
        hasher.update(name.as_bytes());
        hasher.update(bytes);
    }
    Ok(hasher.finalize().to_vec())
}

fn collect_dir_entries(
    base: &Path,
    current: &Path,
    pairs: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), BundleError> {
    let entries = std::fs::read_dir(current)
        .map_err(|e| BundleError::Io(e.to_string()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path
            .strip_prefix(base)
            .map_err(|_| BundleError::Io("path stripping failed".into()))?;
        // Use forward slashes as in zip conventions.
        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if path.is_dir() {
            collect_dir_entries(base, &path, pairs)?;
        } else {
            if rel_str == SIGNATURE_ENTRY {
                continue;
            }
            let bytes = std::fs::read(&path)
                .map_err(|e| BundleError::Io(e.to_string()))?;
            pairs.push((rel_str, bytes));
        }
    }
    Ok(())
}

/// Verify a SIGNATURE blob (96 bytes: 32-byte pubkey + 64-byte signature)
/// against a pre-computed digest.  Returns the raw 32-byte verifying key on
/// success.
pub fn verify_signature(
    sig_bytes: &[u8],
    digest: &[u8],
) -> Result<[u8; 32], BundleError> {
    if sig_bytes.len() != 96 {
        return Err(BundleError::BadSignature(format!(
            "SIGNATURE entry is {} bytes, expected 96",
            sig_bytes.len()
        )));
    }
    let pubkey_bytes: [u8; 32] = sig_bytes[..32].try_into().unwrap();
    let sig_raw: [u8; 64] = sig_bytes[32..96].try_into().unwrap();

    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let vk = VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|e| BundleError::BadSignature(format!("bad public key: {e}")))?;
    let sig = Signature::from_bytes(&sig_raw);
    vk.verify(digest, &sig)
        .map_err(|_| BundleError::BadSignature("signature verification failed".into()))?;
    Ok(pubkey_bytes)
}

/// Return the hex fingerprint (first 8 bytes of the public key).
pub fn fingerprint(pubkey: &[u8; 32]) -> String {
    hex::encode(&pubkey[..8])
}

/// Errors from bundle operations.
#[derive(Debug, Clone)]
pub enum BundleError {
    Zip(String),
    Io(String),
    BadSignature(String),
    MissingEntry(String),
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleError::Zip(s) => write!(f, "zip error: {s}"),
            BundleError::Io(s) => write!(f, "io error: {s}"),
            BundleError::BadSignature(s) => write!(f, "bad signature: {s}"),
            BundleError::MissingEntry(s) => write!(f, "missing entry: {s}"),
        }
    }
}

impl std::error::Error for BundleError {}
