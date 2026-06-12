//! Desktop-entry enumeration for the editor's app-launcher tab.
//!
//! Walks the standard XDG application directories *and* the Flatpak
//! per-user / system export trees, parses each `.desktop` file, and
//! returns a list of `DesktopEntry` records the editor can present in
//! a searchable picker. Fuzzy search and a "clean Exec line" helper
//! are included so a one-click "fill command + icon" UI on top of
//! this is straightforward.
//!
//! Pure Rust, no system dependencies (we hand-roll the minimal INI
//! subset we need from the freedesktop spec — keys, header, `Name[lang]`
//! variants, blank lines, and `#` comments). That keeps this module
//! testable in isolation against synthetic .desktop content without
//! GTK / pkg-config / flatpak CLI on the test box.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A single application as the editor sees it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopEntry {
    /// Display name (the unlocalised `Name=` value).
    pub name: String,
    /// Short helper text (`Comment=`). Empty string when the entry
    /// doesn't have one.
    #[serde(default)]
    pub comment: String,
    /// Optional generic name (`GenericName=`).
    #[serde(default)]
    pub generic_name: String,
    /// The `Exec=` line *before* field-code stripping. Use
    /// [`clean_exec_line`] when filling in a slice's `command`.
    pub exec: String,
    /// Either a freedesktop icon name or an absolute filesystem path.
    pub icon: String,
    /// `true` when the entry has the Flatpak marker (`X-Flatpak=…`).
    pub is_flatpak: bool,
    /// Flatpak app id when [`is_flatpak`] (e.g. `com.boxy_svg.BoxySVG`).
    /// Empty string for native entries.
    pub flatpak_app_id: String,
    /// Source path the entry was parsed from.
    pub path: PathBuf,
}

impl DesktopEntry {
    /// Slice command to write into the user's config: the cleaned
    /// `Exec` line with field codes stripped. Most users want this.
    pub fn slice_command(&self) -> String {
        clean_exec_line(&self.exec)
    }
}

/// Parse a single `.desktop` file. Returns `None` for files that are
/// missing either `Name=` or `Exec=`, or whose `[Desktop Entry]`
/// section has `NoDisplay=true` (those entries shouldn't appear in
/// app pickers).
pub fn parse_desktop_file(path: &Path) -> Option<DesktopEntry> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut entry = parse_desktop_string(&raw)?;
    entry.path = path.to_path_buf();
    Some(entry)
}

/// String-only entry-point — used by the unit tests, doesn't touch
/// the filesystem. Identical parser as [`parse_desktop_file`] minus
/// the I/O.
pub fn parse_desktop_string(raw: &str) -> Option<DesktopEntry> {
    let kv = parse_desktop_entry_section(raw);

    if kv.get("NoDisplay").map(String::as_str) == Some("true") {
        return None;
    }
    if kv.get("Type").map(String::as_str) != Some("Application") && kv.contains_key("Type") {
        // Spec says we should also accept entries with no Type key
        // (treat as Application by default), but if the entry
        // *does* declare a Type, only "Application" qualifies.
        return None;
    }

    let name = kv.get("Name")?.clone();
    let exec = kv.get("Exec")?.clone();
    let icon = kv.get("Icon").cloned().unwrap_or_default();
    let comment = kv.get("Comment").cloned().unwrap_or_default();
    let generic_name = kv.get("GenericName").cloned().unwrap_or_default();
    let flatpak_app_id = kv.get("X-Flatpak").cloned().unwrap_or_default();

    Some(DesktopEntry {
        name,
        comment,
        generic_name,
        exec,
        icon,
        is_flatpak: !flatpak_app_id.is_empty(),
        flatpak_app_id,
        path: PathBuf::new(),
    })
}

