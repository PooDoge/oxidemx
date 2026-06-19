//! Per-project journal — append-only JSONL record for the SP-Learn loop.
//!
//! [`Journal`] appends one [`JournalEntry`] JSON object per line to a
//! caller-supplied path (Task 6 will pass `ProjectPaths::journal_path()`).
//! The format is intentionally transparent: each line is a self-describing
//! JSON object with a `"kind"` discriminant, making it trivially
//! grep-able/jq-able for offline analysis.
#![forbid(unsafe_code)]

use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::AgentdError;

// ── JournalEntry ──────────────────────────────────────────────────────────────

/// One record in the project journal.
///
/// Uses `#[serde(tag = "kind")]` so every line is self-describing:
/// `{"kind":"Turn", ...}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum JournalEntry {
    /// A completed conversation turn (prompt → reply).
    Turn {
        thread: String,
        prompt: String,
        reply: String,
        /// `(prompt_tokens, completion_tokens)`.
        usage: (u32, u32),
        /// Unix timestamp (milliseconds).
        ts: u64,
    },
    /// A tool call result.
    Tool {
        thread: String,
        name: String,
        /// `true` = tool returned Ok, `false` = returned Err.
        ok: bool,
        ts: u64,
    },
    /// An approval decision (allow/deny/always/edit).
    Approval {
        request_id: String,
        tool: String,
        /// Human-readable verdict label (e.g. `"allow"`, `"deny"`, `"always"`).
        verdict: String,
        /// Optional reason / edited payload description.
        reason: Option<String>,
        ts: u64,
    },
    /// A conductor flow lifecycle event.
    FlowEvent {
        run_id: String,
        event: String,
        ts: u64,
    },
}

// ── Journal ───────────────────────────────────────────────────────────────────

/// Append-only JSONL journal for a single project.
///
/// Constructed with a file path; the parent directory must already exist
/// (Task 6 ensures this via `ProjectPaths`). Each call to [`Journal::record`]
/// opens the file in append mode, writes one JSON line, and flushes.
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    /// Create a journal that will write to `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Append `entry` as a single JSON line to the journal file.
    ///
    /// The file is created if it does not exist. Errors are propagated as
    /// [`AgentdError::Io`].
    pub fn record(&self, entry: &JournalEntry) -> Result<(), AgentdError> {
        // Ensure parent directory exists.
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(AgentdError::from)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(AgentdError::from)?;
        let line =
            serde_json::to_string(entry).map_err(|e| AgentdError::Io(e.to_string()))?;
        writeln!(file, "{line}").map_err(AgentdError::from)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_appends_entries() {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::new(d.path().join("journal.jsonl"));
        j.record(&JournalEntry::Turn {
            thread: "t1".into(),
            prompt: "p".into(),
            reply: "r".into(),
            usage: (1, 2),
            ts: 1000,
        })
        .unwrap();
        j.record(&JournalEntry::Approval {
            request_id: "a1".into(),
            tool: "run".into(),
            verdict: "deny".into(),
            reason: Some("nope".into()),
            ts: 2000,
        })
        .unwrap();
        let lines =
            std::fs::read_to_string(d.path().join("journal.jsonl")).unwrap();
        assert_eq!(lines.lines().count(), 2);
        assert!(
            lines.contains("\"kind\":\"Turn\"") && lines.contains("\"kind\":\"Approval\"")
        );
    }
}
