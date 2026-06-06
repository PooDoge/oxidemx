//! Conditional slice predicates.
//!
//! Each slice can carry an optional `visible_if` predicate; the
//! overlay evaluates it before rendering and skips slices whose
//! predicate is false. Unmatched slices are simply absent from the
//! ring (the remaining slices keep their slot positions — we don't
//! re-pack to fill gaps, since the user picked those slot positions
//! deliberately).
//!
//! The evaluator is pure Rust (filesystem + env reads only); no
//! D-Bus, no GTK, no daemon roundtrip. Predicates that genuinely
//! need IPC (e.g. "audio is playing" via MPRIS) will land as a
//! separate variant once the overlay's D-Bus surface gains a
//! pluggable evaluator hook — for now the spec-able ones are:
//!
//!   * `Always` — default, slice always renders.
//!   * `Never` — debugging / temporarily hiding without removing.
//!   * `Executable { name }` — `name` is on $PATH.
//!   * `FileExists { path }` — `path` exists on disk (env-vars in
//!     the path are expanded via `~` and `$VAR` shell-style).
//!   * `ProcessRunning { comm }` — a process with comm `comm`
//!     exists in /proc.
//!   * `EnvSet { var }` — environment variable `var` is set
//!     (any value).
//!   * `EnvEquals { var, value }` — env var `var` equals `value`.
//!   * `All { conditions }` — every nested predicate true.
//!   * `Any { conditions }` — at least one nested predicate true.
//!   * `Not { condition }` — boxed nested predicate inverted.
//!
//! Schema:
//!
//! ```json
//! "visible_if": { "kind": "executable", "name": "git" }
//! "visible_if": { "kind": "all", "conditions": [
//!   { "kind": "executable", "name": "spotify" },
//!   { "kind": "process_running", "comm": "spotify" }
//! ]}
//! ```

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Visibility predicate for a slice. Stored on `Slice::visible_if`
/// (added to the config schema in this commit).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    Always,
    Never,
    Executable { name: String },
    FileExists { path: String },
    ProcessRunning { comm: String },
    EnvSet { var: String },
    EnvEquals { var: String, value: String },
    All { conditions: Vec<Condition> },
    Any { conditions: Vec<Condition> },
    Not { condition: Box<Condition> },
}

impl Default for Condition {
    fn default() -> Self {
        Condition::Always
    }
}

impl Condition {
    /// Evaluate using the running process's environment + filesystem.
    /// Heavy lookups (PATH walk, /proc scan) are not cached here —
    /// the overlay caches predicate results per show() so a flurry
    /// of menu opens doesn't repeatedly stat the same paths.
    pub fn eval(&self) -> bool {
        match self {
            Condition::Always => true,
            Condition::Never => false,
            Condition::Executable { name } => executable_on_path(name),
            Condition::FileExists { path } => Path::new(&expand(path)).exists(),
            Condition::ProcessRunning { comm } => process_with_comm(comm).is_some(),
            Condition::EnvSet { var } => std::env::var_os(var).is_some(),
            Condition::EnvEquals { var, value } => {
                std::env::var(var).map(|v| &v == value).unwrap_or(false)
            }
            Condition::All { conditions } => conditions.iter().all(Self::eval),
            Condition::Any { conditions } => conditions.iter().any(Self::eval),
            Condition::Not { condition } => !condition.eval(),
        }
    }
}

/// Search PATH for an executable file matching `name`.
fn executable_on_path(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.contains('/') {
        // Absolute / relative path — check that file directly.
        return is_executable(Path::new(name));
    }
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return true;
        }
    }
    false
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(md) => md.is_file() && md.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Find a /proc entry whose `comm` matches `target`. Returns the PID
/// of the first match (intentionally Option<u32> so callers can act
/// on it later — for now `eval()` only checks the existence).
pub fn process_with_comm(target: &str) -> Option<u32> {
    let proc = match std::fs::read_dir("/proc") {
        Ok(rd) => rd,
        Err(_) => return None, // Non-Linux or permission-denied
    };
    for entry in proc.flatten() {
        let pid: u32 = match entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse().ok())
        {
            Some(p) => p,
            None => continue,
        };
        let comm_path = entry.path().join("comm");
        if let Ok(s) = std::fs::read_to_string(&comm_path) {
            if s.trim() == target {
                return Some(pid);
            }
        }
    }
    None
}

