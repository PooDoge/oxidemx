//! Risk-tiered approval classifier for autonomous agent tool calls.
//!
//! Decides whether a tool call should be auto-run, held for approval, or
//! outright denied — without touching any UI or blocking the event loop.
//!
//! # Architecture
//!
//! ```text
//! ApprovalClassifier::classify(tool, args, cwd)
//!   ├─ read-only tools ──────────────────────────→ AutoAllow
//!   ├─ file-mutation tools ──→ reversible::classify_path
//!   │     ├─ true  ──────────────────────────────→ AutoAllowIfReversible
//!   │     └─ false ──────────────────────────────→ Ask
//!   ├─ execute_command ──────→ shell::classify_command
//!   │     ├─ metachar/compound ──────────────────→ Ask
//!   │     ├─ hard-deny list ──────────────────────→ AutoDeny
//!   │     ├─ known read-only ─────────────────────→ AutoAllow
//!   │     └─ unknown ─────────────────────────────→ Ask
//!   └─ host/network/unknown ────────────────────→ Ask
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod reversible;
pub mod shell;

use std::path::Path;

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Risk tier assigned to a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    /// Run immediately with no prompt.
    AutoAllow,
    /// Run immediately only if the target file is git-tracked (reversible).
    AutoAllowIfReversible,
    /// Hold and ask the user before proceeding.
    Ask,
    /// Reject; do not execute.
    AutoDeny,
}

/// The outcome of a classification, combining a [`Tier`] with a human-readable
/// explanation suitable for logging or surfacing in a confirmation dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    /// Risk tier.
    pub tier: Tier,
    /// Human-readable explanation of why this tier was chosen.
    pub reason: String,
}

/// Extra per-tool overrides merged on top of the built-in rule set.
///
/// Keep this intentionally minimal — the built-in rules cover the common cases.
/// Add entries here only for project-specific tools.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClassifierConfig {
    /// Tool names that should always be auto-allowed regardless of args.
    pub extra_allow: Vec<String>,
    /// Tool names that should always be auto-denied.
    pub extra_deny: Vec<String>,
}

/// The top-level classifier.
///
/// Construct via [`ApprovalClassifier::default()`] (uses built-in rules only)
/// or [`ApprovalClassifier::from_config`] to layer extra overrides.
#[derive(Debug, Clone, Default)]
pub struct ApprovalClassifier {
    config: ClassifierConfig,
}

// ─────────────────────────────────────────────────────────────────────────────
// Tool categories (built-in)
// ─────────────────────────────────────────────────────────────────────────────

/// Tools that only read state — always safe to auto-run.
const READ_ONLY_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "search_file",
    "parse_document",
    "google_search",
];

/// Tools that mutate files — safe iff the target is git-tracked.
const FILE_MUTATION_TOOLS: &[&str] = &[
    "write_file",
    "edit_file",
    "apply_patch",
    "delete_file",
    "delete",
];

// ─────────────────────────────────────────────────────────────────────────────
// Implementation
// ─────────────────────────────────────────────────────────────────────────────

impl ApprovalClassifier {
    /// Create a classifier with extra overrides on top of the built-in rules.
    pub fn from_config(config: ClassifierConfig) -> Self {
        Self { config }
    }

