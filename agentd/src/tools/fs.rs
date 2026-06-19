//! Concrete implementations of the 7 native tools.
//!
//! All filesystem tools are cwd-scoped and escape-guarded.
//! `list_system_apps` and `google_search` are host-scope (no cwd scoping).

use std::path::{Path, PathBuf};
use serde_json::json;

// ── Escape guard ──────────────────────────────────────────────────────────────

/// Resolve `user_path` relative to `base`, then canonicalize and verify it
/// stays inside `base`. Returns the canonicalized absolute path on success.
pub(super) fn resolve_and_guard(base: &Path, user_path: &str) -> Result<PathBuf, String> {
    let p = if Path::new(user_path).is_absolute() {
        PathBuf::from(user_path)
    } else {
        base.join(user_path)
    };
    let canonical = std::fs::canonicalize(&p)
        .map_err(|e| format!("cannot resolve path: {e}"))?;
    let canon_base = std::fs::canonicalize(base)
        .map_err(|e| format!("cannot resolve cwd: {e}"))?;
    if !canonical.starts_with(&canon_base) {
        return Err(format!("path escapes project directory: {user_path}"));
    }
    Ok(canonical)
}

// ── 1. read_file ─────────────────────────────────────────────────────────────

pub(super) fn read_file(cwd: &Path, args: &serde_json::Value) -> Result<String, String> {
    let path = args["path"]
        .as_str()
        .ok_or_else(|| "read_file: missing 'path' argument".to_string())?;
    let resolved = resolve_and_guard(cwd, path)?;
    std::fs::read_to_string(&resolved)
        .map_err(|e| format!("read_file: cannot read '{}': {e}", resolved.display()))
}

// ── 2. list_dir ──────────────────────────────────────────────────────────────

pub(super) fn list_dir(cwd: &Path, args: &serde_json::Value) -> Result<String, String> {
    let path = args["path"].as_str().unwrap_or(".");
    let resolved = resolve_and_guard(cwd, path)?;
    let mut entries: Vec<String> = std::fs::read_dir(&resolved)
        .map_err(|e| format!("list_dir: cannot read directory '{}': {e}", resolved.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    serde_json::to_string(&entries).map_err(|e| format!("list_dir: json error: {e}"))
}

// ── 3. search_file ───────────────────────────────────────────────────────────

pub(super) fn search_file(cwd: &Path, args: &serde_json::Value) -> Result<String, String> {
    let path = args["path"].as_str().unwrap_or(".");
    let pattern = args["pattern"]
        .as_str()
        .ok_or_else(|| "search_file: missing 'pattern' argument".to_string())?;
    let resolved = resolve_and_guard(cwd, path)?;
    let canon_base = std::fs::canonicalize(cwd)
        .map_err(|e| format!("search_file: cannot resolve cwd: {e}"))?;

    let mut matches: Vec<String> = Vec::new();
    walk_dir(&resolved, pattern, &canon_base, &mut matches)?;
    matches.sort();
    serde_json::to_string(&matches).map_err(|e| format!("search_file: json error: {e}"))
}

fn walk_dir(
    dir: &Path,
    pattern: &str,
    cwd: &Path,
    out: &mut Vec<String>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("search_file: cannot read '{}': {e}", dir.display()))?;
    for entry in entries.flatten() {
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if ft.is_dir() {
            let _ = walk_dir(&entry.path(), pattern, cwd, out);
        } else if name.contains(pattern) {
            // Return path relative to cwd.
            if let Ok(rel) = entry.path().strip_prefix(cwd) {
                out.push(rel.to_string_lossy().into_owned());
            } else {
                out.push(entry.path().to_string_lossy().into_owned());
            }
        }
    }
    Ok(())
}

// ── 4. parse_document ────────────────────────────────────────────────────────

pub(super) fn parse_document(cwd: &Path, args: &serde_json::Value) -> Result<String, String> {
    // For agentd, "parse" means read as UTF-8 text.  Document-parsing via
    // autoagents-toolkit isn't available here; raw text is sufficient for
    // feeding model context.
    let path = args["path"]
        .as_str()
        .ok_or_else(|| "parse_document: missing 'path' argument".to_string())?;
    let resolved = resolve_and_guard(cwd, path)?;
    std::fs::read_to_string(&resolved)
        .map_err(|e| format!("parse_document: cannot read '{}': {e}", resolved.display()))
}

// ── 5. execute_command ───────────────────────────────────────────────────────

pub(super) async fn execute_command(
    cwd: &Path,
    args: &serde_json::Value,
) -> Result<String, String> {
    let command = args["command"]
        .as_str()
        .ok_or_else(|| "execute_command: missing 'command' argument".to_string())?;

    let run = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .output();

    let output = tokio::time::timeout(std::time::Duration::from_secs(10), run)
        .await
        .map_err(|_| "execute_command: timed out after 10 s".to_string())?
        .map_err(|e| format!("execute_command: spawn failed: {e}"))?;

    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = if stderr.is_empty() {
        stdout.into_owned()
    } else {
        format!("{stdout}{stderr}")
    };
    // Cap at 2048 chars.
    let capped: String = combined.chars().take(2048).collect();
    Ok(format!("exit code {code}\noutput:\n{capped}"))
}

// ── 6. list_system_apps ──────────────────────────────────────────────────────

pub(super) fn list_system_apps() -> Result<String, String> {
    let search_dirs = {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        vec![
            PathBuf::from("/usr/share/applications"),
            PathBuf::from("/usr/local/share/applications"),
            PathBuf::from(home).join(".local/share/applications"),
        ]
    };

    let mut apps: Vec<serde_json::Value> = Vec::new();
    for dir in &search_dirs {
        if !dir.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }
            if let Some(app) = parse_desktop_file(&p) {
                apps.push(app);
            }
        }
    }
    apps.sort_by(|a, b| {
        let na = a["name"].as_str().unwrap_or("");
        let nb = b["name"].as_str().unwrap_or("");
        na.cmp(nb)
    });
    serde_json::to_string(&apps).map_err(|e| format!("list_system_apps: json error: {e}"))
}