/// Strip freedesktop field codes from an `Exec=` line so it's a
/// shell-runnable command on its own:
///
///   * `%f`, `%F`, `%u`, `%U` — file/URL placeholders. Removed.
///   * `%i`, `%c`, `%k`, `%v`, `%m` — mostly icon/caption metadata.
///     Removed.
///   * `%%` — literal `%`. Replaced with `%`.
///   * Flatpak's file-forwarding delimiters — `@@`, `@@u`, `@@U`,
///     `@@f`, `@@F` are removed as whole tokens. (`@@u` is a single
///     token meaning "URL list opening marker"; naively stripping
///     `@@` would leave a stray `u`.)
///
/// Whitespace between removed codes collapses to a single space.
pub fn clean_exec_line(exec: &str) -> String {
    // First pass: strip field codes character by character.
    let mut intermediate = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('f') | Some('F') | Some('u') | Some('U') | Some('i') | Some('c')
                | Some('k') | Some('v') | Some('m') => continue,
                Some('%') => intermediate.push('%'),
                Some(other) => {
                    intermediate.push('%');
                    intermediate.push(other);
                }
                None => intermediate.push('%'),
            }
        } else {
            intermediate.push(c);
        }
    }

    // Second pass: tokenise on whitespace and drop any token that's
    // a Flatpak file-forwarding marker.  Doing this token-by-token
    // (instead of substring-replacing `@@`) means `@@u` cleanly
    // disappears as a unit rather than leaving a stray `u`.
    intermediate
        .split_whitespace()
        .filter(|tok| !is_flatpak_marker(tok))
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_flatpak_marker(tok: &str) -> bool {
    matches!(tok, "@@" | "@@u" | "@@U" | "@@f" | "@@F")
}

/// Enumerate every visible application from the standard XDG paths
/// and the Flatpak per-user + system export trees. Duplicate entries
/// (same desktop file id under multiple paths) are deduplicated by
/// keeping the *last-seen* — matches XDG precedence:
/// `~/.local` > `/usr/local` > `/usr` > `/var/lib/flatpak`.
pub fn enumerate_applications() -> Vec<DesktopEntry> {
    let mut by_id: BTreeMap<String, DesktopEntry> = BTreeMap::new();
    for dir in standard_application_dirs() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                    continue;
                }
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| path.to_string_lossy().to_string());
                if let Some(parsed) = parse_desktop_file(&path) {
                    by_id.insert(id, parsed);
                }
            }
        }
    }
    let mut out: Vec<_> = by_id.into_values().collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// XDG application directories *plus* extra packaging-system trees
/// (Flatpak system + per-user, Snap), in the order they should be
/// searched. The walk order matters because `enumerate_applications`
/// dedupes by file-stem id with last-write-wins — earlier entries
/// can be overridden by later ones, so we put system-wide /
/// packaged-app sources first and user-installed last.
///
/// Sources covered:
///   * Snap (`/var/lib/snapd/desktop/applications`) — separate from
///     XDG; no-op on systems without snapd.
///   * Flatpak system (`/var/lib/flatpak/exports/share/applications`)
///   * `$XDG_DATA_DIRS/applications` for every entry of the env
///     var (default `/usr/local/share/:/usr/share/` per spec).
///     Catches distro-specific data dirs we wouldn't otherwise hit.
///   * Flatpak per-user
///     (`~/.local/share/flatpak/exports/share/applications`)
///   * `$XDG_DATA_HOME/applications` (default
///     `~/.local/share/applications`)
///
/// Result is deduplicated so entries that appear in multiple sources
/// (commonly: Flatpak paths embedded in `$XDG_DATA_DIRS`) aren't
/// walked twice.
pub fn standard_application_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    // Snap apps — published outside the XDG tree.
    dirs.push(PathBuf::from("/var/lib/snapd/desktop/applications"));

    // Flatpak system — usually also in $XDG_DATA_DIRS via
    // /var/lib/flatpak/exports/share, but include defensively for
    // older distros that don't export it through the env var.
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));

    // Walk $XDG_DATA_DIRS dynamically so we pick up whatever the
    // distro configures (often includes Flatpak roots, sometimes
    // `/var/cache/app-info`, sandbox-specific bridges, etc.).
    let xdg_data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .unwrap_or_else(|| std::ffi::OsString::from("/usr/local/share:/usr/share"));
    for d in std::env::split_paths(&xdg_data_dirs) {
        dirs.push(d.join("applications"));
    }

    // Flatpak per-user.
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(&home).join(".local/share/flatpak/exports/share/applications"));
    }

    // Per-user XDG dir (highest precedence so user overrides win).
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(xdg).join("applications"));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }

    // Dedupe while preserving order — first occurrence kept, so the
    // priority order above is honoured for the BTreeMap last-write-
    // wins logic in `enumerate_applications`.
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    dirs.retain(|p| seen.insert(p.clone()));
    dirs
}

