//! Concrete implementations of the 7 native tools.
//!
//! All filesystem tools are cwd-scoped and escape-guarded.
//! `list_system_apps` and `google_search` are host-scope (no cwd scoping).

use std::path::{Component, Path, PathBuf};
use serde_json::json;

// ── Arg-key helpers ───────────────────────────────────────────────────────────

/// Try a list of JSON keys in order and return the first `str` value found.
fn pick_str<'a>(args: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    for k in keys {
        if let Some(v) = args.get(*k).and_then(|v| v.as_str()) {
            return Some(v);
        }
    }
    None
}

// ── Escape guard ──────────────────────────────────────────────────────────────

/// Normalize `user_path` relative to `base` (WITHOUT requiring the path to
/// exist), then verify the result stays inside `base`.
///
/// Uses manual `..`/`.` component processing so missing files don't produce a
/// confusing "cannot resolve path" error — the caller gets a clean
/// "No such file or directory" when it actually tries to read.
///
/// After normalization, if the target exists and is a symlink, the symlink is
/// resolved via `canonicalize` and the resulting real path is re-checked
/// against the canonicalized `base`.
pub(super) fn resolve_and_guard(base: &Path, user_path: &str) -> Result<PathBuf, String> {
    // Canonicalize the base (requires it to exist, which is always true in
    // normal operation — the cwd must exist).
    let canon_base = std::fs::canonicalize(base)
        .map_err(|e| format!("cannot resolve cwd: {e}"))?;

    // Build the un-normalized candidate: absolute or relative to base.
    let raw = if Path::new(user_path).is_absolute() {
        PathBuf::from(user_path)
    } else {
        base.join(user_path)
    };

    // Normalize `..` and `.` without hitting the filesystem.
    let mut normalized = PathBuf::new();
    for component in raw.components() {
        match component {
            Component::Prefix(p) => { normalized.push(p.as_os_str()); }
            Component::RootDir   => { normalized.push("/"); }
            Component::CurDir    => {} // skip `.`
            Component::ParentDir => { normalized.pop(); } // collapse `..`
            Component::Normal(n) => { normalized.push(n); }
        }
    }

    // Escape check on the normalized path.
    if !normalized.starts_with(&canon_base) {
        return Err(format!("path escapes project directory: {user_path}"));
    }

    // If the path exists and is a symlink, resolve and re-check.
    if normalized.is_symlink() {
        let resolved = std::fs::canonicalize(&normalized)
            .map_err(|e| format!("cannot resolve symlink: {e}"))?;
        if !resolved.starts_with(&canon_base) {
            return Err(format!("symlink escapes project directory: {user_path}"));
        }
        return Ok(resolved);
    }

    Ok(normalized)
}

// ── 1. read_file ─────────────────────────────────────────────────────────────

pub(super) fn read_file(cwd: &Path, args: &serde_json::Value) -> Result<String, String> {
    // Declared key: `file_path`.  Fall back to legacy keys for tolerance.
    let path = pick_str(args, &["file_path", "path", "source"])
        .ok_or_else(|| "read_file: missing 'file_path' argument".to_string())?;
    let resolved = resolve_and_guard(cwd, path)?;
    std::fs::read_to_string(&resolved)
        .map_err(|e| format!("read_file: '{}': {e}", resolved.display()))
}

// ── 2. list_dir ──────────────────────────────────────────────────────────────

pub(super) fn list_dir(cwd: &Path, args: &serde_json::Value) -> Result<String, String> {
    // Declared key: `directory_path`.  Fall back to legacy keys.
    let path = pick_str(args, &["directory_path", "path", "directory"]).unwrap_or(".");
    let resolved = resolve_and_guard(cwd, path)?;
    let mut entries: Vec<serde_json::Value> = std::fs::read_dir(&resolved)
        .map_err(|e| format!("list_dir: cannot read directory '{}': {e}", resolved.display()))?
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let full = e.path();
            let meta = e.metadata().ok();
            let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
            let size = meta.as_ref().map_or(0, |m| m.len());
            let path_str = full.to_string_lossy().into_owned();
            json!({
                "name": name,
                "path": path_str,
                "is_dir": is_dir,
                "size": size,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        let na = a["name"].as_str().unwrap_or("");
        let nb = b["name"].as_str().unwrap_or("");
        na.cmp(nb)
    });
    serde_json::to_string(&entries).map_err(|e| format!("list_dir: json error: {e}"))
}

// ── 3. search_file ───────────────────────────────────────────────────────────

pub(super) fn search_file(cwd: &Path, args: &serde_json::Value) -> Result<String, String> {
    // Declared keys: `directory` + `pattern`.  Fall back to legacy path keys.
    let path = pick_str(args, &["directory", "directory_path", "path"]).unwrap_or(".");
    let pattern = pick_str(args, &["pattern"])
        .ok_or_else(|| "search_file: missing 'pattern' argument".to_string())?;
    let resolved = resolve_and_guard(cwd, path)?;
    let canon_base = std::fs::canonicalize(cwd)
        .map_err(|e| format!("search_file: cannot resolve cwd: {e}"))?;

    let mut matches: Vec<serde_json::Value> = Vec::new();
    walk_dir(&resolved, pattern, &canon_base, &mut matches)?;
    matches.sort_by(|a, b| {
        let pa = a["path"].as_str().unwrap_or("");
        let pb = b["path"].as_str().unwrap_or("");
        pa.cmp(pb)
    });
    serde_json::to_string(&matches).map_err(|e| format!("search_file: json error: {e}"))
}

/// Wildcard match for a filename against a glob-style pattern.
/// Supports `*` (any sequence of chars) and `?` (any single char).
/// Only the filename component is matched, not directory separators.
fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    glob_match_inner(&p, &n)
}

