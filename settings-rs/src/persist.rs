//! Async config save: serialise the current `AppConfig` to JSON and
//! write atomically to the user's config path. Async only because
//! iced's `Task::perform` wants a future; the actual IO is `std::fs`
//! since the file is small (~2 KB) and writes are infrequent.

use juhradial_shared::AppConfig;
use std::path::PathBuf;

pub async fn save(path: PathBuf, cfg: AppConfig) -> Result<(), String> {
    // Async wrapper around blocking IO. Cheap given the file size.
    tokio::task::spawn_blocking(move || save_blocking(&path, &cfg))
        .await
        .map_err(|e| e.to_string())?
}

fn save_blocking(path: &std::path::Path, cfg: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize: {e}"))?;
    // Atomic write: temp file + rename, so a crash mid-write doesn't
    // truncate the live config the overlay is watching.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))?;
    Ok(())
}
