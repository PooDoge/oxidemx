//! Core logic for the `oxidemx-widget` CLI.
//!
//! Functions here are public so they can be called directly from integration
//! tests without spawning a child process.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use oxidemx_widget_host::signing::{
    bundle_digest_from_zip, fingerprint, verify_signature, BundleError, PINNED_REGISTRY_PUBKEY,
    SIGNATURE_ENTRY,
};
use oxidemx_widget_host::SignatureState;

// ── Error type ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum CliError {
    Io(std::io::Error),
    Bundle(BundleError),
    Manifest(String),
    InvalidSignature(String),
    IdCollision(String),
    BadKeyFile(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Io(e) => write!(f, "io error: {e}"),
            CliError::Bundle(e) => write!(f, "{e}"),
            CliError::Manifest(s) => write!(f, "manifest error: {s}"),
            CliError::InvalidSignature(s) => write!(f, "invalid signature: {s}"),
            CliError::IdCollision(id) => {
                write!(f, "widget id {id:?} already installed; use --force to replace")
            }
            CliError::BadKeyFile(s) => write!(f, "key file error: {s}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e)
    }
}

impl From<BundleError> for CliError {
    fn from(e: BundleError) -> Self {
        CliError::Bundle(e)
    }
}

// ── Dev key ──────────────────────────────────────────────────────────────────

/// Path to the developer signing key: `~/.config/oxidemx/dev-signing.key`.
/// Can be overridden by setting `OXIDEMX_DEV_KEY_PATH` in the environment
/// (used by tests).
pub fn dev_key_path() -> PathBuf {
    if let Some(p) = std::env::var_os("OXIDEMX_DEV_KEY_PATH") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .expect("HOME or XDG_CONFIG_HOME must be set");
    base.join("oxidemx").join("dev-signing.key")
}

/// Load (or generate) the developer signing key from `~/.config/oxidemx/dev-signing.key`.
///
/// The file stores 32 raw bytes (the `SigningKey` seed).  If the file does not
/// exist it is created with a freshly generated key.
pub fn load_or_generate_dev_key() -> Result<SigningKey, CliError> {
    let path = dev_key_path();
    if path.exists() {
        let bytes = std::fs::read(&path).map_err(CliError::Io)?;
        if bytes.len() != 32 {
            return Err(CliError::BadKeyFile(format!(
                "{} is {} bytes, expected 32",
                path.display(),
                bytes.len()
            )));
        }
        let seed: [u8; 32] = bytes.try_into().unwrap();
        Ok(SigningKey::from_bytes(&seed))
    } else {
        // Generate a fresh key.
        use rand_core::OsRng;
        let key = SigningKey::generate(&mut OsRng);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(CliError::Io)?;
        }
        std::fs::write(&path, key.as_bytes()).map_err(CliError::Io)?;
        Ok(key)
    }
}

/// Build a 96-byte SIGNATURE blob from a signing key and a digest.
///
/// Layout: `[32 bytes verifying key] ++ [64 bytes signature]`
fn make_signature_blob(signing_key: &SigningKey, digest: &[u8]) -> Vec<u8> {
    let sig = signing_key.sign(digest);
    let vk: VerifyingKey = signing_key.verifying_key();
    let mut blob = Vec::with_capacity(96);
    blob.extend_from_slice(vk.as_bytes());
    blob.extend_from_slice(&sig.to_bytes());
    blob
}

// ── Pack ─────────────────────────────────────────────────────────────────────

/// Result of a successful `pack` operation.
#[derive(Debug)]
pub struct PackResult {
    /// Path to the produced `.oxw` file.
    pub path: PathBuf,
    /// Hex fingerprint of the signing key used.
    pub fingerprint: String,
}