fn parse_desktop_file(path: &Path) -> Option<serde_json::Value> {
    let src = std::fs::read_to_string(path).ok()?;
    let mut name = String::new();
    let mut exec = String::new();
    let mut icon = String::new();
    let mut categories = String::new();
    let mut in_desktop_entry = false;

    for line in src.lines() {
        let line = line.trim();
        if line == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        }
        if line.starts_with('[') {
            in_desktop_entry = false;
        }
        if !in_desktop_entry {
            continue;
        }
        if let Some(val) = line.strip_prefix("Name=") {
            if name.is_empty() {
                name = val.to_string();
            }
        } else if let Some(val) = line.strip_prefix("Exec=") {
            if exec.is_empty() {
                exec = val.to_string();
            }
        } else if let Some(val) = line.strip_prefix("Icon=") {
            if icon.is_empty() {
                icon = val.to_string();
            }
        } else if let Some(val) = line.strip_prefix("Categories=") {
            if categories.is_empty() {
                categories = val.to_string();
            }
        }
    }

    if name.is_empty() {
        return None;
    }
    Some(json!({
        "name": name,
        "exec": exec,
        "icon": icon,
        "categories": categories,
    }))
}

// ── 7. google_search ─────────────────────────────────────────────────────────

pub(super) async fn google_search(args: &serde_json::Value) -> Result<String, String> {
    let query = args["query"]
        .as_str()
        .ok_or_else(|| "google_search: missing 'query' argument".to_string())?;

    let key = match oxidemx_agent_core::api_key::load_api_key() {
        Ok(k) => k,
        Err(e) => return Ok(format!("google_search unavailable: {e}")),
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={key}"
    );
    let body = json!({
        "contents": [{"role": "user", "parts": [{"text": format!("Search and summarize: {query}")}]}],
        "tools": [{"google_search": {}}]
    });

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => return Ok(format!("google_search unavailable: {e}")),
    };

    let resp = match client.post(&url).json(&body).send().await {
        Ok(r) => r,
        Err(e) => return Ok(format!("google_search unavailable: {e}")),
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return Ok(format!("google_search unavailable: {e}")),
    };

    let text = json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            // Fall back to rendering the raw response if text isn't where expected.
            serde_json::to_string_pretty(&json)
                .unwrap_or_else(|_| "google_search: unexpected response shape".to_string())
        });
    Ok(text)
}
