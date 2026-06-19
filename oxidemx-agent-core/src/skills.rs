//! Skill discovery + enabled-set persistence.
//!
//! Skills follow the cross-tool `SKILL.md` convention — Claude Code,
//! Antigravity (Gemini), and the npx skill-managers all read/write the
//! same shape: a directory whose `SKILL.md` carries `name:` /
//! `description:` frontmatter, then a markdown body of instructions. We
//! discover them NATIVELY from configurable roots (no Node/npx
//! dependency, works offline), so OxideMX is compatible with whatever
//! authored them.
//!
//! Slice 1 (this module + the Skills panel + palette) covers discovery
//! and the enabled set. Slice 2 wires the enabled set into the agent
//! (a `use_skill` tool + name/description in the system prompt, i.e.
//! progressive disclosure) — `read_body` is provided here for that.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A discovered skill.
#[derive(Clone, Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Path to the `SKILL.md` itself (read on demand by the agent's
    /// `use_skill` tool — slice 2).
    #[allow(dead_code)]
    pub path: PathBuf,
    /// Where it came from, for the UI badge.
    pub source: &'static str,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The global skill-scan roots (home-relative Claude + Antigravity directories
/// plus any extras from `skills.json`) — WITHOUT the project-local
/// `.claude/skills` entry that `roots()` appends as a relative path.
///
/// `agentd`'s project model calls this to build its merged skill root list
/// (global roots first, then project-local roots last so project wins on name
/// collision). Returning `PathBuf` values instead of `(PathBuf, label)` pairs
/// keeps the public API minimal — callers that need labels can reconstruct
/// them.
pub fn global_skill_roots() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = home() {
        v.push(h.join(".claude/skills"));
        v.push(h.join(".gemini/antigravity/skills"));
    }
    for r in load_config().roots {
        v.push(PathBuf::from(r));
    }
    v
}

/// Default scan roots (Claude + Antigravity + project CWD) plus any
/// extra roots configured in `skills.json`.
fn roots() -> Vec<(PathBuf, &'static str)> {
    let mut v = Vec::new();
    if let Some(h) = home() {
        v.push((h.join(".claude/skills"), "Claude"));
        v.push((h.join(".gemini/antigravity/skills"), "Antigravity"));
    }
    // Project-level, relative to the CWD if the agent was pointed at one.
    v.push((PathBuf::from(".claude/skills"), "Project"));
    for r in load_config().roots {
        v.push((PathBuf::from(r), "Custom"));
    }
    v
}

/// Pull `name` / `description` from the SKILL.md frontmatter (between
/// the first two `---` fences). Handles quoted and bare values; ignores
/// indented (nested `metadata:`) keys since it requires the key at the
/// start of a trimmed line.
fn parse_frontmatter(src: &str) -> (Option<String>, Option<String>) {
    let mut parts = src.splitn(3, "---");
    let front = match (parts.next(), parts.next()) {
        (Some(_), Some(f)) => f,
        _ => return (None, None),
    };
    let field = |key: &str| -> Option<String> {
        front.lines().find_map(|l| {
            let l = l.trim();
            l.strip_prefix(key)
                .and_then(|r| r.trim_start().strip_prefix(':'))
                .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|v| !v.is_empty())
        })
    };
    (field("name"), field("description"))
}

/// Discover all skills across the configured roots, de-duplicated by
/// name (earlier roots win), sorted by name.
pub fn discover() -> Vec<Skill> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (root, source) in roots() {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in rd.flatten() {
            // `e.path()` is the entry (symlinks are followed by the
            // exists()/read below — many skills are symlinked into the
            // root from external repos).
            let skill_md = e.path().join("SKILL.md");
            let Ok(src) = std::fs::read_to_string(&skill_md) else {
                continue;
            };
            let (name, desc) = parse_frontmatter(&src);
            let name = name.unwrap_or_else(|| e.file_name().to_string_lossy().to_string());
            if name.is_empty() || !seen.insert(name.clone()) {
                continue;
            }
            out.push(Skill {
                name,
                description: desc.unwrap_or_default(),
                path: skill_md,
                source,
            });
        }
    }
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// A discovered prompt-template command (`.claude/commands/<name>.md`).
/// The file body is a prompt with `$ARGUMENTS` / `$1`..`$9` placeholders
/// — the Claude slash-command convention.
#[derive(Clone, Debug)]
pub struct PromptCommand {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

/// Command roots: `<dir>/commands/*.md` siblings of the skill roots.
fn command_roots() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = home() {
        v.push(h.join(".claude/commands"));
        v.push(h.join(".gemini/antigravity/commands"));
    }
    v.push(PathBuf::from(".claude/commands"));
    v
}