/// Pack a widget directory into a `.oxw` zip file, sign with the dev key,
/// and write to `out_path` (defaulting to `<dir_name>.oxw` in the current dir).
///
/// The SIGNATURE entry is the last entry added to the zip, covering all other
/// entries via `bundle_digest_from_zip`.
pub fn pack(widget_dir: &Path, out_path: Option<&Path>) -> Result<PackResult, CliError> {
    // Validate the manifest before packing.
    let manifest_path = widget_dir.join("widget.json");
    let raw = std::fs::read_to_string(&manifest_path)
        .map_err(|e| CliError::Manifest(format!("cannot read widget.json: {e}")))?;
    let manifest: oxidemx_widget_proto::WidgetManifest = serde_json::from_str(&raw)
        .map_err(|e| CliError::Manifest(format!("widget.json does not parse: {e}")))?;
    manifest
        .validate()
        .map_err(|e| CliError::Manifest(format!("manifest validation failed: {e}")))?;
    if manifest.api_version != oxidemx_widget_proto::API_VERSION {
        return Err(CliError::Manifest(format!(
            "api_version {} does not match this tool's version {}",
            manifest.api_version,
            oxidemx_widget_proto::API_VERSION
        )));
    }
    if !widget_dir.join(&manifest.entry).is_file() {
        return Err(CliError::Manifest(format!(
            "entry file {:?} not found in widget dir",
            manifest.entry
        )));
    }
    if !widget_dir.join(&manifest.icon).is_file() {
        return Err(CliError::Manifest(format!(
            "icon file {:?} not found in widget dir",
            manifest.icon
        )));
    }

    // Determine output path.
    let out = match out_path {
        Some(p) => p.to_path_buf(),
        None => {
            let dir_name = widget_dir
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "widget".to_string());
            PathBuf::from(format!("{dir_name}.oxw"))
        }
    };

    // Collect all files in the widget dir, sorted.
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    collect_files_for_zip(widget_dir, widget_dir, &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Write the zip (without SIGNATURE first).
    let zip_bytes = {
        let mut buf = Vec::<u8>::new();
        {
            let mut zw = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, bytes) in &entries {
                zw.start_file(name, options)
                    .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;
                zw.write_all(bytes)
                    .map_err(|e| CliError::Bundle(BundleError::Io(e.to_string())))?;
            }
            zw.finish()
                .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;
        }
        buf
    };

    // Compute the bundle digest over the unsigned zip.
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&zip_bytes))
        .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;
    let digest = bundle_digest_from_zip(&mut archive)?;

    // Sign with the dev key.
    let signing_key = load_or_generate_dev_key()?;
    let sig_blob = make_signature_blob(&signing_key, &digest);
    let fp = fingerprint(&signing_key.verifying_key().to_bytes());

    // Append SIGNATURE entry to zip.
    let final_zip = {
        let mut buf = Vec::<u8>::new();
        {
            let mut zw = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            // Re-add all original entries.
            let mut archive2 = zip::ZipArchive::new(std::io::Cursor::new(&zip_bytes))
                .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;
            for i in 0..archive2.len() {
                let mut f = archive2
                    .by_index(i)
                    .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;
                let entry_options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated);
                zw.start_file(f.name(), entry_options)
                    .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;
                let mut bytes = Vec::new();
                f.read_to_end(&mut bytes)
                    .map_err(|e| CliError::Bundle(BundleError::Io(e.to_string())))?;
                zw.write_all(&bytes)
                    .map_err(|e| CliError::Bundle(BundleError::Io(e.to_string())))?;
            }
            // Append SIGNATURE.
            zw.start_file(SIGNATURE_ENTRY, options)
                .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;
            zw.write_all(&sig_blob)
                .map_err(|e| CliError::Bundle(BundleError::Io(e.to_string())))?;
            zw.finish()
                .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;
        }
        buf
    };

    std::fs::write(&out, &final_zip).map_err(CliError::Io)?;
    Ok(PackResult { path: out, fingerprint: fp })
}

fn collect_files_for_zip(
    base: &Path,
    current: &Path,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), CliError> {
    for entry in std::fs::read_dir(current)?.flatten() {
        let path = entry.path();
        let rel = path.strip_prefix(base).unwrap();
        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if path.is_dir() {
            collect_files_for_zip(base, &path, out)?;
        } else if rel_str != SIGNATURE_ENTRY {
            let bytes = std::fs::read(&path)?;
            out.push((rel_str, bytes));
        }
    }
    Ok(())
}

// ── Verify ───────────────────────────────────────────────────────────────────

/// Result of a successful `verify` operation.
#[derive(Debug)]
pub struct VerifyResult {
    /// The signature state as classified (Pinned / Unknown / Unsigned).
    pub state: SignatureState,
    /// Hex fingerprint of the signer's public key (empty if Unsigned).
    pub fingerprint: String,
}

