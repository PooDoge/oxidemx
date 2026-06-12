//! Persona files — the OpenClaw-style identity/user split (see
//! docs/plans/agent-features-implementation.md §1).
//!
//! Two plain-markdown files in the config dir, both user-editable
//! and included in config exports (unlike the API key):
//!
//!   * `soul.md` — who the assistant is: persona, tone, values,
//!     boundaries. Injected into every system prompt after the
//!     built-in base text, so it WINS on style conflicts.
//!   * `user.md` — durable facts about the user, distinct from the
//!     rotating memory store (which holds machine-curated facts).
//!
//! Files support optional `## general` / `## settings` H2 sections
//! to scope content to one agent mode; content before any H2 (or a
//! file with no H2s) applies to every mode.
//!
//! The agent may rewrite these ONLY through the `persona` tool —
//! never via consolidation or other machinery. When `soul.md` is
//! missing, the chat's first turn carries a one-time bootstrap
//! instruction (the "first-run ritual"): interview the user
//! briefly, then write both files.

use std::path::PathBuf;

/// Per-file injection cap (OpenClaw's budget) — a runaway file
/// can't eat the context window.
const MAX_FILE_CHARS: usize = 20_000;

fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    PathBuf::from(home).join(".config/oxidemx")
}

pub fn soul_path() -> PathBuf {
    config_dir().join("soul.md")
}

pub fn user_path() -> PathBuf {
    config_dir().join("user.md")
}

/// Read + cap one persona file; `None` when missing/empty.
fn read_capped(path: &PathBuf) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut s = trimmed.to_string();
    if s.len() > MAX_FILE_CHARS {
        // Truncate on a char boundary.
        let mut cut = MAX_FILE_CHARS;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n…(truncated)");
    }
    Some(s)
}

/// Filter a persona file's content to `mode` ("general" or
/// "settings"): keep the preamble (before any `## ` heading) plus
/// any section whose H2 title case-insensitively matches `mode`.
/// A file without H2 sections passes through whole.
fn section_filter(content: &str, mode: &str) -> String {
    if !content.lines().any(|l| l.starts_with("## ")) {
        return content.to_string();
    }
    let mut out = String::new();
    let mut keep = true; // preamble before the first H2
    for line in content.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            keep = title.trim().eq_ignore_ascii_case(mode);
            continue; // headings themselves aren't injected
        }
        if keep {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// The soul block for `mode`, or `None` when no soul.md exists yet
/// (which also signals the first-run ritual).
pub fn soul_block(mode: &str) -> Option<String> {
    let content = read_capped(&soul_path())?;
    let filtered = section_filter(&content, mode);
    (!filtered.is_empty()).then_some(filtered)
}

/// The user-facts block for `mode`, when present.
pub fn user_block(mode: &str) -> Option<String> {
    let content = read_capped(&user_path())?;
    let filtered = section_filter(&content, mode);
    (!filtered.is_empty()).then_some(filtered)
}

/// Whether the first-run ritual should fire: no soul.md at all.
pub fn needs_bootstrap() -> bool {
    !soul_path().exists()
}

/// Write one persona file (full replace; the `persona` tool's
/// backend). Content is capped, parents created. Returns the
/// canonical path written.
pub fn write_file(which: &str, content: &str) -> Result<PathBuf, String> {
    let path = match which {
        "soul" => soul_path(),
        "user" => user_path(),
        other => return Err(format!("unknown persona file '{other}' (soul|user)")),
    };
    let mut body = content.trim().to_string();
    if body.len() > MAX_FILE_CHARS {
        return Err(format!(
            "content exceeds the {MAX_FILE_CHARS}-char persona budget; shorten it"
        ));
    }
    if !body.ends_with('\n') {
        body.push('\n');
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_filter_keeps_preamble_and_matching_section() {
        let content = "Always warm and concise.\n\n## general\nBe playful.\n\n## settings\nBe terse and precise.\n";
        let general = section_filter(content, "general");
        assert!(general.contains("Always warm"));
        assert!(general.contains("Be playful"));
        assert!(!general.contains("terse"));
        let settings = section_filter(content, "settings");
        assert!(settings.contains("terse"));
        assert!(!settings.contains("playful"));
    }

    #[test]
    fn section_filter_passes_unsectioned_files_whole() {
        let content = "Just one persona paragraph.";
        assert_eq!(section_filter(content, "general"), content);
        assert_eq!(section_filter(content, "settings"), content);
    }

    #[test]
    fn write_rejects_unknown_file_and_oversize() {
        assert!(write_file("nope", "x").is_err());
        let huge = "x".repeat(MAX_FILE_CHARS + 1);
        assert!(write_file("soul", &huge).is_err());
    }
}