/// Discover prompt-template commands across the command roots
/// (top-level `*.md` only), de-duplicated by name, sorted.
pub fn discover_commands() -> Vec<PromptCommand> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for root in command_roots() {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("md") {
                continue;
            }
            let name = match path.file_stem().and_then(|s| s.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if !seen.insert(name.clone()) {
                continue;
            }
            // Optional `description:` frontmatter; else first non-empty
            // body line as a hint.
            let src = std::fs::read_to_string(&path).unwrap_or_default();
            let (_n, desc) = parse_frontmatter(&src);
            let description = desc.unwrap_or_else(|| {
                command_body_of(&src)
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect()
            });
            out.push(PromptCommand {
                name,
                description,
                path,
            });
        }
    }
    out.sort_by_key(|c| c.name.to_lowercase());
    out
}

/// Body of a command/skill file with the leading `---` frontmatter
/// (if any) stripped.
fn command_body_of(src: &str) -> String {
    if src.trim_start().starts_with("---") {
        let mut parts = src.splitn(3, "---");
        if let (Some(_), Some(_), Some(body)) = (parts.next(), parts.next(), parts.next()) {
            return body.trim_start().to_string();
        }
    }
    src.to_string()
}

/// Render a command's prompt: substitute `$ARGUMENTS` with the full arg
/// string and `$1`..`$9` with positional words. If the template has no
/// placeholders, the args are appended on a new line.
pub fn render_command(path: &Path, args: &str) -> Option<String> {
    let src = std::fs::read_to_string(path).ok()?;
    let mut body = command_body_of(&src);
    let has_placeholder =
        body.contains("$ARGUMENTS") || (1..=9).any(|i| body.contains(&format!("${i}")));
    if has_placeholder {
        body = body.replace("$ARGUMENTS", args);
        for (i, word) in args.split_whitespace().enumerate().take(9) {
            body = body.replace(&format!("${}", i + 1), word);
        }
        // Clear any unfilled positionals.
        for i in 1..=9 {
            body = body.replace(&format!("${i}"), "");
        }
    } else if !args.trim().is_empty() {
        body.push_str("\n\n");
        body.push_str(args);
    }
    Some(body.trim().to_string())
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct SkillsConfig {
    #[serde(default)]
    enabled: Vec<String>,
    #[serde(default)]
    roots: Vec<String>,
}

fn config_path() -> Option<PathBuf> {
    home().map(|h| h.join(".config/oxidemx/skills.json"))
}

fn load_config() -> SkillsConfig {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(c: &SkillsConfig) {
    let Some(p) = config_path() else { return };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(c) {
        let _ = std::fs::write(p, json);
    }
}

/// Names of the skills the user has enabled (the candidate pool the
/// agent may draw on).
pub fn enabled_set() -> HashSet<String> {
    load_config().enabled.into_iter().collect()
}

/// Enable or disable a skill by name (persisted to `skills.json`).
pub fn set_enabled(name: &str, on: bool) {
    let mut c = load_config();
    c.enabled.retain(|n| n != name);
    if on {
        c.enabled.push(name.to_string());
    }
    save_config(&c);
}

/// The instruction body of a skill (everything after the frontmatter).
/// Used by the agent's `use_skill` tool (slice 2).
#[allow(dead_code)]
pub fn read_body(path: &Path) -> Option<String> {
    let src = std::fs::read_to_string(path).ok()?;
    let mut parts = src.splitn(3, "---");
    match (parts.next(), parts.next(), parts.next()) {
        (Some(_), Some(_), Some(body)) => Some(body.trim().to_string()),
        _ => Some(src),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn discovery_finds_skills() {
        let skills = super::discover();
        eprintln!("discovered {} skills:", skills.len());
        for s in &skills {
            eprintln!(
                "  [{}] {} — {}",
                s.source,
                s.name,
                &s.description.chars().take(60).collect::<String>()
            );
        }
        assert!(!skills.is_empty(), "expected to discover skills");
    }

    #[test]
    fn render_command_substitutes_args() {
        let dir = std::env::temp_dir().join("oxidemx-cmd-test");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("deploy.md");
        std::fs::write(
            &f,
            "---\ndescription: deploy it\n---\nDeploy to $1 with notes: $ARGUMENTS",
        )
        .unwrap();
        let out = super::render_command(&f, "staging fast rollback").unwrap();
        assert!(out.contains("Deploy to staging"), "got: {out}");
        assert!(out.contains("notes: staging fast rollback"), "got: {out}");

        // No-placeholder template appends args.
        let g = dir.join("note.md");
        std::fs::write(&g, "Summarize this:").unwrap();
        let out2 = super::render_command(&g, "the meeting").unwrap();
        assert_eq!(out2, "Summarize this:\n\nthe meeting");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