/// `~`/`$VAR` expansion for predicate paths. Conservative — `$VAR`
/// only expands when the value is *set*; unset references stay
/// literal so the predicate fails clearly via the path-not-found
/// branch instead of silently matching the wrong file.
fn expand(input: &str) -> PathBuf {
    let mut s = input.to_string();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            s = PathBuf::from(home)
                .join(rest)
                .to_string_lossy()
                .into_owned();
        }
    } else if s == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            s = home.to_string_lossy().into_owned();
        }
    }

    // Minimal $VAR expansion. Doesn't handle ${VAR} or quoting —
    // those are unlikely in a config-file path field, and getting
    // shell-quoting right is out of scope for what's basically a
    // convenience.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            let mut name = String::new();
            while let Some(&n) = chars.peek() {
                if n.is_ascii_alphanumeric() || n == '_' {
                    name.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            if name.is_empty() {
                out.push('$');
            } else if let Some(val) = std::env::var_os(&name) {
                out.push_str(&val.to_string_lossy());
            } else {
                out.push('$');
                out.push_str(&name);
            }
        } else {
            out.push(c);
        }
    }
    PathBuf::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_is_true_never_is_false() {
        assert!(Condition::Always.eval());
        assert!(!Condition::Never.eval());
    }

    #[test]
    fn executable_finds_real_binary() {
        // `sh` is on $PATH on every supported system.
        assert!(Condition::Executable {
            name: "sh".into()
        }
        .eval());
        assert!(!Condition::Executable {
            name: "definitely-not-a-real-binary-xyzzy".into()
        }
        .eval());
    }

    #[test]
    fn executable_handles_absolute_path() {
        assert!(Condition::Executable {
            name: "/bin/sh".into()
        }
        .eval());
        assert!(!Condition::Executable {
            name: "/nonexistent/path/to/nothing".into()
        }
        .eval());
    }

    #[test]
    fn file_exists_works_with_tilde_expansion() {
        // Use an existing file: $HOME itself (a directory exists()
        // returns true on).
        if std::env::var_os("HOME").is_some() {
            assert!(Condition::FileExists { path: "~".into() }.eval());
        }
        assert!(!Condition::FileExists {
            path: "/nope/never/here".into()
        }
        .eval());
    }

    #[test]
    fn env_set_and_equals() {
        // PATH is set on every system.
        assert!(Condition::EnvSet {
            var: "PATH".into()
        }
        .eval());
        assert!(!Condition::EnvSet {
            var: "OXIDEMX_TEST_NEVER_SET_xyzzy".into()
        }
        .eval());

        // Set a known env var for this thread and verify EnvEquals.
        std::env::set_var("OXIDEMX_TEST_VAR", "hello");
        assert!(Condition::EnvEquals {
            var: "OXIDEMX_TEST_VAR".into(),
            value: "hello".into()
        }
        .eval());
        assert!(!Condition::EnvEquals {
            var: "OXIDEMX_TEST_VAR".into(),
            value: "wrong".into()
        }
        .eval());
        std::env::remove_var("OXIDEMX_TEST_VAR");
    }

    #[test]
    fn process_running_finds_self() {
        // The current process's comm is `oxidemx_shared-…` (cargo
        // test binary). We can't predict that exactly, so just sanity
        // check by looking up `init` (PID 1) which is on every Linux
        // system as either `systemd` or similar.
        let found = process_with_comm("systemd").is_some()
            || process_with_comm("init").is_some();
        assert!(found, "no init/systemd process visible in /proc");
    }

    #[test]
    fn all_combines_with_and() {
        let p = Condition::All {
            conditions: vec![
                Condition::Always,
                Condition::Always,
            ],
        };
        assert!(p.eval());
        let p = Condition::All {
            conditions: vec![Condition::Always, Condition::Never],
        };
        assert!(!p.eval());
    }

    #[test]
    fn any_combines_with_or() {
        let p = Condition::Any {
            conditions: vec![Condition::Never, Condition::Always],
        };
        assert!(p.eval());
        let p = Condition::Any {
            conditions: vec![Condition::Never, Condition::Never],
        };
        assert!(!p.eval());
    }

    #[test]
    fn not_inverts() {
        assert!(Condition::Not {
            condition: Box::new(Condition::Never),
        }
        .eval());
        assert!(!Condition::Not {
            condition: Box::new(Condition::Always),
        }
        .eval());
    }

    #[test]
    fn serde_round_trip() {
        let p = Condition::All {
            conditions: vec![
                Condition::Executable { name: "git".into() },
                Condition::Not {
                    condition: Box::new(Condition::EnvSet {
                        var: "BORING".into(),
                    }),
                },
            ],
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Condition = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
        // Wire-format snapshot: we use snake_case `kind` discriminator.
        assert!(json.contains(r#""kind":"all""#));
        assert!(json.contains(r#""kind":"executable""#));
        assert!(json.contains(r#""kind":"not""#));
    }

    #[test]
    fn deserializes_human_friendly_schema() {
        let json = r#"{
            "kind": "any",
            "conditions": [
                { "kind": "executable", "name": "spotify" },
                { "kind": "process_running", "comm": "spotify" }
            ]
        }"#;
        let p: Condition = serde_json::from_str(json).unwrap();
        match p {
            Condition::Any { conditions } => assert_eq!(conditions.len(), 2),
            _ => panic!("wrong variant"),
        }
    }
}