/// Substring-match (case-insensitive) over name + generic_name +
/// comment + flatpak_app_id. Returns matches sorted by best-match
/// score (lower position in name is better).
///
/// The editor may swap this out for a proper fuzzy matcher
/// (`nucleo-matcher`, etc.) later — substring match is enough for
/// "type a few letters and pick from a list" UX and adds zero deps.
pub fn search<'a>(entries: &'a [DesktopEntry], query: &str) -> Vec<&'a DesktopEntry> {
    if query.is_empty() {
        return entries.iter().collect();
    }
    let q = query.to_lowercase();

    let mut scored: Vec<(usize, &DesktopEntry)> = entries
        .iter()
        .filter_map(|e| {
            let name_lc = e.name.to_lowercase();
            if let Some(pos) = name_lc.find(&q) {
                return Some((pos, e));
            }
            let gen_lc = e.generic_name.to_lowercase();
            if gen_lc.contains(&q) {
                return Some((1000, e));
            }
            let com_lc = e.comment.to_lowercase();
            if com_lc.contains(&q) {
                return Some((2000, e));
            }
            if e.flatpak_app_id.to_lowercase().contains(&q) {
                return Some((3000, e));
            }
            None
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    scored.into_iter().map(|(_, e)| e).collect()
}

// =============================================================================
// minimal .desktop INI parser
// =============================================================================

fn parse_desktop_entry_section(raw: &str) -> BTreeMap<String, String> {
    let mut kv = BTreeMap::new();
    let mut in_section = false;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('[') {
            // New section header.
            let header = rest.trim_end().trim_end_matches(']');
            in_section = header == "Desktop Entry";
            continue;
        }
        if !in_section {
            continue;
        }
        let mut parts = trimmed.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        // Skip locale variants — `Name[de]=…`, `Comment[fr]=…`, etc.
        if key.contains('[') {
            continue;
        }
        kv.insert(key.to_string(), value.to_string());
    }
    kv
}

#[cfg(test)]
mod tests {
    use super::*;

    const NATIVE: &str = "[Desktop Entry]
Name=Boxy SVG
GenericName=Vector Editor
Comment=Scalable Vector Graphics editor
Type=Application
Exec=/usr/bin/boxy-svg %F
Icon=boxy-svg
Terminal=false
Categories=Graphics
";

    const FLATPAK: &str = "[Desktop Entry]
Name=Boxy SVG
Comment=Scalable Vector Graphics (SVG) editor
Exec=/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=boxy-svg --file-forwarding com.boxy_svg.BoxySVG @@ %f @@
Icon=com.boxy_svg.BoxySVG
Type=Application
Terminal=false
Categories=Graphics
StartupWMClass=Boxy SVG
MimeType=image/svg+xml;
Keywords=SVG;Vector;Graphics;Editor;
X-Flatpak-Tags=proprietary;
X-Flatpak=com.boxy_svg.BoxySVG
";

    const HIDDEN: &str = "[Desktop Entry]
Name=Internal Helper
Type=Application
Exec=/usr/libexec/internal
Icon=internal
NoDisplay=true
";

    const SETTINGS_ACTION: &str = "[Desktop Entry]
Name=Sound Settings Action
Type=Link
URL=settings://sound
";

    #[test]
    fn parses_native_entry() {
        let e = parse_desktop_string(NATIVE).expect("parse");
        assert_eq!(e.name, "Boxy SVG");
        assert_eq!(e.generic_name, "Vector Editor");
        assert_eq!(e.icon, "boxy-svg");
        assert_eq!(e.exec, "/usr/bin/boxy-svg %F");
        assert!(!e.is_flatpak);
        assert!(e.flatpak_app_id.is_empty());
    }