/// Verify a `.oxw` bundle's SIGNATURE against the pinned registry key.
///
/// Returns `SignatureState::Unknown` for any valid signature from a key that
/// isn't the pinned registry key (e.g. a developer key), and
/// `SignatureState::Unsigned` if there is no SIGNATURE entry.
pub fn verify(bundle_path: &Path) -> Result<VerifyResult, CliError> {
    let bytes = std::fs::read(bundle_path)?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))
        .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;

    // Check SIGNATURE entry exists.
    let sig_bytes = match archive.by_name(SIGNATURE_ENTRY) {
        Ok(mut f) => {
            let mut b = Vec::new();
            f.read_to_end(&mut b)
                .map_err(|e| CliError::Bundle(BundleError::Io(e.to_string())))?;
            b
        }
        Err(_) => {
            return Ok(VerifyResult {
                state: SignatureState::Unsigned,
                fingerprint: String::new(),
            });
        }
    };

    let digest = bundle_digest_from_zip(&mut archive)?;

    match verify_signature(&sig_bytes, &digest) {
        Ok(pubkey) => {
            let fp = fingerprint(&pubkey);
            let state = if pubkey == PINNED_REGISTRY_PUBKEY {
                SignatureState::Pinned
            } else {
                SignatureState::Unknown(fp.clone())
            };
            Ok(VerifyResult { state, fingerprint: fp })
        }
        Err(e) => Err(CliError::InvalidSignature(e.to_string())),
    }
}

// ── Install ──────────────────────────────────────────────────────────────────

/// Result of a successful `install` operation.
#[derive(Debug)]
pub struct InstallResult {
    /// The widget id that was installed.
    pub id: String,
    /// The directory where the widget was installed.
    pub install_dir: PathBuf,
    /// Signature state of the installed widget.
    pub signature_state: SignatureState,
    /// Fingerprint of the signer (empty if Unsigned).
    pub fingerprint: String,
}

/// Install a `.oxw` bundle into `widgets_dir()`.
///
/// Verifies the bundle first.  If the signature is Unknown (dev key or
/// unrecognised key), the installation proceeds — it is the caller's
/// responsibility to gate on `--force` appropriately.
///
/// If `force` is false and the widget id already exists in `install_root`,
/// returns [`CliError::IdCollision`].
pub fn install(
    bundle_path: &Path,
    force: bool,
    install_root: Option<&Path>,
) -> Result<InstallResult, CliError> {
    // Verify first.
    let ver = verify(bundle_path)?;

    // Read bundle to get manifest id.
    let bytes = std::fs::read(bundle_path)?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))
        .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;

    let manifest: oxidemx_widget_proto::WidgetManifest = {
        let mut f = archive
            .by_name("widget.json")
            .map_err(|_| CliError::Manifest("bundle missing widget.json".into()))?;
        let mut s = String::new();
        f.read_to_string(&mut s)
            .map_err(|e| CliError::Bundle(BundleError::Io(e.to_string())))?;
        serde_json::from_str(&s)
            .map_err(|e| CliError::Manifest(format!("widget.json does not parse: {e}")))?
    };

    let id = manifest.id.clone();

    // Resolve installation root.
    let root = match install_root {
        Some(p) => p.to_path_buf(),
        None => oxidemx_widget_host::WidgetRegistry::widgets_dir()
            .ok_or_else(|| CliError::Io(std::io::Error::other("cannot determine widgets_dir")))?,
    };

    let dest = root.join(&id);

    // Check for id collision.
    if dest.exists() && !force {
        return Err(CliError::IdCollision(id));
    }

    // Remove existing dir if force.
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::create_dir_all(&dest)?;

    // Extract all entries (including SIGNATURE) from the zip.
    let mut archive2 = zip::ZipArchive::new(std::io::Cursor::new(&bytes))
        .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;

    for i in 0..archive2.len() {
        let mut file = archive2
            .by_index(i)
            .map_err(|e| CliError::Bundle(BundleError::Zip(e.to_string())))?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().to_string();
        let mut content = Vec::new();
        file.read_to_end(&mut content)
            .map_err(|e| CliError::Bundle(BundleError::Io(e.to_string())))?;
        let target = dest.join(&name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, &content)?;
    }

    Ok(InstallResult {
        id,
        install_dir: dest,
        signature_state: ver.state,
        fingerprint: ver.fingerprint,
    })
}