    /// Classify a tool call.
    ///
    /// `tool` is the tool name (e.g. `"read_file"`), `args` is the JSON
    /// arguments object, and `cwd` is the working directory of the agent
    /// (used for path-escape and git-tracking checks).
    pub fn classify(&self, tool: &str, args: &serde_json::Value, cwd: &Path) -> Decision {
        // ── Config overrides: deny wins over allow ──
        if self.config.extra_deny.iter().any(|t| t == tool) {
            return Decision {
                tier: Tier::AutoDeny,
                reason: format!("`{tool}` is in the configured deny list"),
            };
        }
        if self.config.extra_allow.iter().any(|t| t == tool) {
            return Decision {
                tier: Tier::AutoAllow,
                reason: format!("`{tool}` is in the configured allow list"),
            };
        }

        // ── Built-in rules ──
        if READ_ONLY_TOOLS.contains(&tool) {
            return Decision {
                tier: Tier::AutoAllow,
                reason: format!("`{tool}` is a read-only tool"),
            };
        }

        if FILE_MUTATION_TOOLS.contains(&tool) {
            let path = extract_path_arg(args);
            return match path {
                Some(p) if reversible::classify_path(cwd, p) => Decision {
                    tier: Tier::AutoAllowIfReversible,
                    reason: format!(
                        "`{tool}` targets `{p}` which is git-tracked — reversible"
                    ),
                },
                Some(p) => Decision {
                    tier: Tier::Ask,
                    reason: format!(
                        "`{tool}` targets `{p}` which is not git-tracked or not under cwd"
                    ),
                },
                None => Decision {
                    tier: Tier::Ask,
                    reason: format!("`{tool}` has no recognisable path argument — review required"),
                },
            };
        }

        if tool == "execute_command" {
            let cmd = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if cmd.is_empty() {
                return Decision {
                    tier: Tier::Ask,
                    reason: "empty or unrecognised command argument".into(),
                };
            }
            return shell::classify_command(cmd);
        }

        // ── Conservative fallback: host/network/unknown → Ask ──
        Decision {
            tier: Tier::Ask,
            reason: format!("`{tool}` is an unknown tool — review required"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Try to extract a file path from common arg key names used by mutation tools.
fn extract_path_arg(args: &serde_json::Value) -> Option<&str> {
    for key in &["file_path", "path", "source"] {
        if let Some(v) = args.get(key).and_then(|v| v.as_str()) {
            return Some(v);
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (verbatim from task brief)
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: stage and commit a single file into the repo's HEAD.
    ///
    /// Creates an initial commit when HEAD doesn't exist yet.
    #[cfg(test)]
    pub(crate) fn commit_file(repo: &git2::Repository, relpath: &str) {
        // Make sure the file is on disk (caller should have written it already).
        let workdir = repo.workdir().expect("bare repos not supported in tests");
        let abs = workdir.join(relpath);
        assert!(abs.exists(), "file must exist before commit_file: {relpath}");

        // Stage the file.
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(relpath)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();

        // Build commit metadata.
        let sig = git2::Signature::now("test", "test@example.com").unwrap();

        // Determine parent: none for the initial commit.
        let parent_commit;
        let parents: Vec<&git2::Commit> = match repo.head() {
            Ok(head_ref) => {
                parent_commit = head_ref.peel_to_commit().unwrap();
                vec![&parent_commit]
            }
            Err(_) => vec![], // initial commit
        };

        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &format!("add {relpath}"),
            &tree,
            &parents,
        )
        .unwrap();
    }

    #[test]
    fn reads_auto_allow_and_force_push_auto_deny() {
        let c = ApprovalClassifier::default();
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            c.classify(
                "read_file",
                &serde_json::json!({"file_path": "x"}),
                d.path()
            )
            .tier,
            Tier::AutoAllow
        );
        assert_eq!(
            c.classify(
                "execute_command",
                &serde_json::json!({"command": "git push --force"}),
                d.path()
            )
            .tier,
            Tier::AutoDeny
        );
        assert_eq!(
            c.classify(
                "execute_command",
                &serde_json::json!({"command": "cargo test"}),
                d.path()
            )
            .tier,
            Tier::AutoAllow
        );
    }

    #[test]
    fn metachars_downgrade_to_ask() {
        let c = ApprovalClassifier::default();
        let d = tempfile::tempdir().unwrap();
        // 'cargo test' alone is AutoAllow, but chained it must become Ask
        assert_eq!(
            c.classify(
                "execute_command",
                &serde_json::json!({"command": "cargo test && rm -rf /"}),
                d.path()
            )
            .tier,
            Tier::Ask
        );
    }

    #[test]
    fn reversible_tracked_file_in_repo() {
        let d = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(d.path()).unwrap();
        std::fs::write(d.path().join("tracked.rs"), "x").unwrap();
        // stage+commit tracked.rs (use git2 index/commit; helper in the test)
        commit_file(&repo, "tracked.rs");
        assert!(reversible::classify_path(d.path(), "tracked.rs"));
        assert!(!reversible::classify_path(d.path(), "untracked.rs"));
        assert!(!reversible::classify_path(d.path(), "../escape.rs"));
    }
}