fn glob_match_inner(p: &[char], n: &[char]) -> bool {
    match (p, n) {
        ([], [])           => true,
        ([], _)            => false,
        (['*', rest @ ..], _) => {
            // `*` matches zero or more characters.
            if glob_match_inner(rest, n) {
                return true;
            }
            for i in 0..n.len() {
                if glob_match_inner(rest, &n[i + 1..]) {
                    return true;
                }
            }
            false
        }
        (['?', pr @ ..], [_, nr @ ..]) => glob_match_inner(pr, nr),
        (['?', ..], [])                => false,
        ([pc, pr @ ..], [nc, nr @ ..]) if pc == nc => glob_match_inner(pr, nr),
        _                              => false,
    }
}

fn walk_dir(
    dir: &Path,
    pattern: &str,
    cwd: &Path,
    out: &mut Vec<serde_json::Value>,
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
        } else if glob_match(pattern, &name) {
            let full = entry.path();
            let rel = full
                .strip_prefix(cwd)
                .map(|r| r.to_string_lossy().into_owned())
                .unwrap_or_else(|_| full.to_string_lossy().into_owned());
            let meta = entry.metadata().ok();
            let size = meta.as_ref().map_or(0, |m| m.len());
            out.push(json!({
                "name": name,
                "path": rel,
                "is_dir": false,
                "size": size,
            }));
        }
    }
    Ok(())
}

// ── 4. parse_document ────────────────────────────────────────────────────────

pub(super) fn parse_document(cwd: &Path, args: &serde_json::Value) -> Result<String, String> {
    // TODO(SP1c-followup): restore structured PDF/DOCX/etc parsing via autoagents-toolkit
    //
    // For agentd, "parse" means read as UTF-8 text.  Document-parsing via
    // autoagents-toolkit isn't available here; raw text is sufficient for
    // feeding model context.

    // Declared key: `source`.  Fall back to other path keys for tolerance.
    let path = pick_str(args, &["source", "file_path", "path"])
        .ok_or_else(|| "parse_document: missing 'source' argument".to_string())?;
    let resolved = resolve_and_guard(cwd, path)?;
    std::fs::read_to_string(&resolved)
        .map_err(|e| format!("parse_document: '{}': {e}", resolved.display()))
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
    // Declared key: `query`.
    let query = match pick_str(args, &["query"]) {
        Some(q) => q,
        None => {
            return Ok(json!({"error": "google_search unavailable: missing 'query' argument"}).to_string());
        }
    };

    let key = match oxidemx_agent_core::api_key::load_api_key() {
        Ok(k) => k,
        Err(e) => return Ok(json!({"error": format!("google_search unavailable: {e}")}).to_string()),
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
        Err(e) => return Ok(json!({"error": format!("google_search unavailable: {e}")}).to_string()),
    };

    let resp = match client.post(&url).json(&body).send().await {
        Ok(r) => r,
        Err(e) => return Ok(json!({"error": format!("google_search unavailable: {e}")}).to_string()),
    };

    let jval: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return Ok(json!({"error": format!("google_search unavailable: {e}")}).to_string()),
    };

    let text = jval["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            // Fall back to rendering the raw response if text isn't where expected.
            serde_json::to_string_pretty(&jval)
                .unwrap_or_else(|_| "google_search: unexpected response shape".to_string())
        });
    Ok(text)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── glob_match ────────────────────────────────────────────────────────────

    #[test]
    fn glob_star_rs_matches_rs_files() {
        assert!(glob_match("*.rs", "foo.rs"), "*.rs should match foo.rs");
        assert!(!glob_match("*.rs", "foo.txt"), "*.rs should not match foo.txt");
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", ""));
    }

    #[test]
    fn glob_question_mark() {
        assert!(glob_match("foo?.txt", "fooa.txt"));
        assert!(!glob_match("foo?.txt", "foo.txt"));
        assert!(!glob_match("foo?.txt", "fooab.txt"));
    }

    #[test]
    fn glob_exact_match() {
        assert!(glob_match("exact.md", "exact.md"));
        assert!(!glob_match("exact.md", "other.md"));
    }

    #[test]
    fn glob_star_only_matches_anything() {
        assert!(glob_match("*", "anything.rs"));
        assert!(glob_match("*", "a"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn glob_no_pattern_chars() {
        assert!(glob_match("readme", "readme"));
        assert!(!glob_match("readme", "README"));
    }

    // ── resolve_and_guard ─────────────────────────────────────────────────────

    #[test]
    fn escape_guard_rejects_dotdot() {
        let d = tempfile::tempdir().unwrap();
        assert!(resolve_and_guard(d.path(), "../../etc/passwd").is_err());
    }

    #[test]
    fn escape_guard_allows_in_cwd() {
        let d = tempfile::tempdir().unwrap();
        // File does not need to exist — guard only checks the path prefix.
        let result = resolve_and_guard(d.path(), "subdir/file.txt");
        assert!(result.is_ok(), "in-cwd path should pass guard: {result:?}");
    }

    #[test]
    fn missing_file_in_cwd_passes_guard() {
        let d = tempfile::tempdir().unwrap();
        // The file doesn't exist — guard should still succeed (no canonicalize on target).
        let result = resolve_and_guard(d.path(), "nonexistent.txt");
        assert!(result.is_ok(), "missing-but-in-cwd path should pass guard");
        // The actual read should then fail with a not-found error, not "escapes".
        let read_err = read_file(d.path(), &serde_json::json!({"file_path": "nonexistent.txt"}))
            .unwrap_err();
        assert!(
            !read_err.contains("escapes"),
            "missing file error must not say 'escapes': {read_err}"
        );
    }
}