    #[test]
    fn parses_flatpak_entry_and_marks_it() {
        let e = parse_desktop_string(FLATPAK).expect("parse");
        assert_eq!(e.name, "Boxy SVG");
        assert!(e.is_flatpak);
        assert_eq!(e.flatpak_app_id, "com.boxy_svg.BoxySVG");
        assert_eq!(e.icon, "com.boxy_svg.BoxySVG");
    }

    #[test]
    fn rejects_no_display_entries() {
        assert!(parse_desktop_string(HIDDEN).is_none());
    }

    #[test]
    fn rejects_non_application_types() {
        assert!(parse_desktop_string(SETTINGS_ACTION).is_none());
    }

    #[test]
    fn ignores_locale_variants() {
        let raw = "[Desktop Entry]
Type=Application
Exec=foo
Name=English Name
Name[de]=Deutscher Name
Name[fr]=Nom Français
Icon=foo
";
        let e = parse_desktop_string(raw).expect("parse");
        // Locale variants must NOT clobber the unlocalised key.
        assert_eq!(e.name, "English Name");
    }

    #[test]
    fn clean_exec_strips_field_codes_and_flatpak_wrappers() {
        let cleaned = clean_exec_line(
            "/usr/bin/flatpak run --command=boxy-svg --file-forwarding com.boxy_svg.BoxySVG @@ %f @@",
        );
        assert_eq!(
            cleaned,
            "/usr/bin/flatpak run --command=boxy-svg --file-forwarding com.boxy_svg.BoxySVG"
        );

        assert_eq!(clean_exec_line("/usr/bin/foo %F %U %i"), "/usr/bin/foo");
        assert_eq!(clean_exec_line("echo 100%% done"), "echo 100% done");
    }

    #[test]
    fn clean_exec_handles_flatpak_url_forwarding_marker() {
        // Real-world: Archives Flatpak entry uses @@u for URL list.
        // Naively stripping `@@` (without token-awareness) leaves a
        // stray `u` at the end. Verify the token-based stripper
        // handles it.
        let cleaned = clean_exec_line(
            "/usr/bin/flatpak run --command=archives --file-forwarding dev.geopjr.Archives @@u %U @@",
        );
        assert_eq!(
            cleaned,
            "/usr/bin/flatpak run --command=archives --file-forwarding dev.geopjr.Archives"
        );
    }

    #[test]
    fn slice_command_uses_cleaned_exec() {
        let e = parse_desktop_string(FLATPAK).unwrap();
        assert_eq!(
            e.slice_command(),
            "/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=boxy-svg --file-forwarding com.boxy_svg.BoxySVG"
        );
    }

    #[test]
    fn search_ranks_name_matches_first() {
        let entries = vec![
            DesktopEntry {
                name: "Code".into(),
                generic_name: "".into(),
                comment: "".into(),
                exec: "code".into(),
                icon: "".into(),
                is_flatpak: false,
                flatpak_app_id: "".into(),
                path: PathBuf::new(),
            },
            DesktopEntry {
                name: "Notes".into(),
                generic_name: "Code editor".into(),
                comment: "".into(),
                exec: "notes".into(),
                icon: "".into(),
                is_flatpak: false,
                flatpak_app_id: "".into(),
                path: PathBuf::new(),
            },
        ];
        let results = search(&entries, "code");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "Code"); // name match outranks generic_name
        assert_eq!(results[1].name, "Notes");
    }

    #[test]
    fn search_handles_empty_query() {
        let entries = vec![DesktopEntry {
            name: "X".into(),
            generic_name: "".into(),
            comment: "".into(),
            exec: "x".into(),
            icon: "".into(),
            is_flatpak: false,
            flatpak_app_id: "".into(),
            path: PathBuf::new(),
        }];
        let results = search(&entries, "");
        assert_eq!(results.len(), 1);
    }
}
